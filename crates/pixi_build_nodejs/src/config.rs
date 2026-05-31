use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

fn default_package_manager() -> String {
    "npm".to_string()
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct NodejsBackendConfig {
    /// The package manager to use. One of "npm" (default), "yarn", "pnpm", or "bun".
    #[serde(default = "default_package_manager")]
    pub package_manager: String,

    /// Extra arguments to pass to the install command (e.g. ["--frozen-lockfile"]).
    #[serde(default)]
    pub extra_install_args: Vec<String>,

    /// The name of the build script to run (e.g. "build"). If not set, the backend
    /// auto-detects and runs the "build" script if it exists in package.json.
    pub build_script: Option<String>,

    /// Extra arguments to pass to the build command.
    #[serde(default)]
    pub extra_build_args: Vec<String>,

    /// The output directory to install to `$PREFIX/share/$PKG_NAME` (relative to
    /// the source dir). If not set, the entire project (excluding `node_modules`)
    /// is installed. Examples: ".next/standalone", "dist", "build".
    pub build_output_dir: Option<String>,

    /// Additional asset directories to copy into the install destination, specified
    /// as "source:dest" pairs relative to the source / install directories.
    /// Example: [".next/static:.next/static", "public:public"]
    #[serde(default)]
    pub extra_assets: Vec<String>,

    /// The server entry point relative to the install directory. When set, a
    /// self-relocating launcher script is created at `$PREFIX/bin/$PKG_NAME`.
    /// Example: "server.js", "dist/server.js"
    pub server_entry: Option<String>,

    /// Environment variables to set during the build.
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// Extra input globs to watch in addition to the defaults.
    #[serde(default)]
    pub extra_input_globs: Vec<String>,

    /// Deprecated. Setting this has no effect.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,
}

impl Default for NodejsBackendConfig {
    fn default() -> Self {
        Self {
            package_manager: default_package_manager(),
            extra_install_args: Vec::new(),
            build_script: None,
            extra_build_args: Vec::new(),
            build_output_dir: None,
            extra_assets: Vec::new(),
            server_entry: None,
            env: IndexMap::new(),
            extra_input_globs: Vec::new(),
            debug_dir: None,
        }
    }
}

impl BackendConfig for NodejsBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            // Use target's package_manager if it explicitly differs from the default
            package_manager: if target_config.package_manager != default_package_manager() {
                target_config.package_manager.clone()
            } else {
                self.package_manager.clone()
            },
            extra_install_args: if target_config.extra_install_args.is_empty() {
                self.extra_install_args.clone()
            } else {
                target_config.extra_install_args.clone()
            },
            build_script: target_config
                .build_script
                .clone()
                .or_else(|| self.build_script.clone()),
            extra_build_args: if target_config.extra_build_args.is_empty() {
                self.extra_build_args.clone()
            } else {
                target_config.extra_build_args.clone()
            },
            build_output_dir: target_config
                .build_output_dir
                .clone()
                .or_else(|| self.build_output_dir.clone()),
            extra_assets: if target_config.extra_assets.is_empty() {
                self.extra_assets.clone()
            } else {
                target_config.extra_assets.clone()
            },
            server_entry: target_config
                .server_entry
                .clone()
                .or_else(|| self.server_entry.clone()),
            env: {
                let mut merged = self.env.clone();
                merged.extend(target_config.env.clone());
                merged
            },
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
            debug_dir: self.debug_dir.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pixi_build_backend::generated_recipe::BackendConfig;
    use serde_json::json;

    #[test]
    fn test_deserialize_empty() {
        let config = serde_json::from_value::<NodejsBackendConfig>(json!({})).unwrap();
        assert_eq!(config.package_manager, "npm");
        assert!(config.extra_install_args.is_empty());
        assert!(config.build_script.is_none());
        assert!(config.server_entry.is_none());
    }

    #[test]
    fn test_deserialize_full() {
        let config = serde_json::from_value::<NodejsBackendConfig>(json!({
            "package-manager": "pnpm",
            "extra-install-args": ["--frozen-lockfile"],
            "build-script": "build:prod",
            "extra-build-args": ["--verbose"],
            "build-output-dir": ".next/standalone",
            "extra-assets": [".next/static:.next/static", "public:public"],
            "server-entry": "server.js",
            "env": {"NODE_ENV": "production"}
        }))
        .unwrap();

        assert_eq!(config.package_manager, "pnpm");
        assert_eq!(config.extra_install_args, vec!["--frozen-lockfile"]);
        assert_eq!(config.build_script, Some("build:prod".to_string()));
        assert_eq!(config.extra_build_args, vec!["--verbose"]);
        assert_eq!(
            config.build_output_dir,
            Some(".next/standalone".to_string())
        );
        assert_eq!(config.extra_assets.len(), 2);
        assert_eq!(config.server_entry, Some("server.js".to_string()));
        assert_eq!(config.env.get("NODE_ENV"), Some(&"production".to_string()));
    }

    #[test]
    fn test_merge_target_overrides_package_manager() {
        let base = NodejsBackendConfig {
            package_manager: "npm".to_string(),
            ..Default::default()
        };
        let target = NodejsBackendConfig {
            package_manager: "pnpm".to_string(),
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert_eq!(merged.package_manager, "pnpm");
    }

    #[test]
    fn test_merge_keeps_base_package_manager_when_target_is_default() {
        let base = NodejsBackendConfig {
            package_manager: "yarn".to_string(),
            ..Default::default()
        };
        let target = NodejsBackendConfig {
            package_manager: "npm".to_string(), // default
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert_eq!(merged.package_manager, "yarn");
    }

    #[test]
    fn test_merge_env_vars_are_combined() {
        let base = NodejsBackendConfig {
            env: indexmap::indexmap! {
                "BASE_VAR".to_string() => "base".to_string()
            },
            ..Default::default()
        };
        let target = NodejsBackendConfig {
            env: indexmap::indexmap! {
                "TARGET_VAR".to_string() => "target".to_string()
            },
            ..Default::default()
        };
        let merged = base.merge_with_target_config(&target).unwrap();
        assert_eq!(merged.env.get("BASE_VAR"), Some(&"base".to_string()));
        assert_eq!(merged.env.get("TARGET_VAR"), Some(&"target".to_string()));
    }

    #[test]
    fn test_merge_target_debug_dir_errors() {
        let base = NodejsBackendConfig::default();
        let target = NodejsBackendConfig {
            debug_dir: Some(PathBuf::from("/target/debug")),
            ..Default::default()
        };
        assert!(base.merge_with_target_config(&target).is_err());
    }
}
