//! Minimal async Hugging Face Hub client: resolve a ref to a commit, list
//! `build/` variants, read `cuda-capabilities`, and download variant files.

use std::path::Path;

use miette::{IntoDiagnostic, Result, miette};
use reqwest::Client;
use serde_json::Value as Json;

const BASE: &str = "https://huggingface.co";

/// Resolve a branch/tag/sha to a concrete commit sha (reproducibility anchor).
pub async fn resolve_sha(client: &Client, repo: &str, rev: &str) -> Result<String> {
    let url = format!("{BASE}/api/models/{repo}/revision/{rev}");
    let json: Json = get(client, &url).await?.json().await.into_diagnostic()?;
    json.get("sha")
        .and_then(Json::as_str)
        .map(str::to_string)
        .ok_or_else(|| miette!("no sha in revision response for {repo}@{rev}"))
}

/// List the sub-directory names under `build/` at a revision.
pub async fn list_variants(client: &Client, repo: &str, rev: &str) -> Result<Vec<String>> {
    let entries = tree(client, repo, rev).await?;
    let mut names = std::collections::BTreeSet::new();
    for e in &entries {
        if let Some(path) = e.get("path").and_then(Json::as_str) {
            let mut parts = path.split('/');
            if parts.next() == Some("build") {
                if let Some(dir) = parts.next() {
                    names.insert(dir.to_string());
                }
            }
        }
    }
    Ok(names.into_iter().collect())
}

/// List files under `build/<variant>/` at a revision, returning
/// `(hub_path, relative_path)` where `relative_path` drops the
/// `build/<variant>/` prefix (i.e. the path inside site-packages).
pub async fn list_variant_files(
    client: &Client,
    repo: &str,
    rev: &str,
    variant: &str,
) -> Result<Vec<(String, String)>> {
    let entries = tree(client, repo, rev).await?;
    let prefix = format!("build/{variant}/");
    let mut files = Vec::new();
    for e in &entries {
        if e.get("type").and_then(Json::as_str) != Some("file") {
            continue;
        }
        if let Some(path) = e.get("path").and_then(Json::as_str) {
            if let Some(rel) = path.strip_prefix(&prefix) {
                files.push((path.to_string(), rel.to_string()));
            }
        }
    }
    Ok(files)
}

/// Download a single repo file (by its full path) into `dest`, creating parents.
pub async fn download_file(
    client: &Client,
    repo: &str,
    rev: &str,
    hub_path: &str,
    dest: &Path,
) -> Result<()> {
    let url = format!("{BASE}/{repo}/resolve/{rev}/{hub_path}");
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).into_diagnostic()?;
    }
    let bytes = get(client, &url).await?.bytes().await.into_diagnostic()?;
    std::fs::write(dest, &bytes).into_diagnostic()
}

/// Read `[kernel.*].cuda-capabilities` (union) from build.toml; `[]` if absent.
pub async fn read_cuda_capabilities(client: &Client, repo: &str, rev: &str) -> Result<Vec<String>> {
    let url = format!("{BASE}/{repo}/resolve/{rev}/build.toml");
    let resp = client.get(&url).send().await.into_diagnostic()?;
    if !resp.status().is_success() {
        return Ok(vec![]); // no build.toml -> "all capabilities"
    }
    let text = resp.text().await.into_diagnostic()?;
    let doc: toml::Value = toml::from_str(&text).into_diagnostic()?;

    let mut caps = std::collections::BTreeSet::new();
    if let Some(kernels) = doc.get("kernel").and_then(toml::Value::as_table) {
        for kernel in kernels.values() {
            if let Some(list) = kernel
                .get("cuda-capabilities")
                .and_then(toml::Value::as_array)
            {
                for c in list {
                    if let Some(s) = c.as_str() {
                        caps.insert(s.to_string());
                    } else if let Some(f) = c.as_float() {
                        caps.insert(format!("{f}"));
                    }
                }
            }
        }
    }
    Ok(caps.into_iter().collect())
}

/// Fetch the recursive file tree at a revision as a JSON array.
async fn tree(client: &Client, repo: &str, rev: &str) -> Result<Vec<Json>> {
    let url = format!("{BASE}/api/models/{repo}/tree/{rev}?recursive=true");
    let json: Json = get(client, &url).await?.json().await.into_diagnostic()?;
    match json {
        Json::Array(entries) => Ok(entries),
        _ => Err(miette!("unexpected tree response for {repo}@{rev}")),
    }
}

/// GET a URL and error on non-success status.
async fn get(client: &Client, url: &str) -> Result<reqwest::Response> {
    let resp = client.get(url).send().await.into_diagnostic()?;
    resp.error_for_status().into_diagnostic()
}
