//! JSON-RPC entry point for the HF-kernel backend.
//!
//! We implement the pixi build [`Protocol`] directly rather than going through
//! `IntermediateBackend` (recipe + rattler-build variant expansion). That path
//! can't express what we need: its only generator-side source of variant values
//! is `default_variants`, which is config-blind and runs before `generate_recipe`
//! — so it can't emit a *backend-fetched, per-repo* set of variants. Implementing
//! `conda_outputs` directly lets us list the Hub and hand the solver ONE
//! `CondaOutput` per variant, each carrying its own `pytorch` / `__cuda` /
//! `__cuda_arch` matchspecs. The solver picks the satisfiable one; pixi locks it.
//!
//! Mode A = repackage: `conda_build_v1` (the actual build) downloads the chosen
//! `build/<variant>` subtree and lays it into site-packages.

mod build;
mod config;
mod hub;
mod mapping;
mod outputs;
mod variant;

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use miette::{miette, IntoDiagnostic, Result};

use pixi_build_backend::protocol::{Protocol, ProtocolInstantiator};
use rattler_conda_types::{
    MatchSpec, NamelessMatchSpec, NoArchType, PackageName, ParseStrictness, Platform,
    VersionWithSource,
};

use pixi_build_types::procedures::conda_build_v1::{
    CondaBuildV1Dependency, CondaBuildV1Params, CondaBuildV1Result,
};
use pixi_build_types::procedures::conda_outputs::{
    CondaOutput, CondaOutputDependencies, CondaOutputIgnoreRunExports, CondaOutputMetadata,
    CondaOutputRunExports, CondaOutputsParams, CondaOutputsResult,
};
use pixi_build_types::procedures::initialize::{InitializeParams, InitializeResult};
use pixi_build_types::procedures::negotiate_capabilities::{
    NegotiateCapabilitiesParams, NegotiateCapabilitiesResult,
};
use pixi_build_types::{
    BackendCapabilities, BinaryPackageSpec, ConstraintSpec, NamedSpec, PackageSpec, ProjectModel,
    SourcePackageName, VariantValue,
};

use crate::mapping::{cuda_arch_floor, CondaRecord, MapOptions};
use crate::outputs::build_all_records;

use config::HfKernelConfig;

// ---------------------------------------------------------------------------
// Instantiator: negotiate capabilities + build the per-connection backend.
// ---------------------------------------------------------------------------

struct HfKernelInstantiator;

#[async_trait::async_trait]
impl ProtocolInstantiator for HfKernelInstantiator {
    async fn negotiate_capabilities(
        _params: NegotiateCapabilitiesParams,
    ) -> Result<NegotiateCapabilitiesResult> {
        Ok(NegotiateCapabilitiesResult {
            capabilities: BackendCapabilities {
                provides_conda_outputs: Some(true),
                provides_conda_build_v1: Some(true),
            },
        })
    }

    async fn initialize(
        &self,
        params: InitializeParams,
    ) -> Result<(Box<dyn Protocol + Send + Sync + 'static>, InitializeResult)> {
        let project_model = params
            .project_model
            .ok_or_else(|| miette!("project model is required"))?;

        let config: HfKernelConfig = match params.configuration {
            Some(v) => serde_json::from_value(v).into_diagnostic()?,
            None => return Err(miette!("[package.build.config] with `repo` is required")),
        };

        let backend = HfKernelBackend {
            config,
            project_model,
            client: reqwest::Client::new(),
        };
        Ok((Box::new(backend), InitializeResult {}))
    }
}

// ---------------------------------------------------------------------------
// Backend: emit one CondaOutput per compatible HF variant.
// ---------------------------------------------------------------------------

struct HfKernelBackend {
    config: HfKernelConfig,
    project_model: ProjectModel,
    client: reqwest::Client,
}

#[async_trait::async_trait]
impl Protocol for HfKernelBackend {
    async fn conda_outputs(&self, params: CondaOutputsParams) -> Result<CondaOutputsResult> {
        let host = params.host_platform;
        let cfg = &self.config;

        let name = cfg
            .package_name
            .clone()
            .or_else(|| self.project_model.name.clone())
            .ok_or_else(|| miette!("package name unknown; set `package-name` in config"))?;
        let version = self
            .project_model
            .version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0.0.0".to_string());

        // Resolve the variant list + capabilities, from config overrides or Hub.
        let rev = cfg.rev.clone().unwrap_or_else(|| "main".to_string());
        let (variant_names, caps, resolved_rev) = self.resolve_inputs(&rev).await?;

        let opts = MapOptions {
            package_name: name.clone(),
            version: version.clone(),
            build_number: 0,
            cuda_capabilities: caps.clone(),
            require_cxx11: cfg.require_cxx11,
        };
        let (records, skipped) = build_all_records(&variant_names, &opts);
        for s in &skipped {
            eprintln!("skip {}: {}", s.variant, s.reason);
        }

        // conda_outputs is per host platform: keep only this subdir's variants.
        let host_str = host.to_string();
        let records: Vec<CondaRecord> =
            records.into_iter().filter(|r| r.subdir == host_str).collect();

        let arch_floor = cuda_arch_floor(&caps);
        let mut outputs = Vec::with_capacity(records.len());
        for r in &records {
            outputs.push(conda_output(
                r,
                &name,
                &version,
                host,
                arch_floor.as_deref(),
                &resolved_rev,
            )?);
        }
        eprintln!(
            "hf-kernel: {}@{} -> {} outputs for {host_str}",
            cfg.repo,
            resolved_rev,
            outputs.len()
        );

        Ok(CondaOutputsResult {
            outputs,
            input_globs: BTreeSet::new(),
        })
    }

    async fn conda_build_v1(&self, params: CondaBuildV1Params) -> Result<CondaBuildV1Result> {
        let out = &params.output;
        let variant = variant_string(&out.variant, "hf_kernel_variant")
            .ok_or_else(|| miette!("output is missing the hf_kernel_variant marker"))?;
        let sha = variant_string(&out.variant, "hf_kernel_sha")
            .ok_or_else(|| miette!("output is missing the hf_kernel_sha marker"))?;

        let name = out.name.as_normalized().to_string();
        let version = out
            .version
            .as_ref()
            .map(|v| v.to_string())
            .unwrap_or_else(|| "0.0.0".to_string());
        let build = out.build.clone().unwrap_or_default();

        let depends = matchspec_strings(&params.run_dependencies);
        let constrains = matchspec_strings(&params.run_constraints);

        let out_dir = params
            .output_directory
            .clone()
            .unwrap_or_else(|| params.work_directory.join("output"));

        let req = build::BuildRequest {
            repo: &self.config.repo,
            sha: &sha,
            variant: &variant,
            name: &name,
            version: &version,
            build: &build,
            build_number: 0,
            subdir: out.subdir,
            depends,
            constrains,
            work_dir: &params.work_directory,
            out_dir: &out_dir,
        };

        let output_file = build::build_package(&self.client, &req).await?;
        eprintln!("hf-kernel: built {}", output_file.display());

        Ok(CondaBuildV1Result {
            output_file,
            input_globs: BTreeSet::new(),
            name,
            version: VersionWithSource::from_str(&version).into_diagnostic()?,
            build,
            subdir: out.subdir,
        })
    }
}

fn variant_string(variant: &BTreeMap<String, VariantValue>, key: &str) -> Option<String> {
    match variant.get(key) {
        Some(VariantValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn matchspec_strings(deps: &Option<Vec<CondaBuildV1Dependency>>) -> Vec<String> {
    deps.as_deref()
        .unwrap_or_default()
        .iter()
        .map(|d| d.spec.to_string())
        .collect()
}

impl HfKernelBackend {
    /// Returns (variant_names, cuda_capabilities, resolved_rev). Uses config
    /// overrides when present (offline/deterministic), else queries the Hub.
    async fn resolve_inputs(&self, rev: &str) -> Result<(Vec<String>, Vec<String>, String)> {
        let cfg = &self.config;
        if let Some(variants) = &cfg.variants {
            let caps = cfg.cuda_capabilities.clone().unwrap_or_default();
            return Ok((variants.clone(), caps, rev.to_string()));
        }
        // Pin to a concrete commit for reproducibility, then list against it.
        let sha = hub::resolve_sha(&self.client, &cfg.repo, rev).await?;
        let variants = hub::list_variants(&self.client, &cfg.repo, &sha).await?;
        let caps = match &cfg.cuda_capabilities {
            Some(c) => c.clone(),
            None => hub::read_cuda_capabilities(&self.client, &cfg.repo, &sha).await?,
        };
        Ok((variants, caps, sha))
    }
}

// ---------------------------------------------------------------------------
// CondaRecord -> CondaOutput
// ---------------------------------------------------------------------------

fn conda_output(
    r: &CondaRecord,
    name: &str,
    version: &str,
    host: Platform,
    arch_floor: Option<&str>,
    sha: &str,
) -> Result<CondaOutput> {
    let depends = r
        .depends
        .iter()
        .map(|s| named_package(s))
        .collect::<Result<Vec<_>>>()?;

    let mut constraints = r
        .constrains
        .iter()
        .map(|s| named_constraint(s))
        .collect::<Result<Vec<_>>>()?;
    if let Some(floor) = arch_floor {
        constraints.push(named_constraint(&format!("__cuda_arch >={floor}"))?);
    }

    let metadata = CondaOutputMetadata {
        name: PackageName::from_str(name).into_diagnostic()?,
        version: VersionWithSource::from_str(version).into_diagnostic()?,
        build: r.build.clone(),
        build_number: r.build_number,
        subdir: host,
        license: None,
        license_family: None,
        noarch: NoArchType::default(),
        purls: None,
        python_site_packages_path: None,
        // Carry the exact Hub variant + pinned commit so conda_build_v1 knows
        // precisely which prebuilt tree to fetch for this output.
        variant: BTreeMap::from([
            (
                "hf_kernel_variant".to_string(),
                VariantValue::String(r.variant.clone()),
            ),
            ("hf_kernel_sha".to_string(), VariantValue::String(sha.to_string())),
        ]),
    };

    Ok(CondaOutput {
        metadata,
        build_dependencies: None,
        host_dependencies: None,
        run_dependencies: CondaOutputDependencies {
            depends,
            constraints,
        },
        ignore_run_exports: CondaOutputIgnoreRunExports::default(),
        run_exports: CondaOutputRunExports::default(),
        input_globs: None,
    })
}

/// Parse a matchspec string into a run-dependency `NamedSpec<PackageSpec>`.
fn named_package(spec: &str) -> Result<NamedSpec<PackageSpec>> {
    let (name, nameless) = split_matchspec(spec)?;
    Ok(NamedSpec {
        name,
        spec: PackageSpec::Binary(to_binary(nameless)),
    })
}

/// Parse a matchspec string into a `NamedSpec<ConstraintSpec>` (e.g. `__cuda >=11.8`).
fn named_constraint(spec: &str) -> Result<NamedSpec<ConstraintSpec>> {
    let (name, nameless) = split_matchspec(spec)?;
    Ok(NamedSpec {
        name,
        spec: ConstraintSpec::Binary(to_binary(nameless)),
    })
}

/// Split `name >=1.0` into (source name, nameless spec) via a strict parse.
fn split_matchspec(spec: &str) -> Result<(SourcePackageName, NamelessMatchSpec)> {
    let ms = MatchSpec::from_str(spec, ParseStrictness::Lenient).into_diagnostic()?;
    let (Some(name_matcher), nameless) = ms.into_nameless() else {
        return Err(miette!("matchspec {spec:?} has no package name"));
    };
    let name = name_matcher
        .as_exact()
        .ok_or_else(|| miette!("matchspec {spec:?} needs an exact name"))?;
    Ok((name.as_source().to_owned(), nameless))
}

/// Mirror of pixi-build-backend's private `convert_nameless_matchspec`.
fn to_binary(spec: NamelessMatchSpec) -> BinaryPackageSpec {
    BinaryPackageSpec {
        version: spec.version,
        build: spec.build,
        build_number: spec.build_number,
        file_name: spec.file_name,
        channel: spec.channel.map(|c| c.base_url.clone().into()),
        subdir: spec.subdir,
        md5: spec.md5,
        sha256: spec.sha256,
        url: spec.url,
        license: spec.license,
    }
}

#[tokio::main]
async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|_log| HfKernelInstantiator).await {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> HfKernelBackend {
        let config = HfKernelConfig {
            repo: "kernels-community/flash-attn".into(),
            rev: Some("main".into()),
            package_name: Some("flash-attn".into()),
            require_cxx11: true,
            variants: Some(vec![
                "torch26-cxx11-cu118-x86_64-linux".into(),  // ok
                "torch27-cxx11-cu126-x86_64-linux".into(),  // ok
                "torch26-cxx98-cu118-x86_64-linux".into(),  // dropped: cxx98
                "torch27-cxx11-cu126-aarch64-linux".into(), // dropped: not linux-64
            ]),
            cuda_capabilities: Some(vec!["9.0".into(), "8.0".into()]),
            debug_dir: None,
        };
        let project_model =
            serde_json::from_value(serde_json::json!({ "name": "flash-attn" })).unwrap();
        HfKernelBackend {
            config,
            project_model,
            client: reqwest::Client::new(),
        }
    }

    fn params() -> CondaOutputsParams {
        CondaOutputsParams {
            channels: vec![],
            host_platform: Platform::Linux64,
            build_platform: Platform::Linux64,
            variant_configuration: None,
            variant_files: None,
            work_directory: std::path::PathBuf::from("/tmp/hf-kernel-test"),
        }
    }

    /// Live network check against a real kernel repo. Run with:
    /// `cargo test -- --ignored live_hub`
    #[tokio::test]
    #[ignore]
    async fn live_hub_listing_and_outputs() {
        let client = reqwest::Client::new();
        let repo = "kernels-community/activation";
        let sha = hub::resolve_sha(&client, repo, "main").await.unwrap();
        assert_eq!(sha.len(), 40, "expected a 40-char commit sha, got {sha:?}");
        let variants = hub::list_variants(&client, repo, &sha).await.unwrap();
        assert!(variants.len() > 10, "expected many variants, got {}", variants.len());

        let opts = MapOptions {
            package_name: "activation".into(),
            version: "0.0.1".into(),
            build_number: 0,
            cuda_capabilities: hub::read_cuda_capabilities(&client, repo, &sha).await.unwrap(),
            require_cxx11: true,
        };
        let (records, _skipped) = build_all_records(&variants, &opts);
        let linux64 = records.iter().filter(|r| r.subdir == "linux-64").count();
        eprintln!("{repo}@{sha}: {} records, {linux64} for linux-64", records.len());
        assert!(linux64 > 0, "expected some linux-64 records");
    }

    /// Live build: fetch a real variant and pack a .conda. Run with:
    /// `cargo test -- --ignored live_build`
    #[tokio::test]
    #[ignore]
    async fn live_build_produces_valid_conda() {
        use pixi_build_types::procedures::conda_build_v1::{CondaBuildV1Output, CondaBuildV1Params};

        let client = reqwest::Client::new();
        let repo = "kernels-community/activation";
        let sha = hub::resolve_sha(&client, repo, "main").await.unwrap();
        let variant = hub::list_variants(&client, repo, &sha)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.ends_with("-x86_64-linux") && v.contains("cxx11") && v.contains("cu"))
            .expect("a linux-64 cxx11 cuda variant");
        eprintln!("building {variant} @ {sha}");

        let work = std::env::temp_dir().join("hf-kernel-build-test");
        let _ = std::fs::remove_dir_all(&work);
        let out_dir = work.join("out");

        let output = CondaBuildV1Output {
            name: PackageName::from_str("activation").unwrap(),
            version: Some(VersionWithSource::from_str("0.0.1").unwrap()),
            build: Some("torch_test".to_string()),
            subdir: Platform::Linux64,
            variant: BTreeMap::from([
                ("hf_kernel_variant".to_string(), VariantValue::String(variant.clone())),
                ("hf_kernel_sha".to_string(), VariantValue::String(sha.clone())),
            ]),
        };
        let params = CondaBuildV1Params {
            channels: vec![],
            build_prefix: None,
            host_prefix: None,
            run_dependencies: None,
            run_constraints: None,
            run_exports: None,
            output,
            work_directory: work.clone(),
            output_directory: Some(out_dir.clone()),
            editable: None,
        };

        let cfg = HfKernelConfig { repo: repo.into(), ..Default::default() };
        let backend = HfKernelBackend {
            config: cfg,
            project_model: serde_json::from_value(serde_json::json!({"name":"activation"})).unwrap(),
            client: client.clone(),
        };

        let res = backend.conda_build_v1(params).await.unwrap();

        assert!(res.output_file.exists(), "no .conda written");
        assert_eq!(res.output_file, out_dir.join("activation-0.0.1-torch_test.conda"));
        let bytes = std::fs::read(&res.output_file).unwrap();
        assert!(bytes.starts_with(b"PK"), ".conda is not a zip");
        let hay = String::from_utf8_lossy(&bytes);
        assert!(hay.contains("metadata.json"), "missing outer metadata.json");
        assert!(hay.contains("info-activation-0.0.1-torch_test.tar.zst"), "missing info archive");
        assert!(hay.contains("pkg-activation-0.0.1-torch_test.tar.zst"), "missing pkg archive");
        eprintln!("wrote {} ({} bytes)", res.output_file.display(), bytes.len());
    }

    #[tokio::test]
    async fn emits_one_output_per_compatible_variant_with_solver_constraints() {
        let res = backend().conda_outputs(params()).await.unwrap();

        // cxx98 and the aarch64 variant are filtered out for linux-64.
        let builds: BTreeSet<_> = res.outputs.iter().map(|o| o.metadata.build.clone()).collect();
        assert_eq!(
            builds,
            BTreeSet::from([
                "torch26_cxx11_cuda118".to_string(),
                "torch27_cxx11_cuda126".to_string(),
            ])
        );

        let o = res
            .outputs
            .iter()
            .find(|o| o.metadata.build == "torch26_cxx11_cuda118")
            .unwrap();
        assert_eq!(o.metadata.subdir, Platform::Linux64);
        assert_eq!(o.metadata.name.as_normalized(), "flash-attn");

        // Run dependency: pytorch pinned to the variant's torch minor.
        let dep_names: BTreeSet<_> = o.run_dependencies.depends.iter().map(|d| d.name.clone()).collect();
        assert!(dep_names.contains("pytorch"));

        // The solver-selection constraints: __cuda floor, cuda-version, __cuda_arch floor.
        let con_names: BTreeSet<_> =
            o.run_dependencies.constraints.iter().map(|c| c.name.clone()).collect();
        assert!(con_names.contains("__cuda"), "constraints: {con_names:?}");
        assert!(con_names.contains("cuda-version"));
        assert!(con_names.contains("__cuda_arch"));
    }
}
