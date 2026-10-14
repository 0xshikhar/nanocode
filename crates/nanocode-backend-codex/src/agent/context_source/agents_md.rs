use std::{path::Path, sync::Arc};

use nanocode_home::{AgentHome, Convention, Diagnostic, Level, ProjectHome};
use tracing::warn;

const PROJECT_DOC_SEPARATOR: &str = "\n\n--- project-doc ---\n\n";

/// User-level instructions from the configured homes: the Codex home's
/// `AGENTS.override.md` (when non-empty) or `AGENTS.md`, then the Claude
/// home's `CLAUDE.md` when it is a distinct document.
pub(crate) fn load_global_instructions(
    codex_home: Option<&Path>,
    claude_home: Option<&Path>,
) -> Option<Arc<str>> {
    let (codex, claude) = match (codex_home, claude_home) {
        (None, None) => return None,
        (Some(codex), Some(claude)) => (codex, claude),
        (Some(home), None) | (None, Some(home)) => (home, home),
    };
    let global = AgentHome::new(codex, claude).global_instructions();
    report(&global.diagnostics);
    // A home that was not configured contributes nothing, even when the
    // configured one is passed for both.
    let texts = global
        .sources
        .iter()
        .filter(|source| match source.convention {
            Convention::Codex => codex_home.is_some(),
            Convention::Claude => claude_home.is_some(),
        })
        .map(|source| source.text.as_str())
        .collect::<Vec<_>>();
    (!texts.is_empty()).then(|| Arc::from(texts.join("\n\n")))
}

pub(super) fn load_instructions(
    workspace: &Path,
    global_instructions: Option<&str>,
) -> Option<String> {
    combine_instructions(
        global_instructions,
        load_project_instructions(workspace).as_deref(),
    )
}

pub(super) fn combine_instructions(
    global_instructions: Option<&str>,
    project_instructions: Option<&str>,
) -> Option<String> {
    match (global_instructions, project_instructions) {
        (Some(global), Some(project)) => Some(format!("{global}{PROJECT_DOC_SEPARATOR}{project}")),
        (Some(global), None) => Some(global.to_owned()),
        (None, Some(project)) => Some(project.to_owned()),
        (None, None) => None,
    }
}

/// Project instructions root-to-leaf, each loaded whole: per directory
/// `AGENTS.override.md` or `AGENTS.md`, then the Claude conventions
/// `CLAUDE.md`, `CLAUDE.local.md`, and `.claude/CLAUDE.md`, each distinct
/// document once.
fn load_project_instructions(workspace: &Path) -> Option<String> {
    let instructions = ProjectHome::discover(workspace).read_instructions();
    report(&instructions.diagnostics);
    instructions.combined()
}

fn report(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        if matches!(diagnostic.level, Level::Warning) {
            warn!(
                path = %diagnostic.path.display(),
                message = %diagnostic.message,
                "instruction file skipped"
            );
        }
    }
}
