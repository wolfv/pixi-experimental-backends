use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GradleBackendConfig {
    /// Gradle tasks to run (default: ["installDist"]).
    #[serde(default = "default_tasks")]
    pub tasks: Vec<String>,

    /// Extra arguments to pass to gradlew / gradle.
    #[serde(default)]
    pub extra_args: Vec<String>,

    /// Environment variables to set during the build.
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// Use the Gradle wrapper (./gradlew) instead of a system gradle.
    /// Default: true.
    #[serde(default = "default_true")]
    pub use_wrapper: bool,

    /// Deprecated. Setting this has no effect.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,

    /// Extra input globs to include in addition to the default ones.
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

fn default_tasks() -> Vec<String> {
    vec!["installDist".to_string()]
}

fn default_true() -> bool {
    true
}

impl Default for GradleBackendConfig {
    fn default() -> Self {
        Self {
            tasks: default_tasks(),
            extra_args: Vec::new(),
            env: IndexMap::new(),
            use_wrapper: true,
            debug_dir: None,
            extra_input_globs: Vec::new(),
        }
    }
}

impl BackendConfig for GradleBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            tasks: if target_config.tasks == default_tasks() {
                self.tasks.clone()
            } else {
                target_config.tasks.clone()
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
            use_wrapper: target_config.use_wrapper,
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
        })
    }
}
