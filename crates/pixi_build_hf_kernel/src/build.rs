//! Mode A build: download a prebuilt HF variant and pack it into a `.conda`.
//!
//! No compilation. We snapshot `build/<variant>/` at the pinned commit into a
//! staging prefix under `site-packages/`, synthesize `info/index.json` +
//! `info/paths.json`, and write the `.conda` with rattler's package writer.
//!
//! Files are placed under `site-packages/` and the package declares
//! `python_site_packages_path` (CEP-17) so a Python-version-independent, but
//! architecture-specific, kernel relocates to the env's real site-packages.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use miette::{miette, IntoDiagnostic, Result};
use rattler_conda_types::compression_level::CompressionLevel;
use rattler_conda_types::package::{IndexJson, PathType, PathsEntry, PathsJson};
use rattler_conda_types::{NoArchType, PackageName, Platform, VersionWithSource};
use rattler_package_streaming::write::write_conda_package;

pub struct BuildRequest<'a> {
    pub repo: &'a str,
    pub sha: &'a str,
    pub variant: &'a str,
    pub name: &'a str,
    pub version: &'a str,
    pub build: &'a str,
    pub build_number: u64,
    pub subdir: Platform,
    pub depends: Vec<String>,
    pub constrains: Vec<String>,
    pub work_dir: &'a Path,
    pub out_dir: &'a Path,
}

/// Build the package and return the path to the written `.conda`.
pub async fn build_package(client: &reqwest::Client, req: &BuildRequest<'_>) -> Result<PathBuf> {
    // 1. Fresh staging prefix with a site-packages/ root.
    let staging = req.work_dir.join("prefix");
    if staging.exists() {
        fs::remove_dir_all(&staging).into_diagnostic()?;
    }
    let sp = staging.join("site-packages");
    fs::create_dir_all(&sp).into_diagnostic()?;

    // 2. Download build/<variant>/* into site-packages/.
    let files = crate::hub::list_variant_files(client, req.repo, req.sha, req.variant).await?;
    if files.is_empty() {
        return Err(miette!(
            "no files under build/{} in {}@{}",
            req.variant,
            req.repo,
            req.sha
        ));
    }
    for (hub_path, rel) in &files {
        crate::hub::download_file(client, req.repo, req.sha, hub_path, &sp.join(rel)).await?;
    }

    // 3. info/index.json
    let info = staging.join("info");
    fs::create_dir_all(&info).into_diagnostic()?;
    let index = IndexJson {
        arch: None,
        build: req.build.to_string(),
        build_number: req.build_number,
        constrains: req.constrains.clone(),
        depends: req.depends.clone(),
        experimental_extra_depends: Default::default(),
        features: None,
        license: None,
        license_family: None,
        name: PackageName::from_str(req.name).into_diagnostic()?,
        noarch: NoArchType::none(),
        platform: None,
        purls: None,
        // CEP-17: relocate site-packages/ to the env python's site-packages.
        python_site_packages_path: Some("site-packages".to_string()),
        subdir: Some(req.subdir.to_string()),
        timestamp: None,
        track_features: vec![],
        version: VersionWithSource::from_str(req.version).into_diagnostic()?,
    };
    write_json(&info.join("index.json"), &index)?;

    // 4. info/paths.json (sha256 + size per file).
    let mut entries = Vec::with_capacity(files.len());
    for (_hub, rel) in &files {
        let abs = sp.join(rel);
        let size = fs::metadata(&abs).into_diagnostic()?.len();
        let sha =
            rattler_digest::compute_file_digest::<rattler_digest::Sha256>(&abs).into_diagnostic()?;
        entries.push(PathsEntry {
            relative_path: PathBuf::from(format!("site-packages/{rel}")),
            no_link: false,
            path_type: PathType::HardLink,
            prefix_placeholder: None,
            sha256: Some(sha),
            size_in_bytes: Some(size),
        });
    }
    entries.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    write_json(
        &info.join("paths.json"),
        &PathsJson {
            paths: entries,
            paths_version: 1,
        },
    )?;

    // 5. Collect every staged file (relative to the prefix) and write the .conda.
    let mut all_paths = Vec::new();
    collect_files(&staging, &mut all_paths)?;

    fs::create_dir_all(req.out_dir).into_diagnostic()?;
    let out_name = format!("{}-{}-{}", req.name, req.version, req.build);
    let out_file = req.out_dir.join(format!("{out_name}.conda"));
    let file = fs::File::create(&out_file).into_diagnostic()?;
    write_conda_package(
        file,
        &staging,
        &all_paths,
        CompressionLevel::Default,
        None,
        &out_name,
        None,
        None,
    )
    .into_diagnostic()?;

    Ok(out_file)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let s = serde_json::to_string_pretty(value).into_diagnostic()?;
    fs::write(path, s).into_diagnostic()
}

/// Collect absolute file paths under `dir`. `write_conda_package` strips
/// `base_path` itself, so entries must be absolute (not pre-stripped).
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).into_diagnostic()? {
        let path = entry.into_diagnostic()?.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}
