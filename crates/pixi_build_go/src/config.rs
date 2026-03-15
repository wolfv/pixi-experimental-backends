use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GoBackendConfig {
    /// Whether to use CGO (default: false, uses go-nocgo compiler).
    /// When true, uses go-cgo compiler and adds C compiler to build requirements.
    #[serde(default)]
    pub cgo_enabled: bool,

    /// Whether to collect licenses from Go dependencies using go-licenses.
    /// Default: false.
    #[serde(default)]
    pub collect_licenses: bool,

    /// Extra args to pass to `go install`
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Environment Variables
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// Deprecated. Setting this has no effect; debug data is always written to
    /// the `debug` subdirectory of the work directory.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,

    /// Extra input globs to include in addition to the default ones
    #[serde(default)]
    pub extra_input_globs: Vec<String>,

    /// List of additional compilers to use (e.g., ["c", "cxx"])
    /// The Go compiler (go-cgo or go-nocgo) is always added based on cgo_enabled.
    pub compilers: Option<Vec<String>>,
}

impl BackendConfig for GoBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            cgo_enabled: target_config.cgo_enabled || self.cgo_enabled,
            collect_licenses: target_config.collect_licenses || self.collect_licenses,
            extra_args: if target_config.extra_args.is_empty() {
                self.extra_args.clone()
            } else {
                target_config.extra_args.clone()
            },
            env: {
                let mut merged_env = self.env.clone();
                merged_env.extend(target_config.env.clone());
                merged_env
            },
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
            compilers: target_config
                .compilers
                .clone()
                .or_else(|| self.compilers.clone()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::GoBackendConfig;
    use pixi_build_backend::generated_recipe::BackendConfig;
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn test_ensure_deseralize_from_empty() {
        let json_data = json!({});
        serde_json::from_value::<GoBackendConfig>(json_data).unwrap();
    }

    #[test]
    fn test_cgo_enabled_deserialization() {
        let json_data = json!({"cgo-enabled": true});
        let config = serde_json::from_value::<GoBackendConfig>(json_data).unwrap();
        assert!(config.cgo_enabled);
    }

    #[test]
    fn test_collect_licenses_deserialization() {
        let json_data = json!({"collect-licenses": true});
        let config = serde_json::from_value::<GoBackendConfig>(json_data).unwrap();
        assert!(config.collect_licenses);
    }

    #[test]
    fn test_merge_with_target_config() {
        let base_config = GoBackendConfig {
            cgo_enabled: false,
            collect_licenses: false,
            extra_args: vec!["--base-arg".to_string()],
            env: indexmap::IndexMap::from([("BASE_VAR".to_string(), "base_value".to_string())]),
            debug_dir: Some(PathBuf::from("/base/debug")),
            extra_input_globs: vec!["*.base".to_string()],
            compilers: Some(vec!["c".to_string()]),
        };

        let target_config = GoBackendConfig {
            cgo_enabled: true,
            collect_licenses: true,
            extra_args: vec!["--target-arg".to_string()],
            env: indexmap::IndexMap::from([("TARGET_VAR".to_string(), "target_value".to_string())]),
            debug_dir: None,
            extra_input_globs: vec!["*.target".to_string()],
            compilers: Some(vec!["c".to_string(), "cxx".to_string()]),
        };

        let merged = base_config
            .merge_with_target_config(&target_config)
            .unwrap();

        assert!(merged.cgo_enabled);
        assert!(merged.collect_licenses);
        assert_eq!(merged.extra_args, vec!["--target-arg".to_string()]);
        assert_eq!(merged.env.get("BASE_VAR"), Some(&"base_value".to_string()));
        assert_eq!(
            merged.env.get("TARGET_VAR"),
            Some(&"target_value".to_string())
        );
        assert_eq!(merged.extra_input_globs, vec!["*.target".to_string()]);
        assert_eq!(
            merged.compilers,
            Some(vec!["c".to_string(), "cxx".to_string()])
        );
    }

    #[test]
    fn test_merge_target_debug_dir_error() {
        let base_config = GoBackendConfig {
            debug_dir: Some(PathBuf::from("/base/debug")),
            ..Default::default()
        };

        let target_config = GoBackendConfig {
            debug_dir: Some(PathBuf::from("/target/debug")),
            ..Default::default()
        };

        let result = base_config.merge_with_target_config(&target_config);
        assert!(result.is_err());
    }
}
