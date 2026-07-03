//! `[package.build.config]` for the HF-kernel backend.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Deserialize, Serialize, Clone)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HfKernelConfig {
    /// Hub repo id, e.g. `kernels-community/flash-attn`.
    pub repo: String,
    /// Git ref (branch/tag/sha). Resolved to a concrete commit for reproducibility.
    #[serde(default)]
    pub rev: Option<String>,
    /// Override the produced conda package name (defaults to the workspace package).
    #[serde(default)]
    pub package_name: Option<String>,
    /// Drop cxx98-ABI variants (conda-forge is cxx11-only). Default true.
    #[serde(default = "default_true")]
    pub require_cxx11: bool,

    /// Explicit variant directory list. When unset the backend lists `build/`
    /// on the Hub. Handy for offline/deterministic builds and testing.
    #[serde(default)]
    pub variants: Option<Vec<String>>,

    /// Override `cuda-capabilities` (otherwise read from the repo's build.toml).
    #[serde(default)]
    pub cuda_capabilities: Option<Vec<String>>,

    #[serde(default)]
    pub debug_dir: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}
