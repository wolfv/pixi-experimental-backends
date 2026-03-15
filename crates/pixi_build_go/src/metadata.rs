use std::path::PathBuf;

use miette::Diagnostic;
use pixi_build_backend::generated_recipe::MetadataProvider;
use rattler_conda_types::Version;

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum MetadataError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to parse go.mod: {0}")]
    Parse(String),
    #[error("failed to parse version: {0}")]
    ParseVersion(#[from] rattler_conda_types::ParseVersionError),
}

/// Parsed metadata from a go.mod file.
struct GoModMetadata {
    /// The module path (e.g., "github.com/user/project")
    module: String,
}

/// An implementation of [`MetadataProvider`] that reads metadata from a
/// go.mod file.
pub struct GoMetadataProvider {
    manifest_root: PathBuf,
    metadata: Option<GoModMetadata>,
    ignore_go_mod: bool,
}

impl GoMetadataProvider {
    pub fn new(manifest_root: impl Into<PathBuf>, ignore_go_mod: bool) -> Self {
        Self {
            manifest_root: manifest_root.into(),
            metadata: None,
            ignore_go_mod,
        }
    }

    fn ensure_metadata(&mut self) -> Result<Option<&GoModMetadata>, MetadataError> {
        if self.ignore_go_mod {
            return Ok(None);
        }
        if self.metadata.is_none() {
            let go_mod_path = self.manifest_root.join("go.mod");
            if !go_mod_path.exists() {
                return Ok(None);
            }
            let content = std::fs::read_to_string(&go_mod_path)?;
            self.metadata = Some(parse_go_mod(&content)?);
        }
        Ok(self.metadata.as_ref())
    }

    /// Returns input globs for files that affect metadata.
    pub fn input_globs(&self) -> Vec<String> {
        if self.ignore_go_mod {
            return Vec::new();
        }
        vec!["go.mod".to_string()]
    }
}

/// Extract the short name from a Go module path.
/// e.g., "github.com/user/project" -> "project"
fn module_short_name(module_path: &str) -> &str {
    module_path.rsplit('/').next().unwrap_or(module_path)
}

fn parse_go_mod(content: &str) -> Result<GoModMetadata, MetadataError> {
    let mut module = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("module") {
            let rest = rest.trim();
            if !rest.is_empty() {
                module = Some(rest.to_string());
            }
        }
    }

    Ok(GoModMetadata {
        module: module
            .ok_or_else(|| MetadataError::Parse("no module directive found".to_string()))?,
    })
}

impl MetadataProvider for GoMetadataProvider {
    type Error = MetadataError;

    fn name(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self
            .ensure_metadata()?
            .map(|m| module_short_name(&m.module).to_string()))
    }

    fn version(&mut self) -> Result<Option<Version>, Self::Error> {
        // go.mod doesn't have a version field for the module itself
        Ok(None)
    }

    fn repository(&mut self) -> Result<Option<String>, Self::Error> {
        Ok(self.ensure_metadata()?.map(|m| {
            // If the module path looks like a URL host/path, convert to https URL
            if m.module.contains('.') && m.module.contains('/') {
                format!("https://{}", m.module)
            } else {
                m.module.clone()
            }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn create_temp_go_project(go_mod_content: &str) -> TempDir {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let go_mod_path = temp_dir.path().join("go.mod");
        fs::write(go_mod_path, go_mod_content).expect("Failed to write go.mod");
        temp_dir
    }

    #[test]
    fn test_parse_go_mod_basic() {
        let content = "module github.com/user/myproject\n\ngo 1.21\n";
        let metadata = parse_go_mod(content).unwrap();
        assert_eq!(metadata.module, "github.com/user/myproject");
    }

    #[test]
    fn test_module_short_name() {
        assert_eq!(module_short_name("github.com/user/project"), "project");
        assert_eq!(module_short_name("simple"), "simple");
        assert_eq!(module_short_name("example.com/deep/nested/pkg"), "pkg");
    }

    #[test]
    fn test_go_metadata_provider_name() {
        let go_mod = "module github.com/user/my-tool\n\ngo 1.22\n";
        let temp_dir = create_temp_go_project(go_mod);
        let mut provider = GoMetadataProvider::new(temp_dir.path(), false);
        assert_eq!(provider.name().unwrap(), Some("my-tool".to_string()));
    }

    #[test]
    fn test_go_metadata_provider_repository() {
        let go_mod = "module github.com/user/my-tool\n\ngo 1.22\n";
        let temp_dir = create_temp_go_project(go_mod);
        let mut provider = GoMetadataProvider::new(temp_dir.path(), false);
        assert_eq!(
            provider.repository().unwrap(),
            Some("https://github.com/user/my-tool".to_string())
        );
    }

    #[test]
    fn test_go_metadata_provider_ignore() {
        let go_mod = "module github.com/user/my-tool\n\ngo 1.22\n";
        let temp_dir = create_temp_go_project(go_mod);
        let mut provider = GoMetadataProvider::new(temp_dir.path(), true);
        assert_eq!(provider.name().unwrap(), None);
        assert_eq!(provider.repository().unwrap(), None);
    }

    #[test]
    fn test_go_metadata_provider_missing_go_mod() {
        let temp_dir = TempDir::new().unwrap();
        let mut provider = GoMetadataProvider::new(temp_dir.path(), false);
        assert_eq!(provider.name().unwrap(), None);
    }

    #[test]
    fn test_input_globs() {
        let provider = GoMetadataProvider::new(Path::new("."), false);
        assert_eq!(provider.input_globs(), vec!["go.mod".to_string()]);
    }

    #[test]
    fn test_input_globs_ignored() {
        let provider = GoMetadataProvider::new(Path::new("."), true);
        assert!(provider.input_globs().is_empty());
    }
}
