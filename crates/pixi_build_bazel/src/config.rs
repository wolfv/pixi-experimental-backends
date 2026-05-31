use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct BazelBackendConfig {
    /// Bazel targets to build (default: ["//..."]).
    #[serde(default = "default_targets")]
    pub targets: Vec<String>,

    /// Targets whose output files (binaries) should be installed into
    /// `$PREFIX/bin`. The output paths are resolved with
    /// `bazel cquery --output=files`, so this works regardless of where in
    /// `bazel-bin` the artifact ends up. Defaults to empty (build only).
    #[serde(default)]
    pub install_targets: Vec<String>,

    /// Extra arguments passed to `bazel build` / `bazel cquery`
    /// (e.g. ["--config=opt", "-c", "opt"]).
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Environment variables to set during the build.
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// Deprecated. Setting this has no effect.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,

    /// Extra input globs to include in addition to the default ones.
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

fn default_targets() -> Vec<String> {
    vec!["//...".to_string()]
}

impl Default for BazelBackendConfig {
    fn default() -> Self {
        Self {
            targets: default_targets(),
            install_targets: Vec::new(),
            extra_args: Vec::new(),
            env: IndexMap::new(),
            debug_dir: None,
            extra_input_globs: Vec::new(),
        }
    }
}

impl BackendConfig for BazelBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            targets: if target_config.targets == default_targets() {
                self.targets.clone()
            } else {
                target_config.targets.clone()
            },
            install_targets: if target_config.install_targets.is_empty() {
                self.install_targets.clone()
            } else {
                target_config.install_targets.clone()
            },
            extra_args: if target_config.extra_args.is_empty() {
                self.extra_args.clone()
            } else {
                target_config.extra_args.clone()
            },
            env: {
                let mut merged = self.env.clone();
                merged.extend(target_config.env.clone());
                merged
            },
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
        })
    }
}
