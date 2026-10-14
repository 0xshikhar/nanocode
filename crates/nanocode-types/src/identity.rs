//! Open backend and provider model identities.
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// Stable backend registration key. Third parties own their namespace.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BackendId(Cow<'static, str>);

impl BackendId {
    /// Responses agent loop identifier.
    #[allow(non_upper_case_globals)]
    pub const Codex: Self = Self(Cow::Borrowed("codex"));
    /// Messages agent loop identifier.
    #[allow(non_upper_case_globals)]
    pub const Claude: Self = Self(Cow::Borrowed("claude"));
    /// Creates an owned backend identifier without a built-in catalog gate.
    pub fn new(value: impl Into<String>) -> Self {
        Self(Cow::Owned(value.into()))
    }
    /// Stable serialized identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BackendId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Backend-qualified provider-native model selection.
/// Model validation belongs to the registered backend, not this wire type.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub struct ModelSelection {
    /// Stable backend registration key.
    pub backend: BackendId,
    /// Provider-native model identifier.
    pub model: String,
}

impl ModelSelection {
    /// Selects a model from any registered backend.
    pub fn new(backend: BackendId, model: impl Into<String>) -> Self {
        Self {
            backend,
            model: model.into(),
        }
    }
}

impl std::str::FromStr for BackendId {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(value))
    }
}
