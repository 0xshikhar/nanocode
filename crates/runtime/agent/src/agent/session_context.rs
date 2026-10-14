use nanocode_types::responses::ResponseItem;

#[derive(Clone, Debug)]
pub struct AgentSessionContext {
    workspace: String,
    history: Vec<ResponseItem>,
}

impl AgentSessionContext {
    #[doc(hidden)]
    #[must_use]
    pub const fn from_backend(workspace: String, history: Vec<ResponseItem>) -> Self {
        Self { workspace, history }
    }

    #[must_use]
    pub fn workspace(&self) -> &str {
        &self.workspace
    }

    #[must_use]
    pub fn history(&self) -> &[ResponseItem] {
        &self.history
    }
}
