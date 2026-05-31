mod build_script;
mod config;
mod metadata;

use build_script::{AssetPair, BuildScriptContext};
use config::NodejsBackendConfig;
use metadata::NodejsMetadataProvider;
use miette::IntoDiagnostic;
use pixi_build_backend::{
    Variable,
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
    traits::ProjectModel,
    variants::NormalizedKey,
};
use pixi_build_types::SourcePackageName;
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
pub struct NodejsGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for NodejsGenerator {
    type Config = NodejsBackendConfig;

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

        let mut nodejs_metadata = NodejsMetadataProvider::new(&manifest_root);
        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut nodejs_metadata).into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;
        let model_dependencies = model.dependencies(Some(host_platform));

        // Ensure nodejs is available during the build (needed to run node scripts
        // that create bin launchers and to detect package.json scripts).
        let nodejs_pkg = SourcePackageName::from("nodejs");
        if !model_dependencies.build.contains_key(&nodejs_pkg) {
            requirements
                .build
                .push("nodejs".parse().into_diagnostic()?);
        }

        // Add the chosen package manager if it is not npm (npm ships with nodejs).
        if config.package_manager != "npm" {
            let pkg_mgr = SourcePackageName::from(config.package_manager.as_str());
            if !model_dependencies.build.contains_key(&pkg_mgr) {
                requirements
                    .build
                    .push(config.package_manager.parse().into_diagnostic()?);
            }
        }

        // Parse "source:dest" asset pairs.
        let extra_assets: Vec<AssetPair> = config
            .extra_assets
            .iter()
            .map(|s| {
                let mut parts = s.splitn(2, ':');
                let src = parts.next().unwrap_or(s).to_string();
                let dst = parts.next().unwrap_or(src.as_str()).to_string();
                AssetPair { src, dst }
            })
            .collect();

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            package_manager: config.package_manager.clone(),
            extra_install_args: config.extra_install_args.clone(),
            build_script: config.build_script.clone(),
            extra_build_args: config.extra_build_args.clone(),
            build_output_dir: config.build_output_dir.clone(),
            extra_assets,
            server_entry: config.server_entry.clone(),
            is_bash: !Platform::current().is_windows(),
        }
        .render();

        generated_recipe.recipe.build.script = Script {
            content: build_script,
            env: config.env.clone(),
            ..Default::default()
        };

        generated_recipe
            .metadata_input_globs
            .extend(nodejs_metadata.input_globs());

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok([
            "package.json",
            "package-lock.json",
            "yarn.lock",
            "pnpm-lock.yaml",
            "bun.lockb",
            "src/**/*.ts",
            "src/**/*.tsx",
            "src/**/*.js",
            "src/**/*.jsx",
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
        IntermediateBackendInstantiator::<NodejsGenerator>::new(log, Arc::default())
    })
    .await
    {
        eprintln!("{err:?}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use recipe_stage0::recipe::{Item, Value};

    use super::*;

    #[macro_export]
    macro_rules! project_fixture {
        ($($json:tt)+) => {
            serde_json::from_value::<pixi_build_types::ProjectModel>(
                serde_json::json!($($json)+)
            ).expect("Failed to create ProjectModel from JSON fixture.")
        };
    }

    #[tokio::test]
    async fn test_nodejs_is_in_build_requirements() {
        let model = project_fixture!({
            "name": "my-app",
            "version": "1.0.0",
            "targets": { "defaultTarget": {} }
        });

        let recipe = NodejsGenerator::default()
            .generate_recipe(
                &model,
                &NodejsBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .unwrap();

        let has_nodejs = recipe
            .recipe
            .requirements
            .build
            .iter()
            .any(|item| format!("{item:?}").contains("nodejs"));
        assert!(has_nodejs, "nodejs should be in build requirements");

        insta::assert_yaml_snapshot!(recipe.recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_pnpm_added_as_build_requirement() {
        let model = project_fixture!({
            "name": "my-app",
            "version": "1.0.0",
            "targets": { "defaultTarget": {} }
        });

        let recipe = NodejsGenerator::default()
            .generate_recipe(
                &model,
                &NodejsBackendConfig {
                    package_manager: "pnpm".to_string(),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .unwrap();

        let has_pnpm = recipe
            .recipe
            .requirements
            .build
            .iter()
            .any(|item| format!("{item:?}").contains("pnpm"));
        assert!(has_pnpm, "pnpm should be in build requirements");
    }

    #[tokio::test]
    async fn test_npm_not_added_separately() {
        let model = project_fixture!({
            "name": "my-app",
            "version": "1.0.0",
            "targets": { "defaultTarget": {} }
        });

        let recipe = NodejsGenerator::default()
            .generate_recipe(
                &model,
                &NodejsBackendConfig::default(), // npm is the default
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .unwrap();

        // npm should NOT appear as a separate requirement (it ships with nodejs)
        let has_npm = recipe
            .recipe
            .requirements
            .build
            .iter()
            .any(|item| {
                if let Item::Value(Value::Concrete(s)) = item {
                    s.to_string() == "npm"
                } else {
                    false
                }
            });
        assert!(!has_npm, "npm should not be added separately");
    }

    #[tokio::test]
    async fn test_env_vars_in_build_script() {
        let model = project_fixture!({
            "name": "my-app",
            "version": "1.0.0",
            "targets": { "defaultTarget": {} }
        });

        let env = IndexMap::from([("NODE_ENV".to_string(), "production".to_string())]);

        let recipe = NodejsGenerator::default()
            .generate_recipe(
                &model,
                &NodejsBackendConfig {
                    env: env.clone(),
                    ..Default::default()
                },
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .unwrap();

        insta::assert_yaml_snapshot!(recipe.recipe.build.script, {
            ".content" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_input_globs_include_lockfiles() {
        let config = NodejsBackendConfig::default();
        let globs = NodejsGenerator::default()
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        assert!(globs.contains("package.json"));
        assert!(globs.contains("package-lock.json"));
        assert!(globs.contains("yarn.lock"));
        assert!(globs.contains("pnpm-lock.yaml"));
    }

    #[test]
    fn test_input_globs_include_extra() {
        let config = NodejsBackendConfig {
            extra_input_globs: vec!["config/**/*.json".to_string()],
            ..Default::default()
        };
        let globs = NodejsGenerator::default()
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        assert!(globs.contains("config/**/*.json"));
    }

    #[tokio::test]
    async fn test_metadata_from_package_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name": "my-next-app", "version": "0.5.0"}"#,
        )
        .unwrap();

        // Project model without name - should be derived from package.json
        let model = project_fixture!({
            "version": "0.5.0",
            "targets": { "defaultTarget": {} }
        });

        let recipe = NodejsGenerator::default()
            .generate_recipe(
                &model,
                &NodejsBackendConfig::default(),
                dir.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .unwrap();

        assert_eq!(recipe.recipe.package.name.to_string(), "my-next-app");
    }
}
