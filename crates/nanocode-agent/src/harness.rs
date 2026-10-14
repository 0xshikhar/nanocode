//! Harness selection without erasing provider-native builders or transcripts.

use std::{fmt, str::FromStr};

use nanocode_types::{Model, ReasoningMode, Thinking};

use crate::{NanocodexError, ServiceTier};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// Open agent-loop registration identity.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessFamily(nanocode_types::BackendId);
impl HarnessFamily {
    /// Responses backend.
    #[allow(non_upper_case_globals)]
    pub const Codex: Self = Self(nanocode_types::BackendId::Codex);
    /// Messages backend.
    #[allow(non_upper_case_globals)]
    pub const Claude: Self = Self(nanocode_types::BackendId::Claude);
    /// Creates an open backend identity.
    pub fn new(value: impl Into<String>) -> Self {
        Self(nanocode_types::BackendId::new(value))
    }
    /// Stable backend identifier.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
    /// Compatibility default for bundled backends. Registrations supply custom defaults.
    pub fn default_model(&self) -> HarnessModel {
        if *self == Self::Codex {
            HarnessModel::Codex(Model::Sol)
        } else if *self == Self::Claude {
            HarnessModel::Claude(ClaudeModel::Opus55)
        } else {
            HarnessModel::new(self.clone(), "default")
        }
    }
}
impl From<nanocode_types::BackendId> for HarnessFamily {
    fn from(value: nanocode_types::BackendId) -> Self {
        Self(value)
    }
}
impl From<HarnessFamily> for nanocode_types::BackendId {
    fn from(value: HarnessFamily) -> Self {
        value.0
    }
}
impl fmt::Display for HarnessFamily {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl FromStr for HarnessFamily {
    type Err = std::convert::Infallible;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(value))
    }
}

/// Known Claude models available to the shared harness router.
///
/// A concrete Claude builder still accepts provider-native model identifiers;
/// this catalog defines the validated choices exposed by shared routing tools.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ClaudeModel {
    /// Claude Opus 5.5.
    Opus55,
    /// Claude Sonnet 5.5.
    Sonnet55,
    /// Claude Haiku 5.5.
    Haiku55,
    /// Claude Fable 5.1.
    Fable51,
    /// Claude Opus 4.6.
    Opus46,
    /// Claude Sonnet 4.6.
    Sonnet46,
    /// Claude Haiku 4.5, with ordinary inference and no adaptive effort.
    Haiku45,
}

impl ClaudeModel {
    /// Known routing models; availability remains the embedding host's policy.
    pub const ALL: [Self; 7] = [
        Self::Opus55,
        Self::Sonnet55,
        Self::Haiku55,
        Self::Fable51,
        Self::Opus46,
        Self::Sonnet46,
        Self::Haiku45,
    ];

    /// Provider-native model identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opus55 => "claude-opus-5-5",
            Self::Sonnet55 => "claude-sonnet-5-5",
            Self::Haiku55 => "claude-haiku-5-5",
            Self::Fable51 => "claude-fable-5-1",
            Self::Opus46 => "claude-opus-4-6",
            Self::Sonnet46 => "claude-sonnet-4-6",
            Self::Haiku45 => "claude-haiku-4-5",
        }
    }

    /// Default reasoning effort for a new native thread.
    pub fn default_thinking(&self) -> Thinking {
        HarnessModel::Claude(*self)
            .capabilities(ModelTransport::Native)
            .default_thinking()
    }

    /// Whether the native adaptive-thinking adapter supports this effort.
    pub fn supports_thinking(&self, thinking: Thinking) -> bool {
        HarnessModel::Claude(*self)
            .capabilities(ModelTransport::Native)
            .supports_thinking(thinking)
    }
}

impl fmt::Display for ClaudeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for ClaudeModel {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "claude-opus-5-5" | "opus" => Ok(Self::Opus55),
            "claude-sonnet-5-5" | "sonnet" => Ok(Self::Sonnet55),
            "claude-haiku-5-5" | "haiku" => Ok(Self::Haiku55),
            "claude-fable-5-1" | "fable" => Ok(Self::Fable51),
            "claude-opus-4-6" => Ok(Self::Opus46),
            "claude-sonnet-4-6" => Ok(Self::Sonnet46),
            "claude-haiku-4-5" | "claude-haiku-4-5-20251001" => Ok(Self::Haiku45),
            _ => Err(
                "unsupported Claude routing model; use opus, sonnet, fable, haiku or a supported Claude model ID",
            ),
        }
    }
}

/// Open backend-qualified model identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HarnessModel {
    backend: HarnessFamily,
    model: String,
}
impl HarnessModel {
    /// Selects a provider-native model from any backend.
    pub fn new(backend: impl Into<HarnessFamily>, model: impl Into<String>) -> Self {
        Self {
            backend: backend.into(),
            model: model.into(),
        }
    }
    /// Rejects empty registration or provider-native model identities.
    pub fn validate(&self) -> crate::Result<()> {
        if self.backend.as_str().trim().is_empty() || self.model.trim().is_empty() {
            return Err(NanocodexError::InvalidRequest(
                "backend and model identities must not be empty".into(),
            ));
        }
        Ok(())
    }
    /// Compatibility constructor for the Responses catalog.
    #[allow(non_snake_case)]
    pub fn Codex(model: Model) -> Self {
        Self::new(HarnessFamily::Codex, model.as_str())
    }
    /// Compatibility constructor for the Messages catalog.
    #[allow(non_snake_case)]
    pub fn Claude(model: ClaudeModel) -> Self {
        Self::new(HarnessFamily::Claude, model.as_str())
    }
    /// Known Responses catalog entry, if any.
    pub fn as_codex(&self) -> Option<Model> {
        (self.backend == HarnessFamily::Codex)
            .then(|| self.model.parse().ok())
            .flatten()
    }
    /// Known Messages catalog entry, if any.
    pub fn as_claude(&self) -> Option<ClaudeModel> {
        (self.backend == HarnessFamily::Claude)
            .then(|| self.model.parse().ok())
            .flatten()
    }
    /// Backend that owns this model.
    pub fn family(&self) -> HarnessFamily {
        self.backend.clone()
    }
    /// Exact provider-native model identifier.
    pub fn as_str(&self) -> &str {
        &self.model
    }

    /// Default native reasoning policy.
    pub fn default_thinking(&self) -> Thinking {
        self.capabilities(ModelTransport::Native).default_thinking()
    }

    /// Whether the native builder for this model supports the requested effort.
    pub fn supports_thinking(&self, thinking: Thinking) -> bool {
        self.capabilities(ModelTransport::Native)
            .supports_thinking(thinking)
    }

    /// Whether native fast processing is offered by this model: priority
    /// processing on Responses models and fast mode on Claude Opus.
    pub fn supports_fast_mode(&self) -> bool {
        self.capabilities(ModelTransport::Native).fast_mode()
    }

    /// Whether the native builder for this model supports a reasoning mode.
    pub fn supports_reasoning_mode(&self, mode: ReasoningMode) -> bool {
        self.capabilities(ModelTransport::Native)
            .supports_reasoning_mode(mode)
    }

    /// Settings this model accepts on one transport.
    ///
    /// This is the shared capability source for thinking levels, processing
    /// tiers and reasoning modes. Builders, setters, pickers and catalog
    /// projections derive their choices from it; an account's managed catalog
    /// may further narrow availability but never widen it.
    ///
    /// ```
    /// use nanocode_agent::{ClaudeModel, HarnessModel, ModelTransport, Thinking};
    ///
    /// let opus = HarnessModel::Claude(ClaudeModel::Opus46);
    /// let native = opus.capabilities(ModelTransport::Native);
    /// assert!(native.supports_thinking(Thinking::Max));
    /// assert!(!native.supports_thinking(Thinking::Xhigh));
    /// let managed = opus.capabilities(ModelTransport::Managed);
    /// assert!(!managed.supports_thinking(Thinking::Max));
    /// assert!(managed.check_thinking(Thinking::Max).is_err());
    /// ```
    pub fn capabilities(&self, transport: ModelTransport) -> ModelCapabilities {
        use Thinking::{High, Low, Max, Medium, None, Xhigh};
        const fn set(levels: &[Thinking]) -> u8 {
            let mut bits = 0;
            let mut index = 0;
            while index < levels.len() {
                bits |= thinking_bit(levels[index]);
                index += 1;
            }
            bits
        }
        let (thinking, default_thinking, fast_mode, ultrafast, pro) =
            match (self.as_codex(), self.as_claude(), transport) {
                (Some(model), _, _) => {
                    let mut bits = 0;
                    let mut index = 0;
                    while index < Thinking::ALL.len() {
                        if model.supports_thinking(Thinking::ALL[index]) {
                            bits |= thinking_bit(Thinking::ALL[index]);
                        }
                        index += 1;
                    }
                    (
                        bits,
                        model.default_thinking(),
                        matches!(model, Model::Astra | Model::Sol | Model::Luna),
                        // The managed control plane accepts only a priority switch.
                        matches!(transport, ModelTransport::Native)
                            && matches!(model, Model::Astra | Model::Sol),
                        model.supports_reasoning_mode(ReasoningMode::Pro),
                    )
                }
                (_, Some(ClaudeModel::Haiku45), _) => (set(&[None]), None, false, false, false),
                // The native Messages adapter sends each level as output_config.effort.
                (_, Some(model), ModelTransport::Native) => (
                    match model {
                        ClaudeModel::Opus46 | ClaudeModel::Sonnet46 => {
                            set(&[Low, Medium, High, Max])
                        }
                        _ => set(&[Low, Medium, High, Xhigh, Max]),
                    },
                    match model {
                        ClaudeModel::Opus55 | ClaudeModel::Haiku55 => Medium,
                        _ => High,
                    },
                    matches!(model, ClaudeModel::Opus55),
                    false,
                    false,
                ),
                // The managed control plane admits Claude at low, medium or high
                // effort, standard mode and standard speed.
                (_, Some(_), ModelTransport::Managed) => {
                    (set(&[Low, Medium, High]), Medium, false, false, false)
                }
                _ => (set(&Thinking::ALL), Medium, false, false, false),
            };
        ModelCapabilities {
            model: self.clone(),
            transport,
            thinking,
            default_thinking,
            fast_mode,
            ultrafast,
            pro,
        }
    }

    /// Validated model choices within one family.
    pub fn for_family(family: HarnessFamily) -> impl Iterator<Item = Self> {
        Model::ALL
            .into_iter()
            .map(Self::Codex)
            .chain(ClaudeModel::ALL.into_iter().map(Self::Claude))
            .filter(move |model| model.family() == family)
    }
}

/// Transport serving a model; the same model can accept different settings on
/// each.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum ModelTransport {
    /// A provider-native builder in this process (Responses or Messages).
    Native,
    /// The account-managed control plane.
    Managed,
}

impl ModelTransport {
    /// Stable public transport identifier.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Managed => "managed",
        }
    }
}

const fn thinking_bit(thinking: Thinking) -> u8 {
    1 << thinking as u8
}

/// Thinking levels, processing tiers and reasoning modes one model accepts on
/// one transport. Obtain it with [`HarnessModel::capabilities`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelCapabilities {
    model: HarnessModel,
    transport: ModelTransport,
    thinking: u8,
    default_thinking: Thinking,
    fast_mode: bool,
    ultrafast: bool,
    pro: bool,
}

impl ModelCapabilities {
    /// Model these capabilities describe.
    pub fn model(&self) -> HarnessModel {
        self.model.clone()
    }
    /// Transport these capabilities describe.
    pub const fn transport(&self) -> ModelTransport {
        self.transport
    }
    /// Effort selected for a new thread.
    pub const fn default_thinking(&self) -> Thinking {
        self.default_thinking
    }
    /// Whether the model accepts this effort.
    pub const fn supports_thinking(&self, thinking: Thinking) -> bool {
        self.thinking & thinking_bit(thinking) != 0
    }
    /// Accepted efforts in ascending order.
    pub fn thinking(&self) -> impl Iterator<Item = Thinking> + use<> {
        let capabilities = self.clone();
        Thinking::ALL
            .into_iter()
            .filter(move |thinking| capabilities.supports_thinking(*thinking))
    }
    /// Whether fast processing (priority on Responses, fast mode on Claude) is accepted.
    pub const fn fast_mode(&self) -> bool {
        self.fast_mode
    }
    /// Whether this processing tier is accepted; Priority is the
    /// compatibility name of Fast.
    pub const fn supports_service_tier(&self, tier: ServiceTier) -> bool {
        match tier {
            ServiceTier::Standard => true,
            ServiceTier::Priority | ServiceTier::Fast => self.fast_mode,
            ServiceTier::Ultrafast => self.ultrafast,
        }
    }
    /// Accepted processing tiers, slowest first.
    pub fn service_tiers(&self) -> impl Iterator<Item = ServiceTier> + use<> {
        let capabilities = self.clone();
        [
            ServiceTier::Standard,
            ServiceTier::Fast,
            ServiceTier::Ultrafast,
        ]
        .into_iter()
        .filter(move |tier| capabilities.supports_service_tier(*tier))
    }
    /// Whether this reasoning mode is accepted.
    pub const fn supports_reasoning_mode(&self, mode: ReasoningMode) -> bool {
        match mode {
            ReasoningMode::Standard => true,
            ReasoningMode::Pro => self.pro,
        }
    }
    /// Accepted reasoning modes, Standard first.
    pub fn reasoning_modes(&self) -> impl Iterator<Item = ReasoningMode> + use<> {
        let capabilities = self.clone();
        [ReasoningMode::Standard, ReasoningMode::Pro]
            .into_iter()
            .filter(move |mode| capabilities.supports_reasoning_mode(*mode))
    }

    /// Keeps a retained effort the model accepts, otherwise its default.
    /// Use for deliberate model switches, never for explicit requests.
    pub const fn normalize_thinking(&self, thinking: Thinking) -> Thinking {
        if self.supports_thinking(thinking) {
            thinking
        } else {
            self.default_thinking
        }
    }

    /// Keeps a retained tier the model accepts, otherwise Standard.
    pub const fn normalize_service_tier(&self, tier: ServiceTier) -> ServiceTier {
        if self.supports_service_tier(tier) {
            tier
        } else {
            ServiceTier::Standard
        }
    }

    /// Keeps a retained reasoning mode the model accepts, otherwise Standard.
    pub const fn normalize_reasoning_mode(&self, mode: ReasoningMode) -> ReasoningMode {
        if self.supports_reasoning_mode(mode) {
            mode
        } else {
            ReasoningMode::Standard
        }
    }

    /// Rejects an explicitly requested effort the model does not accept.
    ///
    /// # Errors
    ///
    /// Returns [`NanocodexError::InvalidRequest`] naming the supported levels.
    pub fn check_thinking(&self, thinking: Thinking) -> crate::Result<()> {
        if self.supports_thinking(thinking) {
            return Ok(());
        }
        Err(NanocodexError::InvalidRequest(format!(
            "{} requires a supported thinking level{}; supported thinking: {} ({} does not support {thinking} thinking)",
            display_name(&self.model),
            self.transport_suffix(),
            join(self.thinking().map(Thinking::as_str)),
            self.model,
        )))
    }

    /// Rejects an explicitly requested processing tier the model does not accept.
    ///
    /// # Errors
    ///
    /// Returns [`NanocodexError::InvalidRequest`] naming the supported tiers.
    pub fn check_service_tier(&self, tier: ServiceTier) -> crate::Result<()> {
        if self.supports_service_tier(tier) {
            return Ok(());
        }
        Err(self.unsupported(
            &format!("the {} service tier", tier.as_str()),
            &format!(
                "supported tiers: {}",
                join(self.service_tiers().map(ServiceTier::as_str))
            ),
        ))
    }

    /// Rejects an explicitly requested fast mode the model does not accept.
    ///
    /// # Errors
    ///
    /// Returns [`NanocodexError::InvalidRequest`] when fast mode is unavailable.
    pub fn check_fast_mode(&self, enabled: bool) -> crate::Result<()> {
        if !enabled || self.fast_mode {
            return Ok(());
        }
        Err(self.unsupported(
            "fast mode",
            &format!(
                "supported tiers: {}",
                join(self.service_tiers().map(ServiceTier::as_str))
            ),
        ))
    }

    /// Rejects an explicitly requested reasoning mode the model does not accept.
    ///
    /// # Errors
    ///
    /// Returns [`NanocodexError::InvalidRequest`] naming the supported modes.
    pub fn check_reasoning_mode(&self, mode: ReasoningMode) -> crate::Result<()> {
        if self.supports_reasoning_mode(mode) {
            return Ok(());
        }
        Err(self.unsupported(
            &format!("{mode} reasoning mode"),
            &format!(
                "supported modes: {}",
                join(self.reasoning_modes().map(ReasoningMode::as_str))
            ),
        ))
    }

    const fn transport_suffix(&self) -> &'static str {
        match self.transport {
            ModelTransport::Native => "",
            ModelTransport::Managed => " on the managed service",
        }
    }

    fn unsupported(&self, setting: &str, supported: &str) -> NanocodexError {
        NanocodexError::InvalidRequest(format!(
            "{} ({}) does not support {setting}{}; {supported}",
            display_name(&self.model),
            self.model,
            self.transport_suffix(),
        ))
    }
}

/// Human-readable model name used in actionable errors.
fn display_name(model: &HarnessModel) -> &'static str {
    match (model.as_codex(), model.as_claude()) {
        (Some(Model::Sol), _) => "GPT-6.1 Sol",
        (Some(Model::Luna), _) => "GPT-6 Luna",
        (Some(Model::Astra), _) => "GPT-6 Astra",
        (Some(Model::Glm53), _) => "GLM-5.3",
        (Some(Model::Kimi), _) => "Kimi K3",
        (Some(Model::Mimo), _) => "MiMo V2.6 Pro",
        (_, Some(ClaudeModel::Opus55)) => "Claude Opus 5.5",
        (_, Some(ClaudeModel::Sonnet55)) => "Claude Sonnet 5.5",
        (_, Some(ClaudeModel::Haiku55)) => "Claude Haiku 5.5",
        (_, Some(ClaudeModel::Fable51)) => "Claude Fable 5.1",
        (_, Some(ClaudeModel::Opus46)) => "Claude Opus 4.6",
        (_, Some(ClaudeModel::Sonnet46)) => "Claude Sonnet 4.6",
        (_, Some(ClaudeModel::Haiku45)) => "Claude Haiku 4.5",
        _ => "The selected model",
    }
}

fn join<'a>(values: impl Iterator<Item = &'a str>) -> String {
    values.collect::<Vec<_>>().join(", ")
}

impl From<Model> for HarnessModel {
    fn from(model: Model) -> Self {
        Self::Codex(model)
    }
}
impl Default for HarnessModel {
    fn default() -> Self {
        Self::Codex(Model::default())
    }
}
impl From<ClaudeModel> for HarnessModel {
    fn from(model: ClaudeModel) -> Self {
        Self::Claude(model)
    }
}
impl PartialEq<Model> for HarnessModel {
    fn eq(&self, model: &Model) -> bool {
        self == &Self::Codex(*model)
    }
}
impl PartialEq<HarnessModel> for Model {
    fn eq(&self, model: &HarnessModel) -> bool {
        model == self
    }
}
impl fmt::Display for HarnessModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl FromStr for HarnessModel {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value
            .parse::<Model>()
            .map(Self::Codex)
            .or_else(|_| value.parse::<ClaudeModel>().map(Self::Claude))
    }
}

impl Serialize for HarnessModel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self
            .as_codex()
            .is_some_and(|model| model.as_str() == self.as_str())
            || self
                .as_claude()
                .is_some_and(|model| model.as_str() == self.as_str())
        {
            serializer.serialize_str(self.as_str())
        } else {
            nanocode_types::ModelSelection::new(self.backend.clone().into(), self.model.clone())
                .serialize(serializer)
        }
    }
}
impl<'de> Deserialize<'de> for HarnessModel {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Known(String),
            Open(nanocode_types::ModelSelection),
        }
        match Wire::deserialize(deserializer)? {
            Wire::Known(value) => value.parse().map_err(de::Error::custom),
            Wire::Open(value) => {
                let model = Self::new(value.backend, value.model);
                model.validate().map_err(de::Error::custom)?;
                Ok(model)
            }
        }
    }
}

impl From<nanocode_types::ModelSelection> for HarnessModel {
    fn from(value: nanocode_types::ModelSelection) -> Self {
        Self::new(value.backend, value.model)
    }
}
impl From<HarnessModel> for nanocode_types::ModelSelection {
    fn from(value: HarnessModel) -> Self {
        Self::new(value.backend.into(), value.model)
    }
}
