pub mod identity;
pub use identity::{BackendId, ModelSelection};
// Shared SDK input, event, model, and tool wire contracts. No backend clients.
use serde::{Deserialize, Serialize};
use std::{fmt, path::PathBuf, str::FromStr};
pub mod events;
pub mod pricing;
pub mod responses;
pub mod tools;
pub use responses::{ContentItem, FunctionOutputContent, ResponseEvent, ResponseItem};
/// The default Responses model used by this SDK.
pub const MODEL: &str = Model::Astra.as_str();

/// Supported coding models.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Model {
    /// GPT-6.1 Sol.
    Sol,
    /// GPT-6 Luna.
    Luna,
    /// GPT-6 Astra.
    #[default]
    Astra,
    /// Z.ai GLM-5.3 served by Cloudflare Workers AI.
    #[serde(rename = "glm-5.3")]
    Glm53,
    /// Moonshot Kimi K3 through a host-managed gateway.
    #[serde(rename = "kimi-k3", alias = "kimi")]
    Kimi,
    /// Xiaomi MiMo V2.6 Pro through a host-managed gateway.
    #[serde(rename = "mimo-v2.6-pro", alias = "mimo")]
    Mimo,
}

impl Model {
    /// Supported model catalog in picker order.
    pub const ALL: [Self; 3] = [Self::Astra, Self::Sol, Self::Luna];

    /// Default reasoning effort from the pinned Codex model catalog.
    #[must_use]
    pub const fn default_thinking(self) -> Thinking {
        match self {
            Self::Astra | Self::Sol | Self::Glm53 | Self::Kimi | Self::Mimo => Thinking::Low,
            Self::Luna => Thinking::Medium,
        }
    }
    /// Returns the Responses API model identifier.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sol => "gpt-6.1-sol",
            Self::Luna => "gpt-6-luna",
            Self::Astra => "gpt-6-astra",
            Self::Glm53 => "@cf/zai-org/glm-5.3",
            Self::Kimi => "kimi-k3",
            Self::Mimo => "mimo-v2.6-pro",
        }
    }

    /// Returns whether the model accepts the requested reasoning effort.
    #[must_use]
    pub const fn supports_thinking(self, thinking: Thinking) -> bool {
        match self {
            Self::Kimi => matches!(thinking, Thinking::Low | Thinking::High),
            Self::Glm53 | Self::Mimo => {
                matches!(thinking, Thinking::Low | Thinking::Medium | Thinking::High)
            }
            Self::Luna => true,
            Self::Astra | Self::Sol => !matches!(thinking, Thinking::None),
        }
    }

    /// Whether the pinned Codex model catalog supports cache-preserving effort updates.
    #[must_use]
    pub const fn supports_reasoning_effort_updates(self) -> bool {
        matches!(self, Self::Astra | Self::Sol)
    }

    /// Returns whether the model accepts the requested reasoning execution mode.
    #[must_use]
    pub const fn supports_reasoning_mode(self, mode: ReasoningMode) -> bool {
        !matches!(
            (self, mode),
            (Self::Glm53 | Self::Kimi | Self::Mimo, ReasoningMode::Pro)
        )
    }

    /// Largest Codex-compatible prompt context for this model.
    #[must_use]
    pub const fn max_context_window_tokens(self) -> u64 {
        match self {
            Self::Glm53 => 1_310_720,
            Self::Kimi => 1_000_000,
            Self::Mimo => 1_048_576,
            _ => MAX_CONTEXT_WINDOW_TOKENS,
        }
    }
}

impl fmt::Display for Model {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Model {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "gpt-6.1-sol" | "sol" => Ok(Self::Sol),
            "gpt-6-luna" | "luna" => Ok(Self::Luna),
            "gpt-6-astra" | "astra" => Ok(Self::Astra),
            "@cf/zai-org/glm-5.3" | "glm-5.3" | "glm53" => Ok(Self::Glm53),
            "kimi-k3" | "kimi" => Ok(Self::Kimi),
            "mimo-v2.6-pro" | "mimo" => Ok(Self::Mimo),
            _ => Err(format!(
                "invalid model {value:?}; expected gpt-6-astra, gpt-6.1-sol, gpt-6-luna, @cf/zai-org/glm-5.3, kimi-k3, or mimo-v2.6-pro"
            )),
        }
    }
}

/// Default GPT-6 context window used for accounting and automatic compaction.
pub const CONTEXT_WINDOW_TOKENS: u64 = 272_000;
/// Largest Codex-compatible prompt context currently accepted by supported models.
///
/// This intentionally follows the current Codex model catalog rather than the
/// Responses API's larger advertised total context window.
pub const MAX_CONTEXT_WINDOW_TOKENS: u64 = 872_000;

/// User input for one agent turn.
///
/// Session policy such as the filesystem workspace belongs to the agent
/// builder rather than an individual prompt.
///
/// # Examples
///
/// ```
/// use nanocode_types::{ImageDetail, Prompt, UserInput};
///
/// let prompt = Prompt::content([
///     UserInput::Text {
///         text: "Describe the deployment diagram.".to_owned(),
///     },
///     UserInput::Image {
///         image_url: "https://example.com/deployment.png".to_owned(),
///         detail: Some(ImageDetail::High),
///     },
/// ]);
///
/// assert!(!prompt.instruction.is_empty());
/// ```
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Prompt {
    /// Ordered text and multimodal content for this turn.
    pub instruction: PromptInput,
    /// Synthetic text-only conversation supplied before this turn.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    transcript: Vec<PromptMessage>,
    /// Runtime-owned revision; never part of model-visible prompt content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instruction_revision: Option<u64>,
}

impl Prompt {
    /// Creates a text-only prompt.
    #[must_use]
    pub fn new(instruction: impl Into<String>) -> Self {
        Self {
            instruction: PromptInput::Text(instruction.into()),
            transcript: Vec::new(),
            instruction_revision: None,
        }
    }

    /// Creates a prompt from ordered content items.
    #[must_use]
    pub fn content(input: impl IntoIterator<Item = UserInput>) -> Self {
        Self {
            instruction: PromptInput::Content(input.into_iter().collect()),
            transcript: Vec::new(),
            instruction_revision: None,
        }
    }

    /// Attaches the trusted runtime instruction revision to this input.
    #[must_use]
    pub const fn with_instruction_revision(mut self, revision: u64) -> Self {
        self.instruction_revision = Some(revision);
        self
    }

    /// Returns the runtime-owned instruction revision, outside model-visible text.
    #[must_use]
    pub const fn instruction_revision(&self) -> Option<u64> {
        self.instruction_revision
    }

    /// Prepends an explicit text-only conversation to this turn.
    ///
    /// This is intended for synthetic conversation benchmarks. Normal
    /// follow-on turns should continue to rely on the agent's retained
    /// session history.
    #[must_use]
    pub fn with_transcript(mut self, messages: impl IntoIterator<Item = PromptMessage>) -> Self {
        self.transcript = messages.into_iter().collect();
        self
    }

    /// Returns the synthetic conversation preceding this turn.
    #[must_use]
    pub fn transcript(&self) -> &[PromptMessage] {
        &self.transcript
    }

    /// Returns the total UTF-8 byte length of model-visible text.
    #[must_use]
    pub fn text_bytes(&self) -> usize {
        self.transcript
            .iter()
            .map(PromptMessage::text_bytes)
            .sum::<usize>()
            .saturating_add(self.instruction.text_bytes())
    }

    /// Returns whether the turn instruction contains no usable content.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.instruction.is_empty()
    }

    /// Validates the instruction and synthetic transcript invariants.
    ///
    /// # Errors
    ///
    /// Returns a typed failure for empty text or a transcript that does not
    /// start with user input and end with assistant output before the final
    /// user instruction. Consecutive messages with the same role are retained
    /// because benchmark transcripts may contain them intentionally.
    pub fn validate(&self) -> Result<(), PromptValidationError> {
        if self.instruction.is_empty() {
            return Err(PromptValidationError::EmptyInstruction);
        }
        if self.transcript.iter().any(PromptMessage::is_empty) {
            return Err(PromptValidationError::EmptyTranscriptMessage);
        }
        if self
            .transcript
            .first()
            .is_some_and(|message| message.role() != PromptMessageRole::User)
            || self
                .transcript
                .last()
                .is_some_and(|message| message.role() != PromptMessageRole::Assistant)
        {
            return Err(PromptValidationError::InvalidTranscriptEndpoints);
        }
        Ok(())
    }
}

/// Invalid model-visible prompt content.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PromptValidationError {
    /// The final user instruction has no usable content.
    #[error("prompt instruction must not be empty")]
    EmptyInstruction,
    /// One synthetic message has no usable content.
    #[error("prompt transcript messages must not be empty")]
    EmptyTranscriptMessage,
    /// Synthetic messages do not begin with user input and end with assistant output.
    #[error("prompt transcript must start with a user message and end with an assistant message")]
    InvalidTranscriptEndpoints,
}

/// One text-only message in a synthetic prompt transcript.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptMessage {
    role: PromptMessageRole,
    content: String,
}

impl PromptMessage {
    /// Creates a synthetic user message.
    #[must_use]
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: PromptMessageRole::User,
            content: content.into(),
        }
    }

    /// Creates a synthetic assistant message.
    #[must_use]
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: PromptMessageRole::Assistant,
            content: content.into(),
        }
    }

    /// Returns the message role.
    #[must_use]
    pub const fn role(&self) -> PromptMessageRole {
        self.role
    }

    /// Returns the complete message text.
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    const fn text_bytes(&self) -> usize {
        self.content.len()
    }

    fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}

/// Role of one message in a synthetic prompt transcript.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptMessageRole {
    /// User-supplied text.
    User,
    /// Assistant text supplied by the benchmark.
    Assistant,
}

impl From<String> for Prompt {
    fn from(instruction: String) -> Self {
        Self::new(instruction)
    }
}

impl From<&str> for Prompt {
    fn from(instruction: &str) -> Self {
        Self::new(instruction)
    }
}

/// Ordered input for one agent turn.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(untagged)]
pub enum PromptInput {
    /// A text-only instruction.
    Text(String),
    /// Ordered text and multimodal input items.
    Content(Vec<UserInput>),
}

impl PromptInput {
    /// Returns the total UTF-8 byte length of text items.
    #[must_use]
    pub fn text_bytes(&self) -> usize {
        match self {
            Self::Text(text) => text.len(),
            Self::Content(items) => items.iter().map(UserInput::text_bytes).sum(),
        }
    }

    /// Returns the total Unicode scalar-value count of text items.
    #[must_use]
    pub fn text_chars(&self) -> usize {
        match self {
            Self::Text(text) => text.chars().count(),
            Self::Content(items) => items.iter().map(UserInput::text_chars).sum(),
        }
    }

    /// Returns whether this input contains no non-whitespace text or media.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) => text.trim().is_empty(),
            Self::Content(items) => items.is_empty() || items.iter().all(UserInput::is_empty),
        }
    }
}

impl From<String> for PromptInput {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for PromptInput {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}

/// One ordered user-supplied prompt item.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum UserInput {
    /// Model-visible text.
    Text {
        /// Text supplied by the user.
        text: String,
    },
    /// An image supplied as a URL or data URL.
    Image {
        /// Image URL visible to the model.
        image_url: String,
        /// Optional image-detail policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// Image addressed by opaque provider file identity.
    #[serde(rename = "image")]
    ImageFile {
        /// Nonempty bounded ASCII provider file ID, never a URL or local path.
        #[serde(serialize_with = "serialize_image_file_id")]
        file_id: String,
        /// Optional provider image-detail hint.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// An image loaded from the local filesystem by a native runtime.
    LocalImage {
        /// Path to the local image.
        path: PathBuf,
        /// Optional image-detail policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// A reserved remote audio input.
    Audio {
        /// Audio URL retained by the input contract.
        audio_url: String,
    },
    /// A reserved local audio input.
    LocalAudio {
        /// Path retained by the input contract.
        path: PathBuf,
    },
    /// A document supplied inline as a base64 data URL, such as a PDF.
    ///
    /// Native Claude sends it as a document content block. Responses-backed
    /// harnesses do not consume inline documents and receive a text notice.
    File {
        /// `data:<media type>;base64,<bytes>` document payload.
        file_data: String,
        /// Optional user-visible file name, never a filesystem path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
}

/// Image fidelity requested from the model.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageDetail {
    /// Lets the provider select the detail level.
    Auto,
    /// Requests lower-resolution image processing.
    Low,
    /// Requests high-resolution image processing.
    High,
    /// Requests original-resolution image processing.
    Original,
}

impl UserInput {
    /// Returns the UTF-8 byte length when this is text, or zero for media.
    #[must_use]
    pub const fn text_bytes(&self) -> usize {
        match self {
            Self::Text { text } => text.len(),
            Self::Image { .. }
            | Self::ImageFile { .. }
            | Self::LocalImage { .. }
            | Self::Audio { .. }
            | Self::LocalAudio { .. }
            | Self::File { .. } => 0,
        }
    }

    /// Returns the Unicode scalar-value count when this is text, or zero for media.
    #[must_use]
    pub fn text_chars(&self) -> usize {
        match self {
            Self::Text { text } => text.chars().count(),
            Self::Image { .. }
            | Self::ImageFile { .. }
            | Self::LocalImage { .. }
            | Self::Audio { .. }
            | Self::LocalAudio { .. }
            | Self::File { .. } => 0,
        }
    }

    /// Returns whether this item contains neither non-whitespace text nor media.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text { text } => text.trim().is_empty(),
            Self::Image { .. }
            | Self::ImageFile { .. }
            | Self::LocalImage { .. }
            | Self::Audio { .. }
            | Self::LocalAudio { .. }
            | Self::File { .. } => false,
        }
    }
}

/// Responses reasoning execution mode for the supported model family.
///
/// Standard mode preserves the default request behavior. Pro mode performs
/// additional model work before returning one final answer and can increase
/// latency and token usage independently of [`Thinking`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ReasoningMode {
    /// Standard reasoning behavior.
    #[default]
    Standard,
    /// Pro reasoning behavior.
    Pro,
}

impl ReasoningMode {
    /// Returns the request value used by the Responses API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Pro => "pro",
        }
    }

    #[doc(hidden)]
    pub const fn request_value(self) -> Option<&'static str> {
        match self {
            Self::Standard => None,
            Self::Pro => Some("pro"),
        }
    }
}

impl fmt::Display for ReasoningMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for ReasoningMode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "standard" => Ok(Self::Standard),
            "pro" => Ok(Self::Pro),
            _ => Err(format!(
                "invalid reasoning mode {value:?}; expected standard or pro"
            )),
        }
    }
}

/// Requested model reasoning effort.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Thinking {
    /// Disable reasoning when supported.
    None,
    /// Low reasoning effort.
    #[default]
    Low,
    /// Medium reasoning effort.
    Medium,
    /// High reasoning effort.
    High,
    /// Extra-high reasoning effort.
    Xhigh,
    /// Maximum reasoning effort.
    Max,
}

impl Thinking {
    /// Supported effort values; filter through Model::supports_thinking.
    pub const ALL: [Self; 6] = [
        Self::None,
        Self::Low,
        Self::Medium,
        Self::High,
        Self::Xhigh,
        Self::Max,
    ];

    /// Returns the request value used by the Responses API.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        }
    }
}

impl fmt::Display for Thinking {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for Thinking {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "none" => Ok(Self::None),
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "xhigh" => Ok(Self::Xhigh),
            "max" => Ok(Self::Max),
            _ => Err(format!(
                "invalid reasoning effort {value:?}; expected none, low, medium, high, xhigh, or max"
            )),
        }
    }
}

#[derive(Deserialize)]
#[serde(
    remote = "UserInput",
    tag = "type",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum UserInputFields {
    /// Model-visible text.
    Text {
        /// Text supplied by the user.
        text: String,
    },
    /// An image supplied as a URL or data URL.
    Image {
        /// Image URL visible to the model.
        image_url: String,
        /// Optional image-detail policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// An image loaded from the local filesystem by a native runtime.
    LocalImage {
        /// Path to the local image.
        path: PathBuf,
        /// Optional image-detail policy.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<ImageDetail>,
    },
    /// A reserved remote audio input.
    Audio {
        /// Audio URL retained by the input contract.
        audio_url: String,
    },
    /// A reserved local audio input.
    LocalAudio {
        /// Path retained by the input contract.
        path: PathBuf,
    },
    /// A document supplied inline as a base64 data URL.
    File {
        /// `data:<media type>;base64,<bytes>` document payload.
        file_data: String,
        /// Optional user-visible file name.
        #[serde(default)]
        filename: Option<String>,
    },
}

fn serialize_image_file_id<S: serde::Serializer>(
    file_id: &str,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if !crate::responses::valid_image_file_id(file_id) {
        return Err(serde::ser::Error::custom("invalid image file_id"));
    }
    serializer.serialize_str(file_id)
}
impl<'de> Deserialize<'de> for UserInput {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("type").and_then(serde_json::Value::as_str) == Some("image") {
            let serde_json::Value::Object(mut object) = value else {
                return Err(serde::de::Error::custom("expected image object"));
            };
            object.remove("type");
            let detail = object.remove("detail");
            let reference: crate::responses::ImageReference =
                serde_json::from_value(serde_json::Value::Object(object))
                    .map_err(serde::de::Error::custom)?;
            let detail: Option<ImageDetail> =
                serde_json::from_value(detail.unwrap_or(serde_json::Value::Null))
                    .map_err(serde::de::Error::custom)?;
            return Ok(match reference {
                crate::responses::ImageReference::Inline { image_url } => {
                    Self::Image { image_url, detail }
                }
                crate::responses::ImageReference::File { file_id } => {
                    Self::ImageFile { file_id, detail }
                }
            });
        }
        UserInputFields::deserialize(value).map_err(serde::de::Error::custom)
    }
}
