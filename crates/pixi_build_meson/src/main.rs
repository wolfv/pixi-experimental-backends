mod build_script;
mod config;

use build_script::{BuildPlatform, BuildScriptContext};
use config::MesonBackendConfig;
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
pub struct MesonGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for MesonGenerator {
    type Config = MesonBackendConfig;

    async fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModel,
        config: &Self::Config,
        manifest_path: PathBuf,
        _host_platform: Platform,
        _python_params: Option<PythonParams>,
        variants: &HashSet<NormalizedKey>,
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

        // Get the list of compilers from config, defaulting to ["cxx"] if not specified
        let compilers = config
            .compilers
            .clone()
            .unwrap_or_else(|| vec!["cxx".to_string()]);

        pixi_build_backend::compilers::add_compilers_to_requirements(
            &compilers,
            &mut requirements.build,
        );
        pixi_build_backend::compilers::add_stdlib_to_requirements(
            &compilers,
            &mut requirements.build,
            variants,
        );

        // Add necessary build tools: meson, ninja, pkg-config
        for tool in ["meson", "ninja", "pkg-config"] {
            requirements.build.push(req(tool));
        }

        let build_script = BuildScriptContext {
            build_platform: if Platform::current().is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: manifest_root.display().to_string(),
            extra_args: config.extra_args.clone(),
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
            // Source files
            "**/*.{c,cc,cxx,cpp,h,hpp,hxx}",
            // Meson files
            "**/meson.build",
            "**/meson.options",
            "**/meson_options.txt",
        ]
        .iter()
        .map(|s: &&str| s.to_string())
        .chain(config.extra_input_globs.clone())
        .collect())
    }

    fn default_variants(
        &self,
        host_platform: Platform,
    ) -> miette::Result<BTreeMap<NormalizedKey, Vec<Variable>>> {
        let mut variants = BTreeMap::new();

        if host_platform.is_windows() {
            variants.insert(NormalizedKey::from("c_compiler"), vec!["vs2022".into()]);
            variants.insert(NormalizedKey::from("cxx_compiler"), vec!["vs2022".into()]);
        }

        Ok(variants)
    }
}

#[tokio::main]
pub async fn main() {
    if let Err(err) = pixi_build_backend::cli::main(|log| {
        IntermediateBackendInstantiator::<MesonGenerator>::new(
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
