use miette::Diagnostic;
use pixi_build_backend::generated_recipe::MetadataProvider;
use rattler_conda_types::Version;
use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum MetadataError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse package.json: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("failed to parse version from package.json: {0}")]
    ParseVersion(#[from] rattler_conda_types::ParseVersionError),
}

#[derive(Debug, Deserialize)]
struct PackageJson {
    name: Option<String>,
    version: Option<String>,
    repository: Option<serde_json::Value>,
}

/// An implementation of [`MetadataProvider`] that reads metadata from a
/// `package.json` file.
pub struct NodejsMetadataProvider {
    manifest_root: PathBuf,
    metadata: Option<PackageJson>,
}

impl NodejsMetadataProvider {
    pub fn new(manifest_root: impl Into<PathBuf>) -> Self {
        Self {
            manifest_root: manifest_root.into(),
            metadata: None,
        }
    }

    fn ensure_metadata(&mut self) -> Result<Option<&PackageJson>, MetadataError> {
        if self.metadata.is_none() {
            let pkg_json_path = self.manifest_root.join("package.json");
            if !pkg_json_path.exists() {
                return Ok(None);
            }
            let content = std::fs::read_to_string(&pkg_json_path)?;
            self.metadata = Some(serde_json::from_str(&content)?);
        }
        Ok(self.metadata.as_ref())
    }

    /// Returns the input globs that affect metadata.
    pub fn input_globs(&self) -> Vec<String> {
        vec!["package.json".to_string()]
    }
}

/// Strip npm scope prefix: "@scope/package" → "package".
fn strip_npm_scope(name: &str) -> String {
    if let Some(stripped) = name.strip_prefix('@') {
        if let Some(slash) = stripped.find('/') {
            return stripped[slash + 1..].to_string();
        }
    }
    name.to_string()
}

impl MetadataProvider for NodejsMetadataProvider {
    type Error = MetadataError;

    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self
            .ensure_metadata()?
            .and_then(|m| m.name.as_deref())
            .map(strip_npm_scope))
    }

    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        let version_str = self
            .ensure_metadata()?
            .and_then(|m| m.version.as_deref())
            .filter(|v| !v.is_empty());

        match version_str {
            Some(v) => Ok(Some(v.parse()?)),
            None => Ok(None),
        }
    }

    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self
            .ensure_metadata()?
            .and_then(|m| m.repository.as_ref())
            .and_then(|r| match r {
                serde_json::Value::String(s) => Some(normalize_repo_url(s)),
                serde_json::Value::Object(o) => o
                    .get("url")
                    .and_then(|u| u.as_str())
                    .map(normalize_repo_url),
                _ => None,
            }))
    }
}

/// Normalize a repository URL: strip git+ prefix and .git suffix.
fn normalize_repo_url(url: &str) -> String {
    url.trim_start_matches("git+")
        .trim_end_matches(".git")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_project(pkg_json: &str) -> TempDir {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("package.json"), pkg_json).unwrap();
        dir
    }

    #[test]
    fn test_name_plain() {
        let dir = make_project(r#"{"name": "my-app", "version": "1.0.0"}"#);
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(p.name().unwrap(), Some("my-app".to_string()));
    }

    #[test]
    fn test_name_strips_scope() {
        let dir = make_project(r#"{"name": "@my-org/my-app", "version": "1.0.0"}"#);
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(p.name().unwrap(), Some("my-app".to_string()));
    }

    #[test]
    fn test_version_parsed() {
        let dir = make_project(r#"{"name": "app", "version": "2.3.4"}"#);
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(
            p.version().unwrap().map(|v| v.to_string()),
            Some("2.3.4".to_string())
        );
    }

    #[test]
    fn test_repository_string() {
        let dir = make_project(
            r#"{"name": "app", "version": "1.0.0", "repository": "https://github.com/user/app"}"#,
        );
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(
            p.repository().unwrap(),
            Some("https://github.com/user/app".to_string())
        );
    }

    #[test]
    fn test_repository_object_with_git_prefix() {
        let dir = make_project(
            r#"{"name": "app", "version": "1.0.0", "repository": {"type": "git", "url": "git+https://github.com/user/app.git"}}"#,
        );
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(
            p.repository().unwrap(),
            Some("https://github.com/user/app".to_string())
        );
    }

    #[test]
    fn test_missing_package_json() {
        let dir = TempDir::new().unwrap();
        let mut p = NodejsMetadataProvider::new(dir.path());
        assert_eq!(p.name().unwrap(), None);
        assert_eq!(p.version().unwrap(), None);
    }

    #[test]
    fn test_input_globs() {
        let p = NodejsMetadataProvider::new(".");
        assert_eq!(p.input_globs(), vec!["package.json"]);
    }
}
