mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::GradleBackendConfig;
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
pub struct GradleGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for GradleGenerator {
    type Config = GradleBackendConfig;

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

        // Add JDK as a build requirement
        let openjdk = SourcePackageName::from("openjdk");
        if !model_dependencies.build.contains_key(&openjdk) {
            requirements
                .build
                .push("openjdk".parse().into_diagnostic()?);
        }

        // Add system gradle if not using the wrapper
        if !config.use_wrapper {
            let gradle = SourcePackageName::from("gradle");
            if !model_dependencies.build.contains_key(&gradle) {
                requirements
                    .build
                    .push("gradle".parse().into_diagnostic()?);
            }
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            tasks: config.tasks.clone(),
            extra_args: config.extra_args.clone(),
            use_wrapper: config.use_wrapper,
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
            "**/*.gradle",
            "**/*.gradle.kts",
            "settings.gradle",
            "settings.gradle.kts",
            "gradle/wrapper/gradle-wrapper.properties",
            "src/**/*.java",
            "src/**/*.kt",
            "src/**/*.scala",
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
        IntermediateBackendInstantiator::<GradleGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
