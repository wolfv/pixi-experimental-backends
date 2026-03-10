use minijinja::Environment;
use serde::Serialize;

#[derive(Serialize)]
pub struct BuildScriptContext {
    pub build_platform: BuildPlatform,
    pub source_dir: String,
    pub extra_args: Vec<String>,
}

#[derive(Copy, Clone, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BuildPlatform {
    Windows,
    Unix,
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
