const NANOCLAUDE: &str = include_str!("../prompts/nanoclaude.md");
const CODE_MODE: &str = include_str!("../prompts/code_mode.md");

pub(crate) fn default_system(code_only: bool) -> String {
    if code_only {
        format!("{}\n\n{}", NANOCLAUDE.trim_end(), CODE_MODE.trim_end())
    } else {
        NANOCLAUDE.trim_end().to_owned()
    }
}
