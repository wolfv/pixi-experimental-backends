mod build_script;
mod config;

use build_script::{BuildPlatform, BuildScriptContext};
use config::AutotoolsBackendConfig;
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
pub struct AutotoolsGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for AutotoolsGenerator {
    type Config = AutotoolsBackendConfig;

    async fn generate_recipe(
        &self,
        model: &pixi_build_types::ProjectModel,
        config: &Self::Config,
        manifest_path: PathBuf,
        host_platform: Platform,
        _python_params: Option<PythonParams>,
        variants: &HashSet<NormalizedKey>,
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

        // Get the list of compilers from config, defaulting to ["cxx"] if not specified
        let compilers = config
            .compilers
            .clone()
            .unwrap_or_else(|| vec!["cxx".to_string()]);

        pixi_build_backend::compilers::add_compilers_to_requirements(
            &compilers,
            &mut requirements.build,
            &model_dependencies,
            &host_platform,
        );
        pixi_build_backend::compilers::add_stdlib_to_requirements(
            &compilers,
            &mut requirements.build,
            variants,
        );

        // Add necessary build tools
        let mut tools: Vec<&str> = vec!["make", "pkg-config"];
        if config.autoreconf {
            tools.extend_from_slice(&["autoconf", "automake", "libtool"]);
        }

        // On Windows, use MSYS2 tools (m2-* packages) like conda-forge does
        if host_platform.is_windows() {
            let m2_tools: Vec<String> = tools.iter().map(|t| format!("m2-{t}")).collect();
            for tool in &m2_tools {
                let tool_name = SourcePackageName::from(tool.as_str());
                if !model_dependencies.build.contains_key(&tool_name) {
                    requirements.build.push(tool.parse().into_diagnostic()?);
                }
            }
            // Also need m2-bash to drive the build
            let m2_bash = SourcePackageName::from("m2-bash");
            if !model_dependencies.build.contains_key(&m2_bash) {
                requirements
                    .build
                    .push("m2-bash".parse().into_diagnostic()?);
            }
        } else {
            for tool in &tools {
                let tool_name = SourcePackageName::from(*tool);
                if !model_dependencies.build.contains_key(&tool_name) {
                    requirements.build.push(tool.parse().into_diagnostic()?);
                }
            }
        }

        let build_script = BuildScriptContext {
            build_platform: if Platform::current().is_windows() {
                BuildPlatform::Windows
            } else {
                BuildPlatform::Unix
            },
            source_dir: manifest_root.display().to_string(),
            extra_configure_args: config.extra_configure_args.clone(),
            extra_make_args: config.extra_make_args.clone(),
            autoreconf: config.autoreconf,
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
            // Source files
            "**/*.{c,cc,cxx,cpp,h,hpp,hxx}",
            // Autotools files
            "**/configure.ac",
            "**/configure.in",
            "**/Makefile.am",
            "**/Makefile.in",
            "**/aclocal.m4",
            "**/configure",
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
        IntermediateBackendInstantiator::<AutotoolsGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}
