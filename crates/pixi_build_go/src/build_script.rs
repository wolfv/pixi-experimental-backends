use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    /// The location of the source
    pub source_dir: String,

    /// Linker flags passed via `-ldflags`
    pub linker_flags: Vec<String>,

    /// Any additional args to pass to `go install`
    pub extra_args: Vec<String>,

    /// Whether to collect licenses
    pub collect_licenses: bool,

    /// The platform that is running the build.
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
mod test {
    use rstest::*;

    #[rstest]
    fn test_build_script(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            source_dir: String::from("my-prefix-dir"),
            linker_flags: vec![],
            extra_args: vec![],
            collect_licenses: false,
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }

    #[rstest]
    fn test_build_script_with_licenses(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            source_dir: String::from("my-prefix-dir"),
            linker_flags: vec![],
            extra_args: vec![],
            collect_licenses: true,
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }

    #[rstest]
    fn test_build_script_with_linker_flags(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            source_dir: String::from("my-prefix-dir"),
            linker_flags: vec!["-s".to_string(), "-w".to_string()],
            extra_args: vec![],
            collect_licenses: false,
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }

    #[rstest]
    fn test_build_script_with_extra_args(#[values(true, false)] is_bash: bool) {
        let context = super::BuildScriptContext {
            source_dir: String::from("my-prefix-dir"),
            linker_flags: vec![],
            extra_args: vec!["-ldflags".to_string(), "-s -w".to_string()],
            collect_licenses: false,
            is_bash,
        };
        let script = context.render();

        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_suffix(if is_bash { "bash" } else { "cmdexe" });
        settings.bind(|| {
            insta::assert_snapshot!(script);
        });
    }
}
