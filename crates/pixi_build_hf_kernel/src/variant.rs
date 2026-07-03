//! Parsing of Hugging Face kernel *build variants*.
//!
//! A kernel repository on the Hub contains a `build/` directory whose
//! sub-directories are named following the template:
//!
//! ```text
//! <framework><version>-cxx<abiver>-<computebackend>-<arch>-<os>
//! ```
//!
//! for example `torch26-cxx98-cu118-x86_64-linux` or
//! `torch27-cxx11-cu126-aarch64-linux`.
//!
//! Note what the directory name does *not* encode: the CUDA *compute
//! capabilities* the fatbin was compiled for. Those are declared once per
//! kernel in `build.toml` (`cuda-capabilities = ["8.0", "9.0"]`) and are
//! handled separately in [`crate::mapping`].

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputeKind {
    Cuda,
    Rocm,
    Cpu,
    Metal,
}

impl ComputeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ComputeKind::Cuda => "cuda",
            ComputeKind::Rocm => "rocm",
            ComputeKind::Cpu => "cpu",
            ComputeKind::Metal => "metal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variant {
    pub raw: String,
    pub framework: String,         // "torch"
    pub framework_version: String, // "2.6"
    pub cxx_abi: String,           // "cxx98" | "cxx11"
    pub compute_kind: ComputeKind,
    pub compute_version: Option<String>, // "11.8" for cuda/rocm, None for cpu/metal
    pub arch: String,                     // "x86_64" | "aarch64"
    pub os: String,                       // "linux"
}

impl Variant {
    /// The old pre-C++11 ABI — incompatible with conda-forge's cxx11 stack.
    pub fn is_cxx98(&self) -> bool {
        self.cxx_abi == "cxx98"
    }

    /// Whether the directory name carried a `cxx*` ABI tag at all.
    pub fn has_abi_tag(&self) -> bool {
        self.cxx_abi != ABI_NONE
    }
}

/// C++ ABI tag when a variant omits one (Windows / metal have no libstdc++
/// ABI split, so their directory names drop the `cxx*` field).
pub const ABI_NONE: &str = "none";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    #[error("variant {0:?}: expected 4 or 5 dash-separated fields, got {1}")]
    FieldCount(String, usize),
    #[error("variant {0:?}: cannot parse framework {1:?}")]
    Framework(String, String),
    #[error("variant {0:?}: unknown C++ ABI tag {1:?}")]
    Abi(String, String),
    #[error("variant {0:?}: cannot parse compute backend {1:?}")]
    Compute(String, String),
}

/// Parse a build-variant directory name into a [`Variant`].
///
/// Accepts the 5-field linux form (`torch26-cxx11-cu118-x86_64-linux`) and the
/// 4-field form used by Windows/metal, which omits the `cxx*` ABI tag
/// (`torch210-cu128-x86_64-windows`, `torch210-metal-aarch64-darwin`). Anything
/// else errors so the backend skips (and logs) it rather than emit a broken
/// record.
pub fn parse_variant(name: &str) -> Result<Variant, ParseError> {
    let parts: Vec<&str> = name.split('-').collect();
    let (fw_raw, abi_raw, compute_raw, arch, os) = match parts.len() {
        5 => (parts[0], Some(parts[1]), parts[2], parts[3], parts[4]),
        4 => (parts[0], None, parts[1], parts[2], parts[3]),
        n => return Err(ParseError::FieldCount(name.to_string(), n)),
    };

    let (framework, framework_version) = parse_framework(fw_raw)
        .ok_or_else(|| ParseError::Framework(name.to_string(), fw_raw.to_string()))?;

    let cxx_abi = match abi_raw {
        None => ABI_NONE,
        Some(a @ ("cxx98" | "cxx11")) => a,
        Some(other) => return Err(ParseError::Abi(name.to_string(), other.to_string())),
    };

    let (compute_kind, compute_version) = parse_compute(compute_raw)
        .ok_or_else(|| ParseError::Compute(name.to_string(), compute_raw.to_string()))?;

    Ok(Variant {
        raw: name.to_string(),
        framework,
        framework_version,
        cxx_abi: cxx_abi.to_string(),
        compute_kind,
        compute_version,
        arch: arch.to_string(),
        os: os.to_string(),
    })
}

/// `torch26` -> ("torch", "2.6"). Trailing digits after the first are the minor.
fn parse_framework(token: &str) -> Option<(String, String)> {
    let split = token.find(|c: char| c.is_ascii_digit())?;
    let (name, digits) = token.split_at(split);
    if name.is_empty() || digits.len() < 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (major, minor) = digits.split_at(1);
    Some((name.to_string(), format!("{major}.{minor}")))
}

/// `cu118` -> (Cuda, "11.8"); `rocm62` -> (Rocm, "6.2"); `cpu`/`metal` -> no version.
fn parse_compute(token: &str) -> Option<(ComputeKind, Option<String>)> {
    match token {
        "cpu" => return Some((ComputeKind::Cpu, None)),
        "metal" => return Some((ComputeKind::Metal, None)),
        _ => {}
    }
    if let Some(rest) = token.strip_prefix("cu") {
        return split_version(rest).map(|v| (ComputeKind::Cuda, Some(v)));
    }
    if let Some(rest) = token.strip_prefix("rocm") {
        return split_version(rest).map(|v| (ComputeKind::Rocm, Some(v)));
    }
    None
}

/// `118` -> "11.8", `126` -> "12.6" (last digit is the minor).
fn split_version(digits: &str) -> Option<String> {
    if digits.len() < 2 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let (major, minor) = digits.split_at(digits.len() - 1);
    Some(format!("{}.{}", major.parse::<u32>().ok()?, minor))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_cuda_variant() {
        let v = parse_variant("torch26-cxx98-cu118-x86_64-linux").unwrap();
        assert_eq!(v.framework, "torch");
        assert_eq!(v.framework_version, "2.6");
        assert_eq!(v.cxx_abi, "cxx98");
        assert_eq!(v.compute_kind, ComputeKind::Cuda);
        assert_eq!(v.compute_version.as_deref(), Some("11.8"));
        assert_eq!(v.arch, "x86_64");
        assert_eq!(v.os, "linux");
    }

    #[test]
    fn parses_cuda12_aarch64() {
        let v = parse_variant("torch27-cxx11-cu126-aarch64-linux").unwrap();
        assert_eq!(v.compute_version.as_deref(), Some("12.6"));
        assert_eq!(v.cxx_abi, "cxx11");
    }

    #[test]
    fn parses_rocm() {
        let v = parse_variant("torch26-cxx11-rocm62-x86_64-linux").unwrap();
        assert_eq!(v.compute_kind, ComputeKind::Rocm);
        assert_eq!(v.compute_version.as_deref(), Some("6.2"));
    }

    #[test]
    fn parses_windows_variant_without_abi_tag() {
        let v = parse_variant("torch210-cu128-x86_64-windows").unwrap();
        assert_eq!(v.framework_version, "2.10"); // torch 2.10
        assert_eq!(v.cxx_abi, ABI_NONE);
        assert!(!v.has_abi_tag());
        assert_eq!(v.compute_kind, ComputeKind::Cuda);
        assert_eq!(v.compute_version.as_deref(), Some("12.8"));
        assert_eq!(v.os, "windows");
    }

    #[test]
    fn parses_metal_variant() {
        let v = parse_variant("torch210-metal-aarch64-darwin").unwrap();
        assert_eq!(v.compute_kind, ComputeKind::Metal);
        assert_eq!(v.cxx_abi, ABI_NONE);
        assert_eq!(v.os, "darwin");
    }

    #[test]
    fn rejects_malformed() {
        // wrong field count, bad ABI tag, unparseable compute backend
        for bad in [
            "junk",
            "a-b-c-d-e-f",
            "torch26-cxxBAD-cu118-x86_64-linux",
            "torch26-cxx11-nope-x86_64-linux",
        ] {
            assert!(parse_variant(bad).is_err(), "should reject {bad:?}");
        }
    }
}
