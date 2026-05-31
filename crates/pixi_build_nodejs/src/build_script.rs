use minijinja::Environment;
use serde::Serialize;

/// A source→dest pair for extra asset directories.
#[derive(Serialize, Debug, Clone)]
pub struct AssetPair {
    pub src: String,
    pub dst: String,
}

#[derive(Serialize)]
pub struct BuildScriptContext {
    /// Absolute path to the source directory.
    pub source_dir: String,

    /// Package manager: "npm", "yarn", "pnpm", or "bun".
    pub package_manager: String,

    /// Extra arguments appended to the install command.
    pub extra_install_args: Vec<String>,

    /// Build script name (e.g. "build"). None means auto-detect from package.json.
    pub build_script: Option<String>,

    /// Extra arguments appended to the build command.
    pub extra_build_args: Vec<String>,

    /// Subdirectory of source_dir to install into `$PREFIX/share/$PKG_NAME`.
    /// None means the whole project (excluding node_modules).
    pub build_output_dir: Option<String>,

    /// Additional asset directories to copy: (source_subdir, dest_subdir) pairs.
    pub extra_assets: Vec<AssetPair>,

    /// Entry point relative to install dir for which a bin launcher is created.
    pub server_entry: Option<String>,

    /// True when building on Unix (bash); false on Windows (cmd.exe).
    pub is_bash: bool,
}

impl BuildScriptContext {
    pub fn render(&self) -> String {
        let env = Environment::new();
        let template = env
            .template_from_str(include_str!("build_script.j2"))
            .unwrap();
        template.render(self).unwrap().trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn default_ctx(is_bash: bool) -> BuildScriptContext {
        BuildScriptContext {
            source_dir: "/src/my-app".to_string(),
            package_manager: "npm".to_string(),
            extra_install_args: vec![],
            build_script: None,
            extra_build_args: vec![],
            build_output_dir: None,
            extra_assets: vec![],
            server_entry: None,
            is_bash,
        }
    }

    #[rstest]
    fn test_default_script(#[values(true, false)] is_bash: bool) {
        let script = default_ctx(is_bash).render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }

    #[rstest]
    fn test_pnpm_with_frozen_lockfile(#[values(true, false)] is_bash: bool) {
        let ctx = BuildScriptContext {
            package_manager: "pnpm".to_string(),
            extra_install_args: vec!["--frozen-lockfile".to_string()],
            ..default_ctx(is_bash)
        };
        let script = ctx.render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }

    #[rstest]
    fn test_explicit_build_script(#[values(true, false)] is_bash: bool) {
        let ctx = BuildScriptContext {
            build_script: Some("build:prod".to_string()),
            extra_build_args: vec!["--verbose".to_string()],
            ..default_ctx(is_bash)
        };
        let script = ctx.render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }

    #[rstest]
    fn test_nextjs_standalone(#[values(true, false)] is_bash: bool) {
        let ctx = BuildScriptContext {
            build_output_dir: Some(".next/standalone".to_string()),
            extra_assets: vec![
                AssetPair {
                    src: ".next/static".to_string(),
                    dst: ".next/static".to_string(),
                },
                AssetPair {
                    src: "public".to_string(),
                    dst: "public".to_string(),
                },
            ],
            server_entry: Some("server.js".to_string()),
            ..default_ctx(is_bash)
        };
        let script = ctx.render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }

    #[rstest]
    fn test_server_entry_only(#[values(true, false)] is_bash: bool) {
        let ctx = BuildScriptContext {
            build_output_dir: Some("dist".to_string()),
            server_entry: Some("index.js".to_string()),
            ..default_ctx(is_bash)
        };
        let script = ctx.render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }

    #[rstest]
    fn test_yarn_no_build_output(#[values(true, false)] is_bash: bool) {
        let ctx = BuildScriptContext {
            package_manager: "yarn".to_string(),
            ..default_ctx(is_bash)
        };
        let script = ctx.render();
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| insta::assert_snapshot!(script));
    }
}
