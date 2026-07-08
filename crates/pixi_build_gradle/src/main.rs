mod build_script;
mod config;

use build_script::BuildScriptContext;
use config::GradleBackendConfig;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    generated_recipe::{DefaultMetadataProvider, GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
};
use rattler_build_jinja::Variable;
use rattler_build_recipe::stage0::{ConditionalList, Item, Script, SerializableMatchSpec, Value};
use rattler_build_types::NormalizedKey;
use rattler_conda_types::{ChannelUrl, Platform};
use std::collections::HashSet;
use std::path::PathBuf;
use std::{collections::BTreeMap, path::Path, sync::Arc};

fn req(name: impl Into<String>) -> Item<SerializableMatchSpec> {
    Item::Value(Value::new_concrete(name.into().parse().unwrap(), None))
}

fn script(content: String, env: indexmap::IndexMap<String, String>) -> Script {
    Script {
        content: Some(ConditionalList::new(vec![Item::Value(
            Value::new_concrete(content, None),
        )])),
        env: env
            .into_iter()
            .map(|(k, v)| (k, Value::new_concrete(v, None)))
            .collect(),
        ..Default::default()
    }
}

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
        _host_platform: Platform,
        _python_params: Option<PythonParams>,
        _variants: &HashSet<NormalizedKey>,
        _channels: Vec<ChannelUrl>,
        _cache_dir: Option<PathBuf>,
        _workspace_scratch_directory: Option<PathBuf>,
        _workspace_directory: Option<PathBuf>,
        _checkout_root: Option<PathBuf>,
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
        // Add JDK as a build requirement
        requirements.build.push(req("openjdk"));

        // Add system gradle if not using the wrapper
        if !config.use_wrapper {
            requirements.build.push(req("gradle"));
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            tasks: config.tasks.clone(),
            extra_args: config.extra_args.clone(),
            use_wrapper: config.use_wrapper,
            is_bash: !Platform::current().is_windows(),
        }
        .render();

        generated_recipe.recipe.build.script = script(build_script, config.env.clone());

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<Vec<String>> {
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
        IntermediateBackendInstantiator::<GradleGenerator>::new(
            pixi_build_backend::tools::BackendIdentifier::new(
                env!("CARGO_PKG_NAME"),
                env!("CARGO_PKG_VERSION"),
            ),
            log,
            Arc::default(),
        )
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
