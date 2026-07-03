//! Map a Hugging Face kernel [`Variant`] onto conda metadata.
//!
//! This is the heart of "Mode A": we do not compile anything, we describe each
//! prebuilt Hub variant as a conda package so that the *solver* — not a runtime
//! probe like `get_kernel()` — selects the right one, and the lockfile pins it.
//!
//! | HF variant axis        | conda expression                          |
//! |------------------------|-------------------------------------------|
//! | `torch26`              | `depends: pytorch 2.6.*`                  |
//! | `cxx98` / `cxx11`      | filter (conda-forge is cxx11-ABI only)    |
//! | `cu118`                | `constrains: __cuda >=11.8` + cuda-version|
//! | `x86_64` / `linux`     | `subdir: linux-64`                        |
//! | compute capabilities   | `constrains: __cuda_arch >=<min>` (CEP46) |

use thiserror::Error;

use crate::variant::{ComputeKind, Variant};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum UnsupportedVariant {
    #[error("{0}: cxx98 ABI is incompatible with conda-forge (cxx11)")]
    Cxx98(String),
    #[error("no conda subdir for {arch}-{os}")]
    NoSubdir { arch: String, os: String },
}

/// SDK-neutral description of one output package. The backend adapter turns
/// this into the pixi SDK's recipe model (name/version/build/depends/constrains).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CondaRecord {
    pub name: String,
    pub version: String,
    pub build: String,
    pub build_number: u64,
    pub subdir: String,
    pub depends: Vec<String>,
    pub constrains: Vec<String>,
    /// carried through for provenance / the build phase
    pub variant: String,
    // --- structured pieces for per-variant recipe templating ---
    /// e.g. `pytorch 2.6.*`
    pub torch_pin: String,
    /// e.g. `11.8`; None for cpu/metal.
    pub cuda_floor: Option<String>,
}

pub fn subdir_for(variant: &Variant) -> Result<&'static str, UnsupportedVariant> {
    let sd = match (variant.arch.as_str(), variant.os.as_str()) {
        ("x86_64", "linux") => "linux-64",
        ("aarch64", "linux") => "linux-aarch64",
        ("x86_64", "windows") => "win-64",
        ("aarch64", "darwin") => "osx-arm64",
        ("x86_64", "darwin") => "osx-64",
        _ => {
            return Err(UnsupportedVariant::NoSubdir {
                arch: variant.arch.clone(),
                os: variant.os.clone(),
            })
        }
    };
    Ok(sd)
}

/// Options controlling how variants are mapped.
#[derive(Debug, Clone)]
pub struct MapOptions {
    pub package_name: String,
    pub version: String,
    pub build_number: u64,
    /// `cuda-capabilities` from the kernel's `build.toml`; empty = "all supported".
    pub cuda_capabilities: Vec<String>,
    pub require_cxx11: bool,
}

/// Turn one build variant into a [`CondaRecord`].
pub fn map_variant(variant: &Variant, opts: &MapOptions) -> Result<CondaRecord, UnsupportedVariant> {
    // Drop only the pre-C++11 ABI; Windows/metal variants carry no ABI tag and
    // have no libstdc++ ABI concern, so they stay.
    if opts.require_cxx11 && variant.is_cxx98() {
        return Err(UnsupportedVariant::Cxx98(variant.raw.clone()));
    }

    let subdir = subdir_for(variant)?.to_string();
    let mut depends = Vec::new();
    let mut constrains = Vec::new();

    // --- framework (torch) ---------------------------------------------------
    // Pin to the exact minor; conda-forge's pytorch build then pulls the
    // matching cuda-version, which is the real ABI contract for the extension.
    // HF's framework tag `torch` is the conda package `pytorch`.
    let framework_pkg = conda_framework_name(&variant.framework);
    let torch_pin = format!("{framework_pkg} {}.*", variant.framework_version);
    depends.push(torch_pin.clone());

    // --- compute backend -----------------------------------------------------
    let mut cuda_floor = None;
    match variant.compute_kind {
        ComputeKind::Cuda => {
            let v = variant.compute_version.as_deref().expect("cuda has version");
            cuda_floor = Some(v.to_string());
            // __cuda reports the driver's max supported CUDA version; the driver
            // must be at least as new as the toolkit the kernel was built against.
            constrains.push(format!("__cuda >={v}"));
            // Keep the kernel's cuda-version aligned with whatever pytorch drags in.
            constrains.push(format!("cuda-version >={v}"));
        }
        ComputeKind::Rocm => {
            let v = variant.compute_version.as_deref().expect("rocm has version");
            constrains.push(format!("__hip >={v}"));
        }
        ComputeKind::Cpu | ComputeKind::Metal => {}
    }

    // --- compute capability -> __cuda_arch (CEP 0046) ------------------------
    if variant.compute_kind == ComputeKind::Cuda && !opts.cuda_capabilities.is_empty() {
        if let Some(floor) = min_capability(&opts.cuda_capabilities) {
            // A fatbin built for {8.0, 9.0} needs a GPU of at least the lowest
            // capability present; higher GPUs stay compatible (same-major SASS /
            // PTX JIT). Refuse anything below the floor rather than crash at load.
            constrains.push(format!("__cuda_arch >={floor}"));
        }
    }

    Ok(CondaRecord {
        name: opts.package_name.clone(),
        version: opts.version.clone(),
        build: build_string(variant),
        build_number: opts.build_number,
        subdir,
        depends,
        constrains,
        variant: variant.raw.clone(),
        torch_pin,
        cuda_floor,
    })
}

/// HF framework tag -> conda package name.
fn conda_framework_name(framework: &str) -> &str {
    match framework {
        "torch" => "pytorch",
        other => other,
    }
}

/// Human-readable, unique-per-variant build string (subdir carries arch/os).
fn build_string(variant: &Variant) -> String {
    let fw = format!(
        "{}{}",
        variant.framework,
        variant.framework_version.replace('.', "")
    );
    let compute = match &variant.compute_version {
        Some(v) => format!("{}{}", variant.compute_kind.as_str(), v.replace('.', "")),
        None => variant.compute_kind.as_str().to_string(),
    };
    // Omit the ABI segment when the variant carried none (Windows/metal).
    if variant.has_abi_tag() {
        format!("{fw}_{}_{compute}", variant.cxx_abi)
    } else {
        format!("{fw}_{compute}")
    }
}

fn cap_key(cap: &str) -> (u32, u32) {
    let (major, minor) = cap.split_once('.').unwrap_or((cap, "0"));
    (major.parse().unwrap_or(0), minor.parse().unwrap_or(0))
}

/// The `__cuda_arch` floor for a repo: the lowest capability its fatbins list
/// (constant across variants). `None` when unspecified ("all capabilities").
pub fn cuda_arch_floor(caps: &[String]) -> Option<String> {
    min_capability(caps)
}

/// Lowest capability in the list, normalized to `major.minor`.
fn min_capability(caps: &[String]) -> Option<String> {
    caps.iter()
        .min_by_key(|c| cap_key(c))
        .map(|c| {
            let (maj, min) = cap_key(c);
            format!("{maj}.{min}")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variant::parse_variant;

    fn opts() -> MapOptions {
        MapOptions {
            package_name: "flash-attn".into(),
            version: "2.7.0".into(),
            build_number: 0,
            cuda_capabilities: vec![],
            require_cxx11: true,
        }
    }

    #[test]
    fn cuda_sets_virtual_package_constraints() {
        let v = parse_variant("torch26-cxx11-cu118-x86_64-linux").unwrap();
        let rec = map_variant(&v, &opts()).unwrap();
        assert_eq!(rec.subdir, "linux-64");
        assert!(rec.depends.contains(&"pytorch 2.6.*".to_string()));
        assert_eq!(rec.torch_pin, "pytorch 2.6.*");
        assert_eq!(rec.cuda_floor.as_deref(), Some("11.8"));
        assert!(rec.constrains.contains(&"__cuda >=11.8".to_string()));
        assert_eq!(rec.build, "torch26_cxx11_cuda118");
    }

    #[test]
    fn cuda_arch_floor_from_capabilities() {
        let v = parse_variant("torch26-cxx11-cu124-x86_64-linux").unwrap();
        let mut o = opts();
        o.cuda_capabilities = vec!["9.0".into(), "8.0".into(), "8.6".into()];
        let rec = map_variant(&v, &o).unwrap();
        assert!(rec.constrains.contains(&"__cuda_arch >=8.0".to_string()));
    }

    #[test]
    fn cxx98_rejected() {
        let v = parse_variant("torch26-cxx98-cu118-x86_64-linux").unwrap();
        assert_eq!(
            map_variant(&v, &opts()),
            Err(UnsupportedVariant::Cxx98("torch26-cxx98-cu118-x86_64-linux".into()))
        );
    }
}
