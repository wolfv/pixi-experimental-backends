mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::BazelBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
    traits::ProjectModel,
};
use pixi_build_types::SourcePackageName;
use rattler_build_jinja::Variable;
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{ChannelUrl, Platform};
use recipe_stage0::recipe::Script;
use std::collections::HashSet;
use std::path::PathBuf;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct BazelGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for BazelGenerator {
    type Config = BazelBackendConfig;

    async fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModel,
        config: &Self::Config,
        manifest_path: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        _variants: &HashSet<NormalizedKey>,
        _channels: Vec<ChannelUrl>,
        _cache_dir: Option<PathBuf>,
    ) -> miette::Result<GeneratedRecipe> {
        let manifest_root = if manifest_path.is_file() {
            manifest_path
                .parent()
                .ok_or_else(|| {
                    miette::Error::msg(format!(
                        "Manifest path {} is a file but has no parent directory.",
                        manifest_path.display()
                    ))
                })?
                .to_path_buf()
        } else {
            manifest_path.clone()
        };

        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut DefaultMetadataProvider)
                .into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;
        let model_dependencies = model.dependencies(Some(host_platform));

        // Add bazel as a build requirement unless the user already pulled it in.
        // `bazel` on conda-forge ships Bazelisk, which respects a checked-in
        // `.bazelversion`, so this works for projects pinning a specific Bazel.
        let bazel = SourcePackageName::from("bazel");
        if !model_dependencies.build.contains_key(&bazel) {
            requirements.build.push("bazel".parse().into_diagnostic()?);
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            targets: config.targets.clone(),
            install_targets: config.install_targets.clone(),
            extra_args: config.extra_args.clone(),
            is_bash: !Platform::current().is_windows(),
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok([
            "BUILD",
            "BUILD.bazel",
            "**/BUILD",
            "**/BUILD.bazel",
            "WORKSPACE",
            "WORKSPACE.bazel",
            "WORKSPACE.bzlmod",
            "MODULE.bazel",
            "MODULE.bazel.lock",
            "**/*.bzl",
            ".bazelrc",
            ".bazelversion",
        ]
        .iter()
        .map(|s| s.to_string())
        .chain(config.extra_input_globs.clone())
        .collect())
    }

    fn default_variants(
        &self,
        _host_platform: Platform,
    ) -> miette::Result<BTreeMap<NormalizedKey, Vec<Variable>>> {
        Ok(BTreeMap::new())
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<BazelGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
