mod decision;
#[cfg(not(target_family = "wasm"))]
pub mod openai;
mod request;
mod tool;

use std::sync::Arc;

use async_trait::async_trait;

pub use decision::{Answer, ChoiceProbability, Decision, DecisionUsage, LevelProbability, Outcome};
pub use request::{
    ChoiceOption, ChoiceValue, DecisionInput, DecisionRequest, ImageDetail, InputPart, Question,
    QuestionKind, ScoreLevel,
};
pub use tool::DecisionTool;

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
pub trait DecisionModel: Send + Sync {
    async fn decide(&self, request: &DecisionRequest) -> Result<Decision, DecisionError>;
}

#[cfg_attr(not(target_family = "wasm"), async_trait)]
#[cfg_attr(target_family = "wasm", async_trait(?Send))]
impl<T: DecisionModel + ?Sized> DecisionModel for Arc<T> {
    async fn decide(&self, request: &DecisionRequest) -> Result<Decision, DecisionError> {
        (**self).decide(request).await
    }
}

#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DecisionError {
    #[error("invalid decision request: {0}")]
    InvalidRequest(String),
    #[error("decision API returned HTTP {status}: {message}")]
    Api { status: u16, message: String },
    #[error("decision request failed: {0}")]
    Transport(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("invalid decision response: {0}")]
    InvalidResponse(String),
}
