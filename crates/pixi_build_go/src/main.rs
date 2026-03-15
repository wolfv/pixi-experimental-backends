mod build_script;
mod config;
mod metadata;

use build_script::BuildScriptContext;
use config::GoBackendConfig;
use metadata::GoMetadataProvider;
use miette::IntoDiagnostic;
use pixi_build_backend::variants::NormalizedKey;
use pixi_build_backend::{
    Variable,
    generated_recipe::{GenerateRecipe, GeneratedRecipe, PythonParams},
    intermediate_backend::IntermediateBackendInstantiator,
    traits::ProjectModel,
};
use rattler_conda_types::{ChannelUrl, Platform};
use recipe_stage0::{
    matchspec::PackageDependency,
    recipe::{Item, Script},
};
use std::collections::HashSet;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Default, Clone)]
pub struct GoGenerator {}

#[async_trait::async_trait]
impl GenerateRecipe for GoGenerator {
    type Config = GoBackendConfig;

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

        let mut go_metadata = GoMetadataProvider::new(&manifest_root, false);

        let mut generated_recipe =
            GeneratedRecipe::from_model(model.clone(), &mut go_metadata).into_diagnostic()?;

        let requirements = &mut generated_recipe.recipe.requirements;

        let model_dependencies = model.dependencies(Some(host_platform));

        // Determine the Go compiler based on cgo_enabled
        let go_compiler = if config.cgo_enabled {
            "go-cgo"
        } else {
            "go-nocgo"
        };

        // Build the list of compilers: Go compiler + any additional compilers
        let mut compilers = vec![go_compiler.to_string()];
        if let Some(extra_compilers) = &config.compilers {
            compilers.extend(extra_compilers.clone());
        }

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

        // Add go-licenses to build requirements if license collection is enabled
        if config.collect_licenses {
            let go_licenses_dep: Item<PackageDependency> =
                "go-licenses".parse().into_diagnostic()?;
            requirements.build.push(go_licenses_dep);
        }

        let build_script = BuildScriptContext {
            source_dir: manifest_root.display().to_string(),
            linker_flags: config.linker_flags.clone(),
            extra_args: config.extra_args.clone(),
            collect_licenses: config.collect_licenses,
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
            .extend(go_metadata.input_globs());

        Ok(generated_recipe)
    }

    fn extract_input_globs_from_build(
        &self,
        config: &Self::Config,
        _workdir: impl AsRef<Path>,
        _editable: bool,
    ) -> miette::Result<BTreeSet<String>> {
        Ok([
            "**/*.go",
            "go.mod",
            "go.sum",
        ]
        .iter()
        .map(|s| s.to_string())
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
        IntermediateBackendInstantiator::<GoGenerator>::new(log, Arc::default())
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
            ).expect("Failed to create TestProjectModel from JSON fixture.")
        };
    }

    #[tokio::test]
    async fn test_go_nocgo_compiler_is_in_build_requirements() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig::default(),
                PathBuf::from("."),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .expect("Failed to generate recipe");

        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(compiler_templates.len(), 1);
        assert!(compiler_templates.contains(&"${{ compiler('go-nocgo') }}".to_string()));

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_go_cgo_compiler_is_in_build_requirements() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig {
                    cgo_enabled: true,
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
            .expect("Failed to generate recipe");

        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        assert!(compiler_templates.contains(&"${{ compiler('go-cgo') }}".to_string()));

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_collect_licenses_adds_go_licenses_dep() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig {
                    collect_licenses: true,
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
            .expect("Failed to generate recipe");

        let build_reqs = &generated_recipe.recipe.requirements.build;
        let has_go_licenses = build_reqs
            .iter()
            .any(|item| format!("{item:?}").contains("go-licenses"));
        assert!(
            has_go_licenses,
            "go-licenses should be in build requirements when collect_licenses is true"
        );

        insta::assert_yaml_snapshot!(generated_recipe.recipe, {
            ".source[0].path" => "[ ... path ... ]",
            ".build.script" => "[ ... script ... ]",
        });
    }

    #[tokio::test]
    async fn test_env_vars_are_set() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {
                    "runDependencies": {
                        "boltons": {
                            "binary": {
                                "version": "*"
                            }
                        }
                    }
                },
            }
        });

        let env = IndexMap::from([("foo".to_string(), "bar".to_string())]);

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig {
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
            .expect("Failed to generate recipe");

        insta::assert_yaml_snapshot!(generated_recipe.recipe.build.script, {
            ".content" => "[ ... script ... ]",
        });
    }

    #[test]
    fn test_input_globs_includes_extra_globs() {
        let config = GoBackendConfig {
            extra_input_globs: vec!["custom/*.txt".to_string()],
            ..Default::default()
        };

        let generator = GoGenerator::default();
        let result = generator
            .extract_input_globs_from_build(&config, PathBuf::new(), false)
            .unwrap();

        assert!(result.contains("**/*.go"));
        assert!(result.contains("go.mod"));
        assert!(result.contains("go.sum"));
        assert!(result.contains("custom/*.txt"));
    }

    #[tokio::test]
    async fn test_with_go_mod_metadata() {
        let temp = tempfile::TempDir::new().unwrap();
        std::fs::write(
            temp.path().join("go.mod"),
            "module github.com/user/my-go-tool\n\ngo 1.22\n",
        )
        .unwrap();

        // Project model without name - should be derived from go.mod
        let project_model = project_fixture!({
            "version": "1.0.0",
            "targets": {
                "defaultTarget": {}
            }
        });

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig::default(),
                temp.path().to_path_buf(),
                Platform::Linux64,
                None,
                &HashSet::new(),
                vec![],
                None,
            )
            .await
            .expect("Failed to generate recipe");

        assert_eq!(
            generated_recipe.recipe.package.name.to_string(),
            "my-go-tool"
        );
        assert!(generated_recipe.metadata_input_globs.contains("go.mod"));
    }

    #[tokio::test]
    async fn test_additional_compilers() {
        let project_model = project_fixture!({
            "name": "foobar",
            "version": "0.1.0",
            "targets": {
                "defaultTarget": {}
            }
        });

        let generated_recipe = GoGenerator::default()
            .generate_recipe(
                &project_model,
                &GoBackendConfig {
                    cgo_enabled: true,
                    compilers: Some(vec!["c".to_string(), "cxx".to_string()]),
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
            .expect("Failed to generate recipe");

        let build_reqs = &generated_recipe.recipe.requirements.build;
        let compiler_templates: Vec<String> = build_reqs
            .iter()
            .filter_map(|item| match item {
                Item::Value(Value::Template(s)) if s.contains("compiler") => Some(s.clone()),
                _ => None,
            })
            .collect();

        assert_eq!(compiler_templates.len(), 3);
        assert!(compiler_templates.contains(&"${{ compiler('go-cgo') }}".to_string()));
        assert!(compiler_templates.contains(&"${{ compiler('c') }}".to_string()));
        assert!(compiler_templates.contains(&"${{ compiler('cxx') }}".to_string()));
    }
}
