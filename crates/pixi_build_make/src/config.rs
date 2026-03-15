use indexmap::IndexMap;
use pixi_build_backend::generated_recipe::BackendConfig;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct MakeBackendConfig {
    /// Extra arguments passed to `make` during the build step.
    #[serde(default)]
    pub extra_make_args: Vec<String>,

    /// Extra arguments passed to `make install`.
    /// `PREFIX=$PREFIX` is always prepended on Unix.
    #[serde(default)]
    pub extra_install_args: Vec<String>,

    /// Environment variables to set during the build.
    #[serde(default)]
    pub env: IndexMap<String, String>,

    /// List of compilers to use (e.g., ["c"], ["c", "cxx"]).
    /// Defaults to ["c"].
    pub compilers: Option<Vec<String>>,

    /// Deprecated. Setting this has no effect.
    #[serde(alias = "debug_dir")]
    pub debug_dir: Option<PathBuf>,

    /// Extra input globs to include in addition to the default ones.
    #[serde(default)]
    pub extra_input_globs: Vec<String>,
}

impl BackendConfig for MakeBackendConfig {
    fn debug_dir(&self) -> Option<&Path> {
        self.debug_dir.as_deref()
    }

    fn merge_with_target_config(&self, target_config: &Self) -> miette::Result<Self> {
        if target_config.debug_dir.is_some() {
            miette::bail!("`debug_dir` cannot have a target specific value");
        }

        Ok(Self {
            extra_make_args: if target_config.extra_make_args.is_empty() {
                self.extra_make_args.clone()
            } else {
                target_config.extra_make_args.clone()
            },
            extra_install_args: if target_config.extra_install_args.is_empty() {
                self.extra_install_args.clone()
            } else {
                target_config.extra_install_args.clone()
            },
            env: {
                let mut merged = self.env.clone();
                merged.extend(target_config.env.clone());
                merged
            },
            compilers: target_config
                .compilers
                .clone()
                .or_else(|| self.compilers.clone()),
            debug_dir: self.debug_dir.clone(),
            extra_input_globs: if target_config.extra_input_globs.is_empty() {
                self.extra_input_globs.clone()
            } else {
                target_config.extra_input_globs.clone()
            },
        })
    }
}
