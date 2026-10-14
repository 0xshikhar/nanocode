use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, VecDeque},
    fmt,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    time::{Instant, timeout},
};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Error as WebSocketError, Message,
        client::IntoClientRequest,
        http::{HeaderValue, header},
        protocol::frame::coding::CloseCode,
    },
};
use tracing::{debug, trace, warn};
use url::Url;

use crate::client::{
    OpenAiAuth, OpenAiAuthError, OpenAiAuthMode, connector::connect_async, responses::MessagePhase,
};

mod webrtc;
pub use webrtc::RealtimeMediaPeer;

pub const REALTIME_SAMPLE_RATE: u32 = 24_000;

pub const REALTIME_CHANNELS: u16 = 1;

pub const REALTIME_MODEL: &str = "gpt-realtime-1.5";

pub const CHATGPT_REALTIME_MODEL: &str = "gpt-live-1-codex";

pub const CHATGPT_REALTIME_VOICES: &[RealtimeVoice] = &[
    RealtimeVoice::Juniper,
    RealtimeVoice::Maple,
    RealtimeVoice::Spruce,
    RealtimeVoice::Ember,
    RealtimeVoice::Vale,
    RealtimeVoice::Breeze,
    RealtimeVoice::Arbor,
    RealtimeVoice::Sol,
    RealtimeVoice::Cove,
];

pub const CHATGPT_REALTIME_VOICE: RealtimeVoice = RealtimeVoice::Cove;

pub const PLATFORM_REALTIME_VOICES: &[RealtimeVoice] = &[
    RealtimeVoice::Alloy,
    RealtimeVoice::Ash,
    RealtimeVoice::Ballad,
    RealtimeVoice::Coral,
    RealtimeVoice::Echo,
    RealtimeVoice::Sage,
    RealtimeVoice::Shimmer,
    RealtimeVoice::Verse,
    RealtimeVoice::Marin,
    RealtimeVoice::Cedar,
];

pub const PLATFORM_REALTIME_VOICE: RealtimeVoice = RealtimeVoice::Marin;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
const COMMAND_CAPACITY: usize = 256;
const EVENT_CAPACITY: usize = 256;
const BACKGROUND_AGENT_TOOL: &str = "background_agent";
const BACKGROUND_AGENT_TOOL_DESCRIPTION: &str = "Send a user request to the background agent. Use this as the default action. Do not rephrase the user's ask or rewrite it in your own words; pass along the user's own words. If the background agent is idle, this starts a new task and returns the final result to the user. If the background agent is already working on a task, this sends the request as guidance to steer that previous task. If the user asks to do something next, later, after this, or once current work finishes, call this tool so the work is actually queued instead of merely promising to do it later.";
const REMAIN_SILENT_TOOL: &str = "remain_silent";
const REMAIN_SILENT_TOOL_DESCRIPTION: &str = "Call this when the best response is to say nothing. Use it instead of speaking after hidden system/control messages, after background agent updates in silent modes, or whenever acknowledging aloud would be distracting. This tool has no user-visible effect.";
const STEER_ACKNOWLEDGEMENT: &str = "This was sent to steer the previous background agent task.";
const AGENT_COMPLETE_ACKNOWLEDGEMENT: &str =
    "Background agent finished. Use the preceding [BACKEND] messages as the result.";
const BACKEND_TEXT_PREFIX: &str = "[BACKEND] ";
const CONTEXT_APPEND_MAX_BYTES: usize = 500;
const INITIAL_ITEMS_MAX_COUNT: usize = 128;
const INITIAL_ITEMS_MAX_TOKENS: usize = 8_192;
const APPROX_BYTES_PER_TOKEN: usize = 4;
const REALTIME_ASSISTANT_OUTPUT_TOKEN_BUDGET: usize = 1_000;
const AGENT_FINAL_MESSAGE_PREFIX: &str = "\"Agent Final Message\":\n\n";
const STANDALONE_HANDOFF_ID: &str = "codex";
const MAX_ACTIVE_TRANSCRIPT_BYTES: usize = 8 * 1024;
const TRUNCATED_TRANSCRIPT_PREFIX: &str = "…";
const SIDEBAND_RECONNECT_BASE_DELAY: Duration = Duration::from_millis(200);
const SIDEBAND_RECONNECT_MAX_DELAY: Duration = Duration::from_secs(5);
const SIDEBAND_STABLE_CONNECTION_DURATION: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RealtimeVersion {
    V1,

    #[default]
    V2,

    V3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeTransport {
    WebSocket,

    WebRtc,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RealtimeSessionMode {
    #[default]
    Conversational,

    Transcription,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RealtimeOutputModality {
    Text,

    #[default]
    Audio,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RealtimeResponseHandoffMode {
    #[default]
    Thinking,

    Commentary,

    BemTags,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum RealtimeInputTextRole {
    #[default]
    User,

    Developer,

    Assistant,
}

impl RealtimeInputTextRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Developer => "developer",
            Self::Assistant => "assistant",
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Assistant => "output_text",
            Self::User | Self::Developer => "input_text",
        }
    }
}

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum RealtimeVoice {
    Alloy,

    Arbor,

    Ash,

    Ballad,

    Breeze,

    Cedar,

    Coral,

    Cove,

    Echo,

    Ember,

    Juniper,

    Maple,

    #[default]
    Marin,

    Sage,

    Shimmer,

    Sol,

    Spruce,

    Vale,

    Verse,
}

impl RealtimeVoice {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alloy => "alloy",
            Self::Arbor => "arbor",
            Self::Ash => "ash",
            Self::Ballad => "ballad",
            Self::Breeze => "breeze",
            Self::Cedar => "cedar",
            Self::Coral => "coral",
            Self::Cove => "cove",
            Self::Echo => "echo",
            Self::Ember => "ember",
            Self::Juniper => "juniper",
            Self::Maple => "maple",
            Self::Marin => "marin",
            Self::Sage => "sage",
            Self::Shimmer => "shimmer",
            Self::Sol => "sol",
            Self::Spruce => "spruce",
            Self::Vale => "vale",
            Self::Verse => "verse",
        }
    }

    const fn supports_frameless(self) -> bool {
        matches!(
            self,
            Self::Arbor
                | Self::Breeze
                | Self::Cove
                | Self::Ember
                | Self::Juniper
                | Self::Maple
                | Self::Sol
                | Self::Spruce
                | Self::Vale
        )
    }

    const fn supports_direct(self) -> bool {
        matches!(
            self,
            Self::Alloy
                | Self::Ash
                | Self::Ballad
                | Self::Cedar
                | Self::Coral
                | Self::Echo
                | Self::Marin
                | Self::Sage
                | Self::Shimmer
                | Self::Verse
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum RealtimeTextRole {
    Developer,

    User,

    Assistant,
}

impl RealtimeTextRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Developer => "developer",
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    const fn content_type(self) -> &'static str {
        match self {
            Self::Developer | Self::User => "input_text",
            Self::Assistant => "output_text",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeInitialItem {
    pub role: RealtimeTextRole,

    pub text: String,
}

impl RealtimeInitialItem {
    #[must_use]
    pub fn new(role: RealtimeTextRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

impl fmt::Display for RealtimeVoice {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RealtimeVoice {
    type Err = RealtimeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "alloy" => Ok(Self::Alloy),
            "arbor" => Ok(Self::Arbor),
            "ash" => Ok(Self::Ash),
            "ballad" => Ok(Self::Ballad),
            "breeze" => Ok(Self::Breeze),
            "cedar" => Ok(Self::Cedar),
            "coral" => Ok(Self::Coral),
            "cove" => Ok(Self::Cove),
            "echo" => Ok(Self::Echo),
            "ember" => Ok(Self::Ember),
            "juniper" => Ok(Self::Juniper),
            "maple" => Ok(Self::Maple),
            "marin" => Ok(Self::Marin),
            "sage" => Ok(Self::Sage),
            "shimmer" => Ok(Self::Shimmer),
            "sol" => Ok(Self::Sol),
            "spruce" => Ok(Self::Spruce),
            "vale" => Ok(Self::Vale),
            "verse" => Ok(Self::Verse),
            _ => Err(RealtimeError::InvalidVoice(value.to_owned())),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeAudio {
    data: Vec<u8>,
}

impl RealtimeAudio {
    pub fn pcm16_le(data: impl Into<Vec<u8>>) -> Result<Self, RealtimeError> {
        let data = data.into();
        if data.len() % size_of::<i16>() != 0 {
            return Err(RealtimeError::InvalidAudio(
                "PCM16 audio must contain complete little-endian samples".to_owned(),
            ));
        }
        Ok(Self { data })
    }

    #[must_use]
    pub fn from_samples(samples: impl IntoIterator<Item = i16>) -> Self {
        let samples = samples.into_iter();
        let mut data = Vec::with_capacity(samples.size_hint().0.saturating_mul(2));
        for sample in samples {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        Self { data }
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.data
    }

    #[must_use]
    pub const fn samples(&self) -> usize {
        self.data.len() / size_of::<i16>()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RealtimeEvent {
    SessionReady {
        session_id: String,
    },

    SpeechStarted,

    InputTranscriptDelta(String),

    InputTranscriptDone(String),

    OutputTranscriptDelta(String),

    OutputTranscriptDone(String),

    Audio(RealtimeAudio),

    AgentRequest {
        call_id: String,

        prompt: String,

        transcript: Vec<RealtimeTranscriptEntry>,
    },

    RemainSilent {
        call_id: String,
    },

    ResponseStarted,

    ResponseDone,

    TranscriptTail(Vec<RealtimeTranscriptEntry>),

    Error(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RealtimeTranscriptEntry {
    pub role: String,

    pub text: String,
}

pub struct RealtimeEvents {
    receiver: mpsc::Receiver<RealtimeEvent>,
}

impl RealtimeEvents {
    pub async fn recv(&mut self) -> Option<RealtimeEvent> {
        self.receiver.recv().await
    }

    pub fn try_recv(&mut self) -> Option<RealtimeEvent> {
        self.receiver.try_recv().ok()
    }
}

pub struct RealtimeSdpConnection {
    sdp: String,
    session: RealtimeSession,
    events: RealtimeEvents,
}

impl RealtimeSdpConnection {
    #[must_use]
    pub fn sdp(&self) -> &str {
        &self.sdp
    }

    #[must_use]
    pub fn into_parts(self) -> (String, RealtimeSession, RealtimeEvents) {
        (self.sdp, self.session, self.events)
    }
}

#[derive(Clone)]
pub struct RealtimeSession {
    commands: mpsc::Sender<Command>,
    protocol: RealtimeProtocol,
    client_managed_handoffs: bool,
    closed: watch::Receiver<bool>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RealtimeAgentSteer {
    Acknowledged,

    ReplacedDelegation,
}

impl RealtimeSession {
    #[must_use]
    pub const fn streams_agent_output(&self) -> bool {
        matches!(
            self.protocol,
            RealtimeProtocol::V1 | RealtimeProtocol::Frameless
        )
    }

    #[must_use]
    pub const fn client_managed_handoffs(&self) -> bool {
        self.client_managed_handoffs
    }

    pub async fn send_audio(&self, audio: RealtimeAudio) -> Result<(), RealtimeError> {
        self.send(CommandKind::Audio(audio)).await.map(|_| ())
    }

    pub async fn send_text(
        &self,
        role: RealtimeInputTextRole,
        text: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.enqueue(CommandKind::Text {
            role,
            text: text.into(),
        })
        .await
    }

    pub async fn append_speech(&self, text: impl Into<String>) -> Result<(), RealtimeError> {
        let text = text.into();
        if text.trim().is_empty() {
            return Ok(());
        }
        self.enqueue(CommandKind::Speech { text }).await
    }

    pub async fn complete_agent_request(
        &self,
        call_id: impl Into<String>,
        output: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.send(CommandKind::AgentOutput {
            call_id: call_id.into(),
            output: output.into(),
        })
        .await
        .map(|_| ())
    }

    pub async fn steer_agent_request(
        &self,
        call_id: impl Into<String>,
    ) -> Result<RealtimeAgentSteer, RealtimeError> {
        match self.protocol {
            RealtimeProtocol::Direct => {
                self.complete_agent_request(call_id, STEER_ACKNOWLEDGEMENT)
                    .await?;
                Ok(RealtimeAgentSteer::Acknowledged)
            }
            RealtimeProtocol::V1 | RealtimeProtocol::Frameless => {
                drop(call_id.into());
                Ok(RealtimeAgentSteer::ReplacedDelegation)
            }
        }
    }

    pub async fn append_agent_output(
        &self,
        call_id: impl Into<String>,
        output: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.append_agent_output_with_phase(call_id, output, None)
            .await
    }

    pub async fn append_agent_output_with_phase(
        &self,
        call_id: impl Into<String>,
        output: impl Into<String>,
        phase: Option<MessagePhase>,
    ) -> Result<(), RealtimeError> {
        self.send(CommandKind::AgentProgress {
            call_id: call_id.into(),
            output: output.into(),
            phase,
        })
        .await
        .map(|_| ())
    }

    pub async fn append_standalone_agent_output(
        &self,
        output: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.append_standalone_agent_output_with_phase(output, None)
            .await
    }

    pub async fn append_standalone_agent_output_with_phase(
        &self,
        output: impl Into<String>,
        phase: Option<MessagePhase>,
    ) -> Result<(), RealtimeError> {
        self.send(CommandKind::StandaloneAgentOutput {
            output: output.into(),
            phase,
        })
        .await
        .map(|_| ())
    }

    pub async fn complete_agent_run(
        &self,
        call_id: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.send(CommandKind::AgentComplete {
            call_id: call_id.into(),
        })
        .await
        .map(|_| ())
    }

    pub async fn complete_silent_request(
        &self,
        call_id: impl Into<String>,
    ) -> Result<(), RealtimeError> {
        self.send(CommandKind::SilentOutput {
            call_id: call_id.into(),
        })
        .await
        .map(|_| ())
    }

    pub async fn close(&self) -> Result<(), RealtimeError> {
        self.close_with_transcript_tail().await.map(|_| ())
    }

    pub async fn close_with_transcript_tail(
        &self,
    ) -> Result<Vec<RealtimeTranscriptEntry>, RealtimeError> {
        let tail = match self.send(CommandKind::Close).await? {
            CommandOutcome::Closed(tail) => tail,
            CommandOutcome::Continue => Vec::new(),
        };
        let mut closed = self.closed.clone();
        if !*closed.borrow() {
            closed.changed().await.map_err(|_| RealtimeError::Closed)?;
        }
        Ok(tail)
    }

    async fn send(&self, kind: CommandKind) -> Result<CommandOutcome, RealtimeError> {
        let (result, completed) = oneshot::channel();
        let command = Command { kind, result };
        timeout(SEND_TIMEOUT, self.commands.send(command))
            .await
            .map_err(|_| RealtimeError::SendTimeout)?
            .map_err(|_| RealtimeError::Closed)?;
        timeout(SEND_TIMEOUT, completed)
            .await
            .map_err(|_| RealtimeError::SendTimeout)?
            .map_err(|_| RealtimeError::Closed)?
    }

    async fn enqueue(&self, kind: CommandKind) -> Result<(), RealtimeError> {
        let (result, _completed) = oneshot::channel();
        timeout(SEND_TIMEOUT, self.commands.send(Command { kind, result }))
            .await
            .map_err(|_| RealtimeError::SendTimeout)?
            .map_err(|_| RealtimeError::Closed)
    }
}

#[derive(Clone)]
pub struct RealtimeSessionBuilder {
    auth: OpenAiAuth,
    api_base_url: String,
    attestation_header: Option<Arc<str>>,
    websocket_url: Option<String>,
    instructions: Arc<str>,
    model: Option<String>,
    voice: Option<RealtimeVoice>,
    session_id: Option<String>,
    initial_items: Vec<RealtimeInitialItem>,
    version: Option<RealtimeVersion>,
    transport: Option<RealtimeTransport>,
    session_mode: RealtimeSessionMode,
    output_modality: RealtimeOutputModality,
    client_managed_handoffs: bool,
    delegation_ack_filler: Option<bool>,
    codex_responses_as_items: bool,
    codex_response_item_prefix: Option<String>,
    codex_response_handoff_mode: RealtimeResponseHandoffMode,
    codex_response_handoff_channel_prefixes: BTreeMap<String, Vec<String>>,
}

impl RealtimeSessionBuilder {
    pub(crate) const fn new(
        auth: OpenAiAuth,
        api_base_url: String,
        instructions: Arc<str>,
    ) -> Self {
        Self {
            auth,
            api_base_url,
            attestation_header: None,
            websocket_url: None,
            instructions,
            model: None,
            voice: None,
            session_id: None,
            initial_items: Vec::new(),
            version: None,
            transport: None,
            session_mode: RealtimeSessionMode::Conversational,
            output_modality: RealtimeOutputModality::Audio,
            client_managed_handoffs: false,
            delegation_ack_filler: None,
            codex_responses_as_items: false,
            codex_response_item_prefix: None,
            codex_response_handoff_mode: RealtimeResponseHandoffMode::Thinking,
            codex_response_handoff_channel_prefixes: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    #[must_use]
    pub const fn version(mut self, version: RealtimeVersion) -> Self {
        self.version = Some(version);
        self
    }

    #[must_use]
    pub const fn transport(mut self, transport: RealtimeTransport) -> Self {
        self.transport = Some(transport);
        self
    }

    #[must_use]
    pub const fn session_mode(mut self, mode: RealtimeSessionMode) -> Self {
        self.session_mode = mode;
        self
    }

    #[must_use]
    pub const fn output_modality(mut self, modality: RealtimeOutputModality) -> Self {
        self.output_modality = modality;
        self
    }

    #[must_use]
    pub const fn client_managed_handoffs(mut self, managed: bool) -> Self {
        self.client_managed_handoffs = managed;
        self
    }

    #[must_use]
    pub const fn delegation_ack_filler(mut self, enabled: bool) -> Self {
        self.delegation_ack_filler = Some(enabled);
        self
    }

    #[must_use]
    pub const fn codex_responses_as_items(mut self, as_items: bool) -> Self {
        self.codex_responses_as_items = as_items;
        self
    }

    #[must_use]
    pub fn codex_response_item_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.codex_response_item_prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub const fn codex_response_handoff_mode(mut self, mode: RealtimeResponseHandoffMode) -> Self {
        self.codex_response_handoff_mode = mode;
        self
    }

    #[must_use]
    pub fn codex_response_handoff_channel_prefixes(
        mut self,
        prefixes: BTreeMap<String, Vec<String>>,
    ) -> Self {
        self.codex_response_handoff_channel_prefixes = prefixes;
        self
    }

    #[must_use]
    pub const fn voice(mut self, voice: RealtimeVoice) -> Self {
        self.voice = Some(voice);
        self
    }

    #[must_use]
    pub fn websocket_url(mut self, websocket_url: impl Into<String>) -> Self {
        self.websocket_url = Some(websocket_url.into());
        self
    }

    #[must_use]
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    #[must_use]
    pub fn initial_items(mut self, items: impl IntoIterator<Item = RealtimeInitialItem>) -> Self {
        self.initial_items = items.into_iter().collect();
        self
    }

    #[must_use]
    pub fn initial_item(mut self, role: RealtimeTextRole, text: impl Into<String>) -> Self {
        self.initial_items
            .push(RealtimeInitialItem::new(role, text));
        self
    }

    #[must_use]
    pub fn attestation_header(mut self, value: impl Into<Arc<str>>) -> Self {
        self.attestation_header = Some(value.into());
        self
    }

    pub async fn connect(self) -> Result<(RealtimeSession, RealtimeEvents), RealtimeError> {
        validate_session_builder(&self)?;
        let version = self.version.unwrap_or(match self.auth.mode() {
            OpenAiAuthMode::ApiKey => RealtimeVersion::V2,
            OpenAiAuthMode::ChatGpt => RealtimeVersion::V3,
        });
        let protocol = match version {
            RealtimeVersion::V1 => RealtimeProtocol::V1,
            RealtimeVersion::V2 => RealtimeProtocol::Direct,
            RealtimeVersion::V3 => RealtimeProtocol::Frameless,
        };
        let transport = self.transport.unwrap_or(if version == RealtimeVersion::V3 {
            RealtimeTransport::WebRtc
        } else {
            RealtimeTransport::WebSocket
        });
        let model = self.model.clone().unwrap_or_else(|| match version {
            RealtimeVersion::V1 | RealtimeVersion::V2 => REALTIME_MODEL.to_owned(),
            RealtimeVersion::V3 => CHATGPT_REALTIME_MODEL.to_owned(),
        });
        validate_realtime_configuration(
            version,
            transport,
            self.session_mode,
            self.output_modality,
            &self.initial_items,
        )?;
        validate_initial_items(version, &self.initial_items)?;
        let voice = self.voice.unwrap_or(match version {
            RealtimeVersion::V1 | RealtimeVersion::V3 => CHATGPT_REALTIME_VOICE,
            RealtimeVersion::V2 => PLATFORM_REALTIME_VOICE,
        });
        validate_voice(version, voice)?;

        let output_policy = OutputPolicy {
            codex_responses_as_items: self.codex_responses_as_items,
            codex_response_item_prefix: self.codex_response_item_prefix,
            handoff_mode: self.codex_response_handoff_mode,
            channel_prefixes: self.codex_response_handoff_channel_prefixes,
        };
        let (socket, media, sideband, initial_event) = match transport {
            RealtimeTransport::WebSocket => {
                let auth = self.auth.snapshot().await?;
                let endpoint = match self.websocket_url {
                    Some(endpoint) => endpoint,
                    None => realtime_endpoint(&self.api_base_url, &model, version)?,
                };
                let mut request = endpoint
                    .as_str()
                    .into_client_request()
                    .map_err(|error| RealtimeError::InvalidUrl(error.to_string()))?;
                webrtc::add_auth_headers(request.headers_mut(), &auth)?;
                match version {
                    RealtimeVersion::V1 => {
                        request
                            .headers_mut()
                            .insert("openai-alpha", HeaderValue::from_static("quicksilver=v1"));
                    }
                    RealtimeVersion::V3 => {
                        request
                            .headers_mut()
                            .insert("openai-alpha", HeaderValue::from_static("quicksilver=v2"));
                    }
                    RealtimeVersion::V2 => {}
                }
                request.headers_mut().insert(
                    header::USER_AGENT,
                    HeaderValue::from_static(concat!("nanocode/", env!("CARGO_PKG_VERSION"))),
                );
                if let Some(session_id) = &self.session_id {
                    request.headers_mut().insert(
                        "x-session-id",
                        HeaderValue::from_str(session_id)
                            .map_err(|error| RealtimeError::InvalidSessionId(error.to_string()))?,
                    );
                }

                let connect_started = Instant::now();
                let (mut socket, response) = timeout(CONNECT_TIMEOUT, connect_async(request))
                    .await
                    .map_err(|_| RealtimeError::ConnectTimeout)?
                    .map_err(map_websocket_error)?;
                debug!(
                    status = response.status().as_u16(),
                    elapsed_ms = connect_started.elapsed().as_millis(),
                    "connected GPT Realtime websocket"
                );
                let update = configured_session_update(
                    &self.instructions,
                    &model,
                    voice,
                    version,
                    self.session_mode,
                    self.output_modality,
                    &self.initial_items,
                    self.delegation_ack_filler,
                );
                send_json(&mut socket, &update).await?;
                let initial_event = if version == RealtimeVersion::V3 {
                    Some(wait_for_frameless_session_started(&mut socket).await?)
                } else {
                    None
                };
                (socket, None, None, initial_event)
            }
            RealtimeTransport::WebRtc => {
                let connection = webrtc::connect(webrtc::ConnectConfig {
                    auth: &self.auth,
                    api_base_url: &self.api_base_url,
                    attestation_header: self.attestation_header.as_deref(),
                    websocket_url: self.websocket_url.as_deref(),
                    instructions: &self.instructions,
                    model: &model,
                    voice,
                    session_id: self.session_id.as_deref(),
                    initial_items: &self.initial_items,
                    delegation_ack_filler: self.delegation_ack_filler,
                    version,
                })
                .await?;
                (
                    connection.socket,
                    Some(connection.media),
                    Some(connection.sideband),
                    None,
                )
            }
        };
        Ok(spawn_connected_session(
            socket,
            protocol,
            media,
            sideband,
            output_policy,
            self.client_managed_handoffs,
            SessionOwnership::Owned,
            initial_event,
        ))
    }

    pub async fn connect_with_sdp(
        self,
        offer_sdp: impl Into<String>,
    ) -> Result<RealtimeSdpConnection, RealtimeError> {
        validate_session_builder(&self)?;
        if self.transport.is_some() {
            return Err(RealtimeError::InvalidConfiguration(
                "connect_with_sdp owns transport selection".to_owned(),
            ));
        }
        let version = self.version.unwrap_or(RealtimeVersion::V1);
        validate_external_call_version(version)?;
        validate_realtime_configuration(
            version,
            RealtimeTransport::WebRtc,
            self.session_mode,
            self.output_modality,
            &self.initial_items,
        )?;
        validate_initial_items(version, &self.initial_items)?;
        let protocol = realtime_protocol(version);
        let model = self.model.clone().unwrap_or_else(|| match version {
            RealtimeVersion::V1 | RealtimeVersion::V2 => REALTIME_MODEL.to_owned(),
            RealtimeVersion::V3 => CHATGPT_REALTIME_MODEL.to_owned(),
        });
        let voice = self.voice.unwrap_or(match version {
            RealtimeVersion::V1 | RealtimeVersion::V3 => CHATGPT_REALTIME_VOICE,
            RealtimeVersion::V2 => PLATFORM_REALTIME_VOICE,
        });
        validate_voice(version, voice)?;
        let offer_sdp = offer_sdp.into();
        let prepared = webrtc::prepare_with_sdp(
            webrtc::ConnectConfig {
                auth: &self.auth,
                api_base_url: &self.api_base_url,
                attestation_header: self.attestation_header.as_deref(),
                websocket_url: self.websocket_url.as_deref(),
                instructions: &self.instructions,
                model: &model,
                voice,
                session_id: self.session_id.as_deref(),
                initial_items: &self.initial_items,
                delegation_ack_filler: self.delegation_ack_filler,
                version,
            },
            &offer_sdp,
        )
        .await?;
        let output_policy = OutputPolicy {
            codex_responses_as_items: self.codex_responses_as_items,
            codex_response_item_prefix: self.codex_response_item_prefix,
            handoff_mode: self.codex_response_handoff_mode,
            channel_prefixes: self.codex_response_handoff_channel_prefixes,
        };
        let (session, events) = spawn_pending_sideband_session(
            prepared.sideband,
            prepared.initial_update,
            protocol,
            output_policy,
            self.client_managed_handoffs,
        );
        Ok(RealtimeSdpConnection {
            sdp: prepared.sdp,
            session,
            events,
        })
    }
}

pub struct RealtimeCallAttachmentBuilder {
    auth: OpenAiAuth,
    call_id: String,
    attestation_header: Option<Arc<str>>,
    websocket_url: Option<String>,
    session_id: Option<String>,
    version: Option<RealtimeVersion>,
    client_managed_handoffs: bool,
    codex_responses_as_items: bool,
    codex_response_item_prefix: Option<String>,
    codex_response_handoff_mode: RealtimeResponseHandoffMode,
    codex_response_handoff_channel_prefixes: BTreeMap<String, Vec<String>>,
}

impl RealtimeCallAttachmentBuilder {
    pub(crate) const fn new(auth: OpenAiAuth, call_id: String) -> Self {
        Self {
            auth,
            call_id,
            attestation_header: None,
            websocket_url: None,
            session_id: None,
            version: None,
            client_managed_handoffs: false,
            codex_responses_as_items: false,
            codex_response_item_prefix: None,
            codex_response_handoff_mode: RealtimeResponseHandoffMode::Thinking,
            codex_response_handoff_channel_prefixes: BTreeMap::new(),
        }
    }

    #[must_use]
    pub const fn version(mut self, version: RealtimeVersion) -> Self {
        self.version = Some(version);
        self
    }

    #[must_use]
    pub fn websocket_url(mut self, websocket_url: impl Into<String>) -> Self {
        self.websocket_url = Some(websocket_url.into());
        self
    }

    #[must_use]
    pub fn session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    #[must_use]
    pub fn attestation_header(mut self, value: impl Into<Arc<str>>) -> Self {
        self.attestation_header = Some(value.into());
        self
    }

    #[must_use]
    pub const fn client_managed_handoffs(mut self, managed: bool) -> Self {
        self.client_managed_handoffs = managed;
        self
    }

    #[must_use]
    pub const fn codex_responses_as_items(mut self, as_items: bool) -> Self {
        self.codex_responses_as_items = as_items;
        self
    }

    #[must_use]
    pub fn codex_response_item_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.codex_response_item_prefix = Some(prefix.into());
        self
    }

    #[must_use]
    pub const fn codex_response_handoff_mode(mut self, mode: RealtimeResponseHandoffMode) -> Self {
        self.codex_response_handoff_mode = mode;
        self
    }

    #[must_use]
    pub fn codex_response_handoff_channel_prefixes(
        mut self,
        prefixes: BTreeMap<String, Vec<String>>,
    ) -> Self {
        self.codex_response_handoff_channel_prefixes = prefixes;
        self
    }

    pub async fn connect(self) -> Result<(RealtimeSession, RealtimeEvents), RealtimeError> {
        let version = self.version.unwrap_or(RealtimeVersion::V1);
        validate_external_call_version(version)?;
        let protocol = realtime_protocol(version);
        let connection = webrtc::connect_existing_call(webrtc::ExistingCallConfig {
            auth: &self.auth,
            attestation_header: self.attestation_header.as_deref(),
            websocket_url: self.websocket_url.as_deref(),
            session_id: self.session_id.as_deref(),
            version,
            call_id: &self.call_id,
        })
        .await?;
        let output_policy = OutputPolicy {
            codex_responses_as_items: self.codex_responses_as_items,
            codex_response_item_prefix: self.codex_response_item_prefix,
            handoff_mode: self.codex_response_handoff_mode,
            channel_prefixes: self.codex_response_handoff_channel_prefixes,
        };
        Ok(spawn_connected_session(
            connection.socket,
            protocol,
            None,
            Some(connection.sideband),
            output_policy,
            self.client_managed_handoffs,
            SessionOwnership::External,
            None,
        ))
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_connected_session(
    socket: Socket,
    protocol: RealtimeProtocol,
    media: Option<webrtc::WebRtcMedia>,
    sideband: Option<webrtc::WebRtcSideband>,
    output_policy: OutputPolicy,
    client_managed_handoffs: bool,
    ownership: SessionOwnership,
    initial_event: Option<RealtimeEvent>,
) -> (RealtimeSession, RealtimeEvents) {
    let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
    let (closed_tx, closed) = watch::channel(false);
    if let Some(event) = initial_event {
        event_tx
            .try_send(event)
            .expect("a new realtime event queue has capacity");
    }
    tokio::spawn(async move {
        run_socket_with_pending(
            socket,
            command_rx,
            event_tx,
            protocol,
            media,
            sideband,
            output_policy,
            ownership,
            VecDeque::new(),
        )
        .await;
        closed_tx.send_replace(true);
    });
    (
        RealtimeSession {
            commands: command_tx,
            protocol,
            client_managed_handoffs,
            closed,
        },
        RealtimeEvents { receiver: event_rx },
    )
}

fn spawn_pending_sideband_session(
    sideband: webrtc::WebRtcSideband,
    initial_update: Option<Value>,
    protocol: RealtimeProtocol,
    output_policy: OutputPolicy,
    client_managed_handoffs: bool,
) -> (RealtimeSession, RealtimeEvents) {
    let (command_tx, command_rx) = mpsc::channel(COMMAND_CAPACITY);
    let (event_tx, event_rx) = mpsc::channel(EVENT_CAPACITY);
    let (closed_tx, closed) = watch::channel(false);
    tokio::spawn(async move {
        run_pending_sideband(
            sideband,
            initial_update,
            command_rx,
            event_tx,
            protocol,
            output_policy,
        )
        .await;
        closed_tx.send_replace(true);
    });
    (
        RealtimeSession {
            commands: command_tx,
            protocol,
            client_managed_handoffs,
            closed,
        },
        RealtimeEvents { receiver: event_rx },
    )
}

fn validate_session_builder(builder: &RealtimeSessionBuilder) -> Result<(), RealtimeError> {
    if builder.instructions.trim().is_empty() {
        return Err(RealtimeError::InvalidInstructions);
    }
    if builder
        .model
        .as_ref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(RealtimeError::InvalidModel);
    }
    Ok(())
}

const fn realtime_protocol(version: RealtimeVersion) -> RealtimeProtocol {
    match version {
        RealtimeVersion::V1 => RealtimeProtocol::V1,
        RealtimeVersion::V2 => RealtimeProtocol::Direct,
        RealtimeVersion::V3 => RealtimeProtocol::Frameless,
    }
}

fn validate_external_call_version(version: RealtimeVersion) -> Result<(), RealtimeError> {
    if version == RealtimeVersion::V2 {
        Err(RealtimeError::InvalidConfiguration(
            "AVAS realtime calls require realtime v1 or v3".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn validate_initial_items(
    version: RealtimeVersion,
    items: &[RealtimeInitialItem],
) -> Result<(), RealtimeError> {
    if !items.is_empty() && version != RealtimeVersion::V3 {
        return Err(RealtimeError::InvalidInitialItems(
            "initial items require realtime v3".to_owned(),
        ));
    }
    if items.len() > INITIAL_ITEMS_MAX_COUNT {
        return Err(RealtimeError::InvalidInitialItems(format!(
            "must contain no more than {INITIAL_ITEMS_MAX_COUNT} items"
        )));
    }

    let mut total_tokens = 0_usize;
    for item in items {
        let item_tokens = approx_token_count(&item.text);
        if item_tokens > INITIAL_ITEMS_MAX_TOKENS {
            return Err(RealtimeError::InvalidInitialItems(format!(
                "each item must not exceed {INITIAL_ITEMS_MAX_TOKENS} estimated tokens"
            )));
        }
        total_tokens = total_tokens.saturating_add(item_tokens);
    }
    if total_tokens > INITIAL_ITEMS_MAX_TOKENS {
        return Err(RealtimeError::InvalidInitialItems(format!(
            "items must not exceed {INITIAL_ITEMS_MAX_TOKENS} estimated tokens in total"
        )));
    }
    Ok(())
}

fn validate_realtime_configuration(
    version: RealtimeVersion,
    transport: RealtimeTransport,
    session_mode: RealtimeSessionMode,
    output_modality: RealtimeOutputModality,
    initial_items: &[RealtimeInitialItem],
) -> Result<(), RealtimeError> {
    if transport == RealtimeTransport::WebRtc && version == RealtimeVersion::V2 {
        return Err(RealtimeError::InvalidConfiguration(
            "AVAS WebRTC requires realtime v1 or v3".to_owned(),
        ));
    }
    if version != RealtimeVersion::V2 && output_modality == RealtimeOutputModality::Text {
        return Err(RealtimeError::InvalidConfiguration(
            "text output modality requires realtime v2".to_owned(),
        ));
    }
    if transport == RealtimeTransport::WebRtc && session_mode == RealtimeSessionMode::Transcription
    {
        return Err(RealtimeError::InvalidConfiguration(
            "AVAS realtime calls require conversational realtime".to_owned(),
        ));
    }
    if version != RealtimeVersion::V3 && !initial_items.is_empty() {
        return Err(RealtimeError::InvalidInitialItems(
            "initial items require realtime v3".to_owned(),
        ));
    }
    Ok(())
}

fn validate_voice(version: RealtimeVersion, voice: RealtimeVoice) -> Result<(), RealtimeError> {
    let supported = match version {
        RealtimeVersion::V1 | RealtimeVersion::V3 => voice.supports_frameless(),
        RealtimeVersion::V2 => voice.supports_direct(),
    };
    if supported {
        Ok(())
    } else {
        Err(RealtimeError::InvalidVoice(voice.to_string()))
    }
}

const fn approx_token_count(text: &str) -> usize {
    text.len()
        .saturating_add(APPROX_BYTES_PER_TOKEN.saturating_sub(1))
        / APPROX_BYTES_PER_TOKEN
}

#[derive(Debug, thiserror::Error)]
pub enum RealtimeError {
    #[error(transparent)]
    Authentication(#[from] OpenAiAuthError),

    #[error("GPT Realtime instructions must not be empty")]
    InvalidInstructions,

    #[error("GPT Realtime model must not be empty")]
    InvalidModel,

    #[error("invalid GPT Realtime configuration: {0}")]
    InvalidConfiguration(String),

    #[error("unsupported GPT Realtime voice {0:?}")]
    InvalidVoice(String),

    #[error("invalid GPT Realtime audio: {0}")]
    InvalidAudio(String),

    #[error("invalid GPT Realtime URL: {0}")]
    InvalidUrl(String),

    #[error("invalid GPT Realtime authorization: {0}")]
    InvalidAuthorization(String),

    #[error("invalid GPT Realtime session ID: {0}")]
    InvalidSessionId(String),

    #[error("invalid GPT Realtime initial items: {0}")]
    InvalidInitialItems(String),

    #[error("GPT Realtime connection timed out")]
    ConnectTimeout,

    #[error("GPT Realtime send timed out")]
    SendTimeout,

    #[error("GPT Realtime session is closed")]
    Closed,

    #[error("GPT Realtime WebSocket failed: {0}")]
    WebSocket(String),

    #[error("GPT Realtime sideband handshake failed with HTTP {status}")]
    WebSocketHandshake { status: u16 },

    #[error("GPT Realtime HTTP call failed: {0}")]
    Http(String),

    #[error("GPT Realtime WebRTC failed: {0}")]
    WebRtc(String),

    #[error("invalid GPT Realtime message: {0}")]
    Message(String),
}

struct Command {
    kind: CommandKind,
    result: oneshot::Sender<Result<CommandOutcome, RealtimeError>>,
}

enum CommandKind {
    Audio(RealtimeAudio),
    Text {
        role: RealtimeInputTextRole,
        text: String,
    },
    Speech {
        text: String,
    },
    AgentOutput {
        call_id: String,
        output: String,
    },
    AgentProgress {
        call_id: String,
        output: String,
        phase: Option<MessagePhase>,
    },
    StandaloneAgentOutput {
        output: String,
        phase: Option<MessagePhase>,
    },
    AgentComplete {
        call_id: String,
    },
    SilentOutput {
        call_id: String,
    },
    Close,
}

enum CommandOutcome {
    Continue,
    Closed(Vec<RealtimeTranscriptEntry>),
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum RealtimeProtocol {
    V1,
    Direct,
    Frameless,
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum SessionOwnership {
    Owned,
    External,
}

struct OutputPolicy {
    codex_responses_as_items: bool,
    codex_response_item_prefix: Option<String>,
    handoff_mode: RealtimeResponseHandoffMode,
    channel_prefixes: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Default)]
struct OutputRoutingState {
    bem_channels: HashMap<String, BemChannelParser>,
}

#[derive(Default)]
struct SocketState {
    active_transcript: ActiveTranscript,
    response_create: ResponseCreateQueue,
    output_audio: Option<OutputAudioState>,
    output_routing: OutputRoutingState,
}

#[derive(Clone, Default)]
struct BemChannelParser {
    buffered_text: String,
    phase: Option<MessagePhase>,
}

impl BemChannelParser {
    fn push(&mut self, text: &str, prefixes: &BTreeMap<String, Vec<String>>) -> Option<String> {
        if self.phase.is_some() {
            return Some(text.to_owned());
        }

        self.buffered_text.push_str(text);
        self.phase = bem_phase(&self.buffered_text, prefixes);
        self.phase?;
        Some(std::mem::take(&mut self.buffered_text))
    }

    const fn phase(&self) -> Option<MessagePhase> {
        self.phase
    }

    fn finish(&mut self) -> String {
        std::mem::take(&mut self.buffered_text)
    }
}

#[derive(Serialize)]
#[serde(tag = "type")]
enum ClientEvent<'a> {
    #[serde(rename = "session.update")]
    SessionUpdate { session: SessionUpdate<'a> },
    #[serde(rename = "input_audio_buffer.append")]
    AudioBufferAppend { audio: String },
    #[serde(rename = "input_audio.append")]
    AudioAppend { audio: String },
    #[serde(rename = "conversation.item.create")]
    ItemCreate { item: ConversationItem<'a> },
    #[serde(rename = "conversation.item.truncate")]
    ItemTruncate {
        item_id: &'a str,
        content_index: u8,
        audio_end_ms: u32,
    },
    #[serde(rename = "response.create")]
    ResponseCreate,
    #[serde(rename = "delegation.context.append")]
    DelegationContextAppend {
        delegation_item_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        channel: Option<RealtimeContextAppendChannel>,
        content: [FramelessInputText<'a>; 1],
    },
    #[serde(rename = "session.context.append")]
    SessionContextAppend {
        #[serde(skip_serializing_if = "Option::is_none")]
        channel: Option<RealtimeContextAppendChannel>,
        content: [FramelessInputText<'a>; 1],
    },
    #[serde(rename = "conversation.handoff.append")]
    ConversationHandoffAppend {
        handoff_id: &'a str,
        output_text: &'a str,
    },
    #[serde(rename = "session.close")]
    SessionClose,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum RealtimeContextAppendChannel {
    Speakable,
    Commentary,
}

#[derive(Serialize)]
struct FramelessInputText<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
}

#[derive(Serialize)]
struct SessionUpdate<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    instructions: &'a str,
    output_modalities: [&'static str; 1],
    audio: SessionAudio<'a>,
    tools: [SessionTool; 2],
    tool_choice: &'static str,
}

#[derive(Serialize)]
struct SessionAudio<'a> {
    input: SessionAudioInput,
    output: SessionAudioOutput<'a>,
}

#[derive(Serialize)]
struct SessionAudioInput {
    format: AudioFormat,
    noise_reduction: NoiseReduction,
    transcription: Transcription,
    turn_detection: TurnDetection,
}

#[derive(Serialize)]
struct SessionAudioOutput<'a> {
    format: AudioFormat,
    voice: &'a str,
}

#[derive(Clone, Copy, Serialize)]
struct AudioFormat {
    #[serde(rename = "type")]
    kind: &'static str,
    rate: u32,
}

#[derive(Serialize)]
struct NoiseReduction {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct Transcription {
    model: &'static str,
}

#[derive(Serialize)]
struct TurnDetection {
    #[serde(rename = "type")]
    kind: &'static str,
    interrupt_response: bool,
    create_response: bool,
    silence_duration_ms: u32,
}

#[derive(Serialize)]
struct SessionTool {
    #[serde(rename = "type")]
    kind: &'static str,
    name: &'static str,
    description: &'static str,
    parameters: Value,
}

#[derive(Serialize)]
#[serde(untagged)]
enum ConversationItem<'a> {
    Message {
        #[serde(rename = "type")]
        kind: &'static str,
        role: &'static str,
        content: [ConversationInputText<'a>; 1],
    },
    FunctionOutput {
        #[serde(rename = "type")]
        kind: &'static str,
        call_id: &'a str,
        output: &'a str,
    },
}

#[derive(Serialize)]
struct ConversationInputText<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    text: &'a str,
}

fn session_update(instructions: &str, voice: RealtimeVoice) -> ClientEvent<'_> {
    let format = AudioFormat {
        kind: "audio/pcm",
        rate: REALTIME_SAMPLE_RATE,
    };
    ClientEvent::SessionUpdate {
        session: SessionUpdate {
            kind: "realtime",
            instructions,
            output_modalities: ["audio"],
            audio: SessionAudio {
                input: SessionAudioInput {
                    format,
                    noise_reduction: NoiseReduction { kind: "near_field" },
                    transcription: Transcription {
                        model: "gpt-4o-mini-transcribe",
                    },
                    turn_detection: TurnDetection {
                        kind: "server_vad",
                        interrupt_response: true,
                        create_response: true,
                        silence_duration_ms: 500,
                    },
                },
                output: SessionAudioOutput {
                    format,
                    voice: voice.as_str(),
                },
            },
            tools: [
                SessionTool {
                    kind: "function",
                    name: BACKGROUND_AGENT_TOOL,
                    description: BACKGROUND_AGENT_TOOL_DESCRIPTION,
                    parameters: json!({
                        "type": "object",
                        "properties": {
                            "prompt": {
                                "type": "string",
                                "description": "The user request to delegate to the background agent."
                            }
                        },
                        "required": ["prompt"],
                        "additionalProperties": false
                    }),
                },
                SessionTool {
                    kind: "function",
                    name: REMAIN_SILENT_TOOL,
                    description: REMAIN_SILENT_TOOL_DESCRIPTION,
                    parameters: json!({
                        "type": "object",
                        "properties": {},
                        "additionalProperties": false
                    }),
                },
            ],
            tool_choice: "auto",
        },
    }
}

#[allow(clippy::too_many_arguments)]
fn configured_session_update(
    instructions: &str,
    model: &str,
    voice: RealtimeVoice,
    version: RealtimeVersion,
    mode: RealtimeSessionMode,
    output: RealtimeOutputModality,
    initial_items: &[RealtimeInitialItem],
    delegation_ack_filler: Option<bool>,
) -> Value {
    match version {
        RealtimeVersion::V1 => json!({
            "type": "session.update",
            "session": {
                "type": "quicksilver",
                "instructions": instructions,
                "audio": {
                    "input": {
                        "format": { "type": "audio/pcm", "rate": REALTIME_SAMPLE_RATE }
                    },
                    "output": { "voice": voice.as_str() }
                }
            }
        }),
        RealtimeVersion::V2 if mode == RealtimeSessionMode::Transcription => json!({
            "type": "session.update",
            "session": {
                "type": "transcription",
                "audio": {
                    "input": {
                        "format": { "type": "audio/pcm", "rate": REALTIME_SAMPLE_RATE },
                        "transcription": { "model": "gpt-4o-mini-transcribe" }
                    }
                }
            }
        }),
        RealtimeVersion::V2 => {
            let mut value = serde_json::to_value(session_update(instructions, voice))
                .expect("typed realtime session update serializes");
            value["session"]["output_modalities"] = json!([match output {
                RealtimeOutputModality::Text => "text",
                RealtimeOutputModality::Audio => "audio",
            }]);
            value
        }
        RealtimeVersion::V3 => {
            let items = initial_items
                .iter()
                .map(|item| {
                    json!({
                        "type": "message",
                        "role": item.role.as_str(),
                        "content": [{
                            "type": item.role.content_type(),
                            "text": item.text,
                        }],
                    })
                })
                .collect::<Vec<_>>();
            let mut session = json!({
                "model": model,
                "instructions": instructions,
                "audio": { "output": { "voice": voice.as_str() } },
                "delegation": { "type": "client" },
            });
            if let Some(delegation_ack_filler) = delegation_ack_filler {
                session["delegation"]["ack_filler"] = Value::Bool(delegation_ack_filler);
            }
            if !items.is_empty() {
                session["initial_items"] = Value::Array(items);
            }
            json!({ "type": "session.update", "session": session })
        }
    }
}

async fn run_pending_sideband(
    sideband: webrtc::WebRtcSideband,
    initial_update: Option<Value>,
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<RealtimeEvent>,
    protocol: RealtimeProtocol,
    output_policy: OutputPolicy,
) {
    let mut pending_commands = VecDeque::new();
    let mut socket = {
        let connect = sideband.reconnect();
        tokio::pin!(connect);
        loop {
            tokio::select! {
                result = &mut connect => {
                    match result {
                        Ok(socket) => break socket,
                        Err(error) => {
                            let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
                            return;
                        }
                    }
                }
                command = commands.recv(), if pending_commands.len() < COMMAND_CAPACITY => {
                    match command {
                        Some(Command { kind: CommandKind::Close, result }) => {
                            let _ = result.send(Ok(CommandOutcome::Closed(Vec::new())));
                            return;
                        }
                        Some(command) => pending_commands.push_back(command),
                        None => return,
                    }
                }
            }
        }
    };
    if let Some(update) = initial_update
        && let Err(error) = send_json(&mut socket, &update).await
    {
        let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
        return;
    }
    run_socket_with_pending(
        socket,
        commands,
        events,
        protocol,
        None,
        Some(sideband),
        output_policy,
        SessionOwnership::External,
        pending_commands,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
async fn run_socket_with_pending(
    mut socket: Socket,
    mut commands: mpsc::Receiver<Command>,
    events: mpsc::Sender<RealtimeEvent>,
    protocol: RealtimeProtocol,
    mut media: Option<webrtc::WebRtcMedia>,
    sideband: Option<webrtc::WebRtcSideband>,
    output_policy: OutputPolicy,
    ownership: SessionOwnership,
    mut pending_commands: VecDeque<Command>,
) {
    let media_input = media.as_ref().map(webrtc::WebRtcMedia::input);
    let reconnectable_sideband = sideband.is_some() && protocol == RealtimeProtocol::Frameless;
    let mut state = SocketState::default();
    let mut tail_returned = false;
    let mut rapid_disconnects = 0_u32;
    'session: loop {
        let connected_at = Instant::now();
        let transport_error = loop {
            if let Some(command) = pending_commands.pop_front() {
                match handle_socket_command(
                    &mut socket,
                    command,
                    protocol,
                    media_input.as_ref(),
                    &mut state,
                    &output_policy,
                    &events,
                    reconnectable_sideband,
                    ownership,
                )
                .await
                {
                    SocketCommandExit::Continue => continue,
                    SocketCommandExit::Terminal { returned_tail } => {
                        tail_returned = returned_tail;
                        break 'session;
                    }
                    SocketCommandExit::TransportLost { error, command } => {
                        pending_commands.push_front(command);
                        break error;
                    }
                }
            }

            let has_webrtc_media = media.is_some();
            tokio::select! {
                command = commands.recv() => {
                    let Some(command) = command else {
                        if let Err(error) = close_socket(&mut socket, protocol, ownership).await {
                            debug!(%error, "failed to close dropped GPT Realtime session");
                        }
                        break 'session;
                    };
                    match handle_socket_command(
                        &mut socket,
                        command,
                        protocol,
                        media_input.as_ref(),
                        &mut state,
                        &output_policy,
                        &events,
                        reconnectable_sideband,
                        ownership,
                    ).await {
                        SocketCommandExit::Continue => {}
                        SocketCommandExit::Terminal { returned_tail } => {
                            tail_returned = returned_tail;
                            break 'session;
                        }
                        SocketCommandExit::TransportLost { error, command } => {
                            pending_commands.push_front(command);
                            break error;
                        }
                    }
                }
                message = socket.next() => {
                    match handle_server_message(
                        &mut socket,
                        message,
                        &events,
                        protocol,
                        &mut state,
                        has_webrtc_media,
                    ).await {
                        Ok(true) => break 'session,
                        Ok(false) => {}
                        Err(error) if reconnectable_sideband && sideband_transport_loss(&error) => {
                            break error;
                        }
                        Err(error) => {
                            let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
                            break 'session;
                        }
                    }
                }
                audio = recv_media(&mut media) => {
                    match audio {
                        Some(Ok(audio)) => {
                            if events.send(RealtimeEvent::Audio(audio)).await.is_err() {
                                break 'session;
                            }
                        }
                        Some(Err(error)) => {
                            let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
                            break 'session;
                        }
                        None => media = None,
                    }
                }
            }
        };

        let Some(sideband) = sideband.as_ref() else {
            let _ = events
                .send(RealtimeEvent::Error(transport_error.to_string()))
                .await;
            break;
        };
        if connected_at.elapsed() >= SIDEBAND_STABLE_CONNECTION_DURATION {
            rapid_disconnects = 0;
        }
        rapid_disconnects = rapid_disconnects.saturating_add(1);
        let delay = sideband_reconnect_delay(rapid_disconnects);
        warn!(
            delay_ms = delay.as_millis(),
            "live Realtime sideband transport lost; reconnecting: {transport_error}"
        );
        match reconnect_sideband(
            sideband,
            delay,
            &mut commands,
            &mut pending_commands,
            &events,
            &mut media,
            media_input.as_ref(),
            &mut state,
        )
        .await
        {
            SidebandReconnectExit::Connected(reconnected) => socket = reconnected,
            SidebandReconnectExit::Terminal { returned_tail } => {
                tail_returned = returned_tail;
                break;
            }
            SidebandReconnectExit::Failed(error) if webrtc::sideband_session_ended(&error) => {
                debug!("Realtime sideband session ended while reconnecting");
                if let Some(command) = pending_commands.pop_front() {
                    let _ = command.result.send(Err(error));
                }
                break;
            }
            SidebandReconnectExit::Failed(error) => {
                let message = error.to_string();
                if let Some(command) = pending_commands.pop_front() {
                    let _ = command.result.send(Err(error));
                }
                let _ = events.send(RealtimeEvent::Error(message)).await;
                break;
            }
        }
    }
    if !tail_returned {
        let tail = state.active_transcript.take_tail();
        if !tail.is_empty() {
            let _ = events.send(RealtimeEvent::TranscriptTail(tail)).await;
        }
    }
    if let Some(media) = media {
        media.close().await;
    }
    debug!("GPT Realtime websocket task stopped");
}

enum SocketCommandExit {
    Continue,
    Terminal {
        returned_tail: bool,
    },
    TransportLost {
        error: RealtimeError,
        command: Command,
    },
}

enum SidebandReconnectExit {
    Connected(Socket),
    Terminal { returned_tail: bool },
    Failed(RealtimeError),
}

#[allow(clippy::too_many_arguments)]
async fn reconnect_sideband(
    sideband: &webrtc::WebRtcSideband,
    delay: Duration,
    commands: &mut mpsc::Receiver<Command>,
    pending_commands: &mut VecDeque<Command>,
    events: &mpsc::Sender<RealtimeEvent>,
    media: &mut Option<webrtc::WebRtcMedia>,
    media_input: Option<&mpsc::Sender<RealtimeAudio>>,
    state: &mut SocketState,
) -> SidebandReconnectExit {
    let reconnect = async {
        tokio::time::sleep(delay).await;
        sideband.reconnect().await
    };
    tokio::pin!(reconnect);
    loop {
        tokio::select! {
            result = &mut reconnect => {
                return match result {
                    Ok(socket) => SidebandReconnectExit::Connected(socket),
                    Err(error) => SidebandReconnectExit::Failed(error),
                };
            }
            command = commands.recv(), if pending_commands.len() < COMMAND_CAPACITY => {
                let Some(command) = command else {
                    return SidebandReconnectExit::Terminal { returned_tail: false };
                };
                match &command.kind {
                    CommandKind::Audio(audio) => {
                        let result = send_webrtc_audio(media_input, audio).await;
                        let failed = result.is_err();
                        if let Err(error) = &result {
                            let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
                        }
                        let _ = command.result.send(result.map(|()| CommandOutcome::Continue));
                        if failed {
                            return SidebandReconnectExit::Terminal { returned_tail: false };
                        }
                    }
                    CommandKind::Close => {
                        let tail = state.active_transcript.take_tail();
                        let _ = command.result.send(Ok(CommandOutcome::Closed(tail)));
                        return SidebandReconnectExit::Terminal { returned_tail: true };
                    }
                    _ => pending_commands.push_back(command),
                }
            }
            audio = recv_media(media) => {
                match audio {
                    Some(Ok(audio)) => {
                        if events.send(RealtimeEvent::Audio(audio)).await.is_err() {
                            return SidebandReconnectExit::Terminal { returned_tail: false };
                        }
                    }
                    Some(Err(error)) => {
                        let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
                        return SidebandReconnectExit::Terminal { returned_tail: false };
                    }
                    None => *media = None,
                }
            }
        }
    }
}

async fn send_webrtc_audio(
    media_input: Option<&mpsc::Sender<RealtimeAudio>>,
    audio: &RealtimeAudio,
) -> Result<(), RealtimeError> {
    let Some(input) = media_input else {
        return Err(RealtimeError::Closed);
    };
    if audio.is_empty() {
        return Ok(());
    }
    timeout(SEND_TIMEOUT, input.send(audio.clone()))
        .await
        .map_err(|_| RealtimeError::SendTimeout)?
        .map_err(|_| RealtimeError::Closed)
}

#[allow(clippy::too_many_arguments)]
async fn handle_socket_command(
    socket: &mut Socket,
    command: Command,
    protocol: RealtimeProtocol,
    media_input: Option<&mpsc::Sender<RealtimeAudio>>,
    state: &mut SocketState,
    output_policy: &OutputPolicy,
    events: &mpsc::Sender<RealtimeEvent>,
    reconnectable: bool,
    ownership: SessionOwnership,
) -> SocketCommandExit {
    let replayable = reconnectable && command.kind.replayable_after_sideband_loss();
    let output_routing_before = replayable.then(|| state.output_routing.clone());
    let result = handle_command(
        socket,
        &command.kind,
        protocol,
        media_input,
        state,
        output_policy,
        ownership,
    )
    .await;
    let result = match result {
        Err(error) if replayable && sideband_transport_loss(&error) => {
            if let Some(output_routing) = output_routing_before {
                state.output_routing = output_routing;
            }
            return SocketCommandExit::TransportLost { error, command };
        }
        result => result,
    };
    let returned_tail = matches!(result, Ok(CommandOutcome::Closed(_)));
    let failed = result.is_err();
    if let Err(error) = &result {
        let _ = events.send(RealtimeEvent::Error(error.to_string())).await;
    }
    let _ = command.result.send(result);
    if returned_tail || failed {
        SocketCommandExit::Terminal { returned_tail }
    } else {
        SocketCommandExit::Continue
    }
}

impl CommandKind {
    const fn replayable_after_sideband_loss(&self) -> bool {
        matches!(
            self,
            Self::Text { .. }
                | Self::Speech { .. }
                | Self::AgentOutput { .. }
                | Self::AgentProgress { .. }
                | Self::StandaloneAgentOutput { .. }
                | Self::AgentComplete { .. }
                | Self::SilentOutput { .. }
        )
    }
}

const fn sideband_transport_loss(error: &RealtimeError) -> bool {
    matches!(
        error,
        RealtimeError::WebSocket(_) | RealtimeError::SendTimeout | RealtimeError::Closed
    )
}

fn sideband_reconnect_delay(rapid_disconnects: u32) -> Duration {
    let multiplier = 2_u32.saturating_pow(rapid_disconnects.saturating_sub(1));
    SIDEBAND_RECONNECT_BASE_DELAY
        .saturating_mul(multiplier)
        .min(SIDEBAND_RECONNECT_MAX_DELAY)
}

async fn recv_media(
    media: &mut Option<webrtc::WebRtcMedia>,
) -> Option<Result<RealtimeAudio, RealtimeError>> {
    match media {
        Some(media) => media.recv().await,
        None => std::future::pending().await,
    }
}

async fn handle_command(
    socket: &mut Socket,
    command: &CommandKind,
    protocol: RealtimeProtocol,
    media_input: Option<&mpsc::Sender<RealtimeAudio>>,
    state: &mut SocketState,
    output_policy: &OutputPolicy,
    ownership: SessionOwnership,
) -> Result<CommandOutcome, RealtimeError> {
    match command {
        CommandKind::Audio(audio) => {
            if !audio.is_empty() {
                match protocol {
                    RealtimeProtocol::V1 => {
                        send_json(
                            socket,
                            &ClientEvent::AudioAppend {
                                audio: STANDARD.encode(audio.as_bytes()),
                            },
                        )
                        .await?;
                    }
                    RealtimeProtocol::Direct => {
                        send_json(
                            socket,
                            &ClientEvent::AudioBufferAppend {
                                audio: STANDARD.encode(audio.as_bytes()),
                            },
                        )
                        .await?;
                    }
                    RealtimeProtocol::Frameless => {
                        if media_input.is_some() {
                            send_webrtc_audio(media_input, audio).await?;
                        } else {
                            send_json(
                                socket,
                                &ClientEvent::AudioAppend {
                                    audio: STANDARD.encode(audio.as_bytes()),
                                },
                            )
                            .await?;
                        }
                    }
                }
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::Text { role, text } => {
            let text = if protocol == RealtimeProtocol::Direct
                && *role == RealtimeInputTextRole::User
                && !text.is_empty()
                && !text.starts_with("[USER] ")
            {
                Cow::Owned(format!("[USER] {text}"))
            } else {
                Cow::Borrowed(text.as_str())
            };
            if protocol == RealtimeProtocol::Frameless {
                send_session_context(socket, &text, None).await?;
            } else {
                send_conversation_text(socket, *role, &text).await?;
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::Speech { text } => {
            let text = realtime_backend_output(protocol, text.clone());
            match protocol {
                RealtimeProtocol::V1 => {
                    send_handoff_append(socket, STANDALONE_HANDOFF_ID, &text).await?;
                }
                RealtimeProtocol::Direct => {
                    send_backend_output(socket, &text).await?;
                    state.response_create.request(socket).await?;
                }
                RealtimeProtocol::Frameless => {
                    send_session_context(
                        socket,
                        &text,
                        Some(RealtimeContextAppendChannel::Speakable),
                    )
                    .await?;
                }
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::AgentOutput { call_id, output } => {
            match protocol {
                RealtimeProtocol::V1 => {
                    send_function_output(socket, call_id, output).await?;
                }
                RealtimeProtocol::Direct => {
                    send_function_output(socket, call_id, output).await?;
                    state.response_create.request(socket).await?;
                }
                RealtimeProtocol::Frameless => {
                    send_delegation_context(socket, call_id, output, None).await?;
                }
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::AgentProgress {
            call_id,
            output,
            phase,
        } => {
            let Some((output, phase)) = route_streamed_output(
                protocol,
                call_id,
                output.clone(),
                *phase,
                output_policy,
                &mut state.output_routing,
            ) else {
                return Ok(CommandOutcome::Continue);
            };
            send_agent_progress(socket, protocol, call_id, output, phase, output_policy).await?;
            Ok(CommandOutcome::Continue)
        }
        CommandKind::StandaloneAgentOutput { output, phase } => {
            let mut output = output.clone();
            let phase = standalone_output_phase(protocol, &output, *phase, output_policy);
            let channel = output_channel(&output, phase, output_policy);
            output = realtime_backend_output(protocol, output);
            if output_policy.codex_responses_as_items {
                send_response_item(socket, protocol, &output, channel, output_policy).await?;
            } else {
                match protocol {
                    RealtimeProtocol::V1 => {
                        if !matches!(phase, Some(MessagePhase::Commentary)) {
                            output = format!("{AGENT_FINAL_MESSAGE_PREFIX}{output}");
                        }
                        send_handoff_append(socket, STANDALONE_HANDOFF_ID, &output).await?;
                    }
                    RealtimeProtocol::Direct => {
                        send_backend_output(socket, &output).await?;
                        state.response_create.request(socket).await?;
                    }
                    RealtimeProtocol::Frameless => {
                        send_session_context(socket, &output, channel).await?;
                    }
                }
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::AgentComplete { call_id } => {
            if let Some(mut parser) = state.output_routing.bem_channels.remove(call_id) {
                let output = parser.finish();
                if !output.is_empty() {
                    warn!(%call_id, "BEM output ended before a recognized channel header was received");
                    send_agent_progress(
                        socket,
                        protocol,
                        call_id,
                        output,
                        Some(MessagePhase::FinalAnswer),
                        output_policy,
                    )
                    .await?;
                }
            }
            if matches!(protocol, RealtimeProtocol::Direct) {
                let acknowledgement = if output_policy.codex_responses_as_items {
                    ""
                } else {
                    AGENT_COMPLETE_ACKNOWLEDGEMENT
                };
                send_function_output(socket, call_id, acknowledgement).await?;
                state.response_create.request(socket).await?;
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::SilentOutput { call_id } => {
            match protocol {
                RealtimeProtocol::V1 | RealtimeProtocol::Direct => {
                    send_function_output(socket, call_id, "").await?
                }
                RealtimeProtocol::Frameless => {
                    send_delegation_context(socket, call_id, "", None).await?;
                }
            }
            Ok(CommandOutcome::Continue)
        }
        CommandKind::Close => {
            let tail = state.active_transcript.take_tail();
            close_socket(socket, protocol, ownership).await?;
            Ok(CommandOutcome::Closed(tail))
        }
    }
}

fn route_streamed_output(
    protocol: RealtimeProtocol,
    call_id: &str,
    output: String,
    phase: Option<MessagePhase>,
    policy: &OutputPolicy,
    state: &mut OutputRoutingState,
) -> Option<(String, Option<MessagePhase>)> {
    if protocol != RealtimeProtocol::Frameless
        || policy.handoff_mode != RealtimeResponseHandoffMode::BemTags
    {
        return Some((output, phase));
    }

    let parser = state.bem_channels.entry(call_id.to_owned()).or_default();
    let output = parser.push(&output, &policy.channel_prefixes)?;
    Some((output, parser.phase()))
}

fn standalone_output_phase(
    protocol: RealtimeProtocol,
    output: &str,
    phase: Option<MessagePhase>,
    policy: &OutputPolicy,
) -> Option<MessagePhase> {
    if protocol == RealtimeProtocol::Frameless
        && policy.handoff_mode == RealtimeResponseHandoffMode::BemTags
    {
        bem_phase(output, &policy.channel_prefixes).or(Some(MessagePhase::FinalAnswer))
    } else {
        phase
    }
}

async fn send_agent_progress(
    socket: &mut Socket,
    protocol: RealtimeProtocol,
    call_id: &str,
    output: String,
    phase: Option<MessagePhase>,
    output_policy: &OutputPolicy,
) -> Result<(), RealtimeError> {
    let channel = output_channel(&output, phase, output_policy);
    let output = realtime_backend_output(protocol, output);
    if output_policy.codex_responses_as_items {
        return send_response_item(socket, protocol, &output, channel, output_policy).await;
    }

    match protocol {
        RealtimeProtocol::V1 => {
            if matches!(phase, Some(MessagePhase::Commentary)) {
                send_handoff_append(socket, call_id, &output).await
            } else {
                send_function_output(socket, call_id, &output).await
            }
        }
        RealtimeProtocol::Direct => send_backend_output(socket, &output).await,
        RealtimeProtocol::Frameless => {
            send_delegation_context(socket, call_id, &output, channel).await
        }
    }
}

async fn send_session_context(
    socket: &mut Socket,
    output: &str,
    channel: Option<RealtimeContextAppendChannel>,
) -> Result<(), RealtimeError> {
    for chunk in context_append_chunks(output) {
        send_json(
            socket,
            &ClientEvent::SessionContextAppend {
                channel,
                content: [FramelessInputText {
                    kind: "input_text",
                    text: chunk,
                }],
            },
        )
        .await?;
    }
    Ok(())
}

async fn close_socket(
    socket: &mut Socket,
    protocol: RealtimeProtocol,
    ownership: SessionOwnership,
) -> Result<(), RealtimeError> {
    if ownership == SessionOwnership::External {
        return Ok(());
    }
    if matches!(protocol, RealtimeProtocol::Frameless) {
        send_json(socket, &ClientEvent::SessionClose).await?;
    }
    socket.close(None).await.map_err(map_websocket_error)
}

async fn send_delegation_context(
    socket: &mut Socket,
    delegation_item_id: &str,
    output: &str,
    channel: Option<RealtimeContextAppendChannel>,
) -> Result<(), RealtimeError> {
    for chunk in context_append_chunks(output) {
        send_json(
            socket,
            &ClientEvent::DelegationContextAppend {
                delegation_item_id,
                channel,
                content: [FramelessInputText {
                    kind: "input_text",
                    text: chunk,
                }],
            },
        )
        .await?;
    }
    Ok(())
}

async fn send_handoff_append(
    socket: &mut Socket,
    handoff_id: &str,
    output: &str,
) -> Result<(), RealtimeError> {
    send_json(
        socket,
        &ClientEvent::ConversationHandoffAppend {
            handoff_id,
            output_text: output,
        },
    )
    .await
}

async fn send_conversation_text(
    socket: &mut Socket,
    role: RealtimeInputTextRole,
    text: &str,
) -> Result<(), RealtimeError> {
    send_json(
        socket,
        &ClientEvent::ItemCreate {
            item: ConversationItem::Message {
                kind: "message",
                role: role.as_str(),
                content: [ConversationInputText {
                    kind: role.content_type(),
                    text,
                }],
            },
        },
    )
    .await
}

async fn send_response_item(
    socket: &mut Socket,
    protocol: RealtimeProtocol,
    output: &str,
    channel: Option<RealtimeContextAppendChannel>,
    policy: &OutputPolicy,
) -> Result<(), RealtimeError> {
    let output = policy
        .codex_response_item_prefix
        .as_deref()
        .filter(|prefix| !prefix.is_empty())
        .map_or_else(
            || output.to_owned(),
            |prefix| format!("{prefix}\n\n{output}"),
        );
    let output =
        truncate_realtime_text_to_token_budget(&output, REALTIME_ASSISTANT_OUTPUT_TOKEN_BUDGET);
    if protocol == RealtimeProtocol::Frameless {
        send_session_context(socket, &output, channel).await
    } else {
        send_conversation_text(socket, RealtimeInputTextRole::Developer, &output).await
    }
}

fn output_channel(
    output: &str,
    phase: Option<MessagePhase>,
    policy: &OutputPolicy,
) -> Option<RealtimeContextAppendChannel> {
    match policy.handoff_mode {
        RealtimeResponseHandoffMode::Thinking => None,
        RealtimeResponseHandoffMode::Commentary => Some(RealtimeContextAppendChannel::Commentary),
        RealtimeResponseHandoffMode::BemTags => {
            match phase.or_else(|| bem_phase(output, &policy.channel_prefixes)) {
                Some(MessagePhase::Commentary) => Some(RealtimeContextAppendChannel::Commentary),
                Some(MessagePhase::FinalAnswer) | None => {
                    Some(RealtimeContextAppendChannel::Speakable)
                }
            }
        }
    }
}

fn bem_phase(text: &str, prefixes: &BTreeMap<String, Vec<String>>) -> Option<MessagePhase> {
    for (channel, default, phase) in [
        (
            "analysis",
            "<|start|>assistant<|channel|>analysis<|message|>",
            MessagePhase::Commentary,
        ),
        (
            "commentary",
            "<|start|>assistant<|channel|>commentary<|message|>",
            MessagePhase::Commentary,
        ),
        (
            "final",
            "<|start|>assistant<|channel|>final<|message|>",
            MessagePhase::FinalAnswer,
        ),
    ] {
        let matches = prefixes.get(channel).map_or_else(
            || text.starts_with(default),
            |prefixes| {
                prefixes
                    .iter()
                    .any(|prefix| !prefix.is_empty() && text.starts_with(prefix))
            },
        );
        if matches {
            return Some(phase);
        }
    }
    None
}

fn context_append_chunks(text: &str) -> Vec<&str> {
    if text.len() <= CONTEXT_APPEND_MAX_BYTES {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + CONTEXT_APPEND_MAX_BYTES).min(text.len());
        while end > start && !text.is_char_boundary(end) {
            end -= 1;
        }
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

async fn send_function_output(
    socket: &mut Socket,
    call_id: &str,
    output: &str,
) -> Result<(), RealtimeError> {
    send_json(
        socket,
        &ClientEvent::ItemCreate {
            item: ConversationItem::FunctionOutput {
                kind: "function_call_output",
                call_id,
                output,
            },
        },
    )
    .await
}

async fn send_backend_output(socket: &mut Socket, output: &str) -> Result<(), RealtimeError> {
    let text = if output.starts_with(BACKEND_TEXT_PREFIX) {
        output.to_owned()
    } else {
        format!("{BACKEND_TEXT_PREFIX}{output}")
    };
    send_json(
        socket,
        &ClientEvent::ItemCreate {
            item: ConversationItem::Message {
                kind: "message",
                role: "user",
                content: [ConversationInputText {
                    kind: "input_text",
                    text: &text,
                }],
            },
        },
    )
    .await
}

fn realtime_backend_output(protocol: RealtimeProtocol, output: String) -> String {
    let output = if protocol != RealtimeProtocol::Direct
        || output.is_empty()
        || output.starts_with(BACKEND_TEXT_PREFIX)
    {
        output
    } else {
        format!("{BACKEND_TEXT_PREFIX}{output}")
    };
    truncate_realtime_text_to_token_budget(&output, REALTIME_ASSISTANT_OUTPUT_TOKEN_BUDGET)
}

fn truncate_realtime_text_to_token_budget(text: &str, budget_tokens: usize) -> String {
    let mut truncation_budget = budget_tokens;
    loop {
        let candidate = crate::client::session::compaction::truncate_middle_with_token_budget(
            text,
            truncation_budget,
        );
        let candidate_tokens = approx_token_count(&candidate);
        if candidate_tokens <= budget_tokens {
            break candidate;
        }
        let excess_tokens = candidate_tokens.saturating_sub(budget_tokens);
        let next_budget = truncation_budget.saturating_sub(excess_tokens.max(1));
        if next_budget == 0 {
            break crate::client::session::compaction::truncate_middle_with_token_budget(text, 0);
        }
        truncation_budget = next_budget;
    }
}

async fn wait_for_frameless_session_started(
    socket: &mut Socket,
) -> Result<RealtimeEvent, RealtimeError> {
    timeout(CONNECT_TIMEOUT, async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Text(payload))) => {
                    trace!(target: "crate::client::realtime::wire", payload = %payload, "GPT Realtime event");
                    let value: Value = serde_json::from_str(&payload)
                        .map_err(|error| RealtimeError::Message(error.to_string()))?;
                    match parse_event_value(&value, RealtimeProtocol::Frameless)? {
                        Some(event @ RealtimeEvent::SessionReady { .. }) => return Ok(event),
                        Some(RealtimeEvent::Error(message)) => {
                            return Err(RealtimeError::WebSocket(message));
                        }
                        _ => {
                            return Err(RealtimeError::WebSocket(
                                "frameless realtime session received an event before session.started"
                                    .to_owned(),
                            ));
                        }
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    return Err(RealtimeError::WebSocket(
                        "frameless realtime session ended before session.started".to_owned(),
                    ));
                }
                Some(Ok(Message::Binary(_))) => {
                    return Err(RealtimeError::WebSocket(
                        "unexpected binary realtime websocket event".to_owned(),
                    ));
                }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => {}
                Some(Ok(Message::Frame(_))) => {}
                Some(Err(error)) => return Err(map_websocket_error(error)),
            }
        }
    })
    .await
    .map_err(|_| RealtimeError::ConnectTimeout)?
}

async fn send_json<T: Serialize>(socket: &mut Socket, value: &T) -> Result<(), RealtimeError> {
    let payload =
        serde_json::to_string(value).map_err(|error| RealtimeError::Message(error.to_string()))?;
    trace!(target: "crate::client::realtime::wire", payload = %payload, "GPT Realtime request");
    timeout(SEND_TIMEOUT, socket.send(Message::Text(payload.into())))
        .await
        .map_err(|_| RealtimeError::SendTimeout)?
        .map_err(map_websocket_error)
}

async fn handle_server_message(
    socket: &mut Socket,
    message: Option<Result<Message, WebSocketError>>,
    events: &mpsc::Sender<RealtimeEvent>,
    protocol: RealtimeProtocol,
    state: &mut SocketState,
    has_webrtc_media: bool,
) -> Result<bool, RealtimeError> {
    let Some(message) = message else {
        return if protocol == RealtimeProtocol::Frameless {
            Err(RealtimeError::WebSocket(
                "Realtime sideband event stream ended unexpectedly".to_owned(),
            ))
        } else {
            Ok(true)
        };
    };
    match message.map_err(map_websocket_error)? {
        Message::Text(payload) => {
            trace!(target: "crate::client::realtime::wire", payload = %payload, "GPT Realtime event");
            let value: Value = serde_json::from_str(&payload)
                .map_err(|error| RealtimeError::Message(error.to_string()))?;
            if let Some(mut event) = parse_event_value(&value, protocol)? {
                if has_webrtc_media
                    && protocol == RealtimeProtocol::Frameless
                    && matches!(event, RealtimeEvent::Audio(_))
                {
                    return Ok(false);
                }
                if matches!(protocol, RealtimeProtocol::Direct) {
                    handle_direct_audio_state(socket, &value, &event, &mut state.output_audio)
                        .await;
                }
                if let RealtimeEvent::AgentRequest { call_id, .. } = &event
                    && !state.active_transcript.accept_delegation(call_id)
                {
                    return Ok(false);
                }
                state.active_transcript.update(&mut event, protocol);
                if matches!(protocol, RealtimeProtocol::Direct) {
                    match &event {
                        RealtimeEvent::ResponseStarted => state.response_create.mark_started(),
                        RealtimeEvent::ResponseDone => {
                            state.response_create.mark_finished(socket).await?
                        }
                        _ => {}
                    }
                }
                if events.send(event).await.is_err() {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Message::Ping(payload) => {
            socket
                .send(Message::Pong(payload))
                .await
                .map_err(map_websocket_error)?;
            Ok(false)
        }
        Message::Pong(_) | Message::Frame(_) => Ok(false),
        Message::Close(frame) => {
            if protocol == RealtimeProtocol::Frameless
                && !matches!(
                    frame.as_ref().map(|frame| frame.code),
                    Some(CloseCode::Normal)
                )
            {
                let detail = frame
                    .map(|frame| format!("{} ({})", frame.code, frame.reason))
                    .unwrap_or_else(|| "without a close frame".to_owned());
                Err(RealtimeError::WebSocket(format!(
                    "Realtime sideband closed unexpectedly: {detail}"
                )))
            } else {
                Ok(true)
            }
        }
        Message::Binary(_) => Err(RealtimeError::Message(
            "unexpected binary WebSocket frame".to_owned(),
        )),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct OutputAudioState {
    item_id: String,
    audio_end_ms: u32,
}

async fn handle_direct_audio_state(
    socket: &mut Socket,
    value: &Value,
    event: &RealtimeEvent,
    output_audio: &mut Option<OutputAudioState>,
) {
    match event {
        RealtimeEvent::Audio(audio) => update_output_audio_state(value, audio, output_audio),
        RealtimeEvent::SpeechStarted => {
            let Some(state) = output_audio.take() else {
                return;
            };
            let speech_item_id = value.get("item_id").and_then(Value::as_str);
            if speech_item_id.is_some_and(|item_id| item_id != state.item_id) {
                return;
            }
            if let Err(error) = send_json(
                socket,
                &ClientEvent::ItemTruncate {
                    item_id: &state.item_id,
                    content_index: 0,
                    audio_end_ms: state.audio_end_ms,
                },
            )
            .await
            {
                warn!(%error, "failed to truncate interrupted GPT Realtime audio");
            }
        }
        RealtimeEvent::ResponseDone
        | RealtimeEvent::AgentRequest { .. }
        | RealtimeEvent::RemainSilent { .. } => *output_audio = None,
        RealtimeEvent::SessionReady { .. }
        | RealtimeEvent::InputTranscriptDelta(_)
        | RealtimeEvent::InputTranscriptDone(_)
        | RealtimeEvent::OutputTranscriptDelta(_)
        | RealtimeEvent::OutputTranscriptDone(_)
        | RealtimeEvent::ResponseStarted
        | RealtimeEvent::TranscriptTail(_)
        | RealtimeEvent::Error(_) => {}
    }
}

fn update_output_audio_state(
    value: &Value,
    audio: &RealtimeAudio,
    output_audio: &mut Option<OutputAudioState>,
) {
    let Some(item_id) = value.get("item_id").and_then(Value::as_str) else {
        return;
    };
    let samples = value
        .get("samples_per_channel")
        .and_then(Value::as_u64)
        .unwrap_or(audio.samples() as u64);
    let sample_rate = value
        .get("sample_rate")
        .and_then(Value::as_u64)
        .unwrap_or(u64::from(REALTIME_SAMPLE_RATE))
        .max(1);
    let audio_end_ms =
        u32::try_from(samples.saturating_mul(1_000) / sample_rate).unwrap_or(u32::MAX);
    if audio_end_ms == 0 {
        return;
    }

    if let Some(state) = output_audio
        && state.item_id == item_id
    {
        state.audio_end_ms = state.audio_end_ms.saturating_add(audio_end_ms);
        return;
    }
    *output_audio = Some(OutputAudioState {
        item_id: item_id.to_owned(),
        audio_end_ms,
    });
}

#[derive(Default)]
struct ResponseCreateQueue {
    active: bool,
    pending: bool,
}

impl ResponseCreateQueue {
    async fn request(&mut self, socket: &mut Socket) -> Result<(), RealtimeError> {
        if self.active {
            self.pending = true;
            return Ok(());
        }
        send_json(socket, &ClientEvent::ResponseCreate).await?;
        self.active = true;
        Ok(())
    }

    const fn mark_started(&mut self) {
        self.active = true;
    }

    async fn mark_finished(&mut self, socket: &mut Socket) -> Result<(), RealtimeError> {
        self.active = false;
        if !self.pending {
            return Ok(());
        }
        self.pending = false;
        self.request(socket).await
    }
}

#[derive(Default)]
struct ActiveTranscript {
    entries: Vec<RealtimeTranscriptEntry>,
    new_input_entry: bool,
    new_output_entry: bool,
    seen_delegations: VecDeque<String>,
}

impl ActiveTranscript {
    fn accept_delegation(&mut self, id: &str) -> bool {
        if self.seen_delegations.iter().any(|seen| seen == id) {
            return false;
        }
        self.seen_delegations.push_back(id.to_owned());
        if self.seen_delegations.len() > 256 {
            self.seen_delegations.pop_front();
        }
        true
    }

    fn take_tail(&mut self) -> Vec<RealtimeTranscriptEntry> {
        std::mem::take(&mut self.entries)
            .into_iter()
            .filter(|entry| !entry.text.trim().is_empty())
            .collect()
    }

    fn update(&mut self, event: &mut RealtimeEvent, protocol: RealtimeProtocol) {
        match event {
            RealtimeEvent::SpeechStarted => self.new_input_entry = true,
            RealtimeEvent::InputTranscriptDelta(delta) => {
                append_transcript_delta(
                    &mut self.entries,
                    "user",
                    delta,
                    self.new_input_entry,
                    protocol,
                );
                if !delta.is_empty() || protocol != RealtimeProtocol::Frameless {
                    self.new_input_entry = false;
                }
            }
            RealtimeEvent::OutputTranscriptDelta(delta) => {
                append_transcript_delta(
                    &mut self.entries,
                    "assistant",
                    delta,
                    self.new_output_entry,
                    protocol,
                );
                if !delta.is_empty() || protocol != RealtimeProtocol::Frameless {
                    self.new_output_entry = false;
                }
            }
            RealtimeEvent::InputTranscriptDone(text) => {
                apply_transcript_done(
                    &mut self.entries,
                    "user",
                    text,
                    self.new_input_entry,
                    protocol,
                );
                self.new_input_entry = protocol == RealtimeProtocol::Frameless;
            }
            RealtimeEvent::OutputTranscriptDone(text) => {
                apply_transcript_done(
                    &mut self.entries,
                    "assistant",
                    text,
                    self.new_output_entry,
                    protocol,
                );
                self.new_output_entry = protocol == RealtimeProtocol::Frameless;
            }
            RealtimeEvent::AgentRequest {
                prompt, transcript, ..
            } => {
                append_handoff_input(&mut self.entries, prompt);
                *transcript = std::mem::take(&mut self.entries);
                self.new_input_entry = true;
                self.new_output_entry = true;
            }
            RealtimeEvent::ResponseStarted => self.new_output_entry = true,
            RealtimeEvent::SessionReady { .. }
            | RealtimeEvent::Audio(_)
            | RealtimeEvent::RemainSilent { .. }
            | RealtimeEvent::ResponseDone
            | RealtimeEvent::TranscriptTail(_)
            | RealtimeEvent::Error(_) => {}
        }
        truncate_active_transcript(&mut self.entries);
    }
}

fn truncate_active_transcript(entries: &mut Vec<RealtimeTranscriptEntry>) {
    let mut total_bytes = transcript_entries_bytes(entries);
    while total_bytes > MAX_ACTIVE_TRANSCRIPT_BYTES && entries.len() > 1 {
        total_bytes = total_bytes.saturating_sub(transcript_entry_bytes(&entries[0]));
        entries.remove(0);
    }
    let Some(entry) = entries.first_mut() else {
        return;
    };
    let entry_overhead = entry.role.len() + 3;
    let max_text_bytes = MAX_ACTIVE_TRANSCRIPT_BYTES.saturating_sub(entry_overhead);
    if entry.text.len() <= max_text_bytes {
        return;
    }
    let mut start = entry
        .text
        .len()
        .saturating_sub(max_text_bytes.saturating_sub(TRUNCATED_TRANSCRIPT_PREFIX.len()));
    while !entry.text.is_char_boundary(start) {
        start += 1;
    }
    entry.text = format!("{TRUNCATED_TRANSCRIPT_PREFIX}{}", &entry.text[start..]);
}

fn transcript_entries_bytes(entries: &[RealtimeTranscriptEntry]) -> usize {
    entries.iter().map(transcript_entry_bytes).sum()
}

const fn transcript_entry_bytes(entry: &RealtimeTranscriptEntry) -> usize {
    entry.role.len() + entry.text.len() + 3
}

fn append_transcript_delta(
    entries: &mut Vec<RealtimeTranscriptEntry>,
    role: &str,
    delta: &str,
    force_new: bool,
    protocol: RealtimeProtocol,
) {
    if delta.is_empty() {
        return;
    }
    let entry = if protocol == RealtimeProtocol::Frameless {
        entries.iter_mut().rev().find(|entry| entry.role == role)
    } else {
        entries.last_mut().filter(|entry| entry.role == role)
    };
    if !force_new && let Some(last) = entry {
        last.text.push_str(delta);
        return;
    }
    entries.push(RealtimeTranscriptEntry {
        role: role.to_owned(),
        text: delta.to_owned(),
    });
}

fn apply_transcript_done(
    entries: &mut Vec<RealtimeTranscriptEntry>,
    role: &str,
    text: &str,
    force_new: bool,
    protocol: RealtimeProtocol,
) {
    if text.is_empty() {
        return;
    }
    let entry = if protocol == RealtimeProtocol::Frameless {
        entries.iter_mut().rev().find(|entry| entry.role == role)
    } else {
        entries.last_mut().filter(|entry| entry.role == role)
    };
    if !force_new && let Some(last) = entry {
        if protocol != RealtimeProtocol::Frameless || text.starts_with(&last.text) {
            last.text = text.to_owned();
        }
        return;
    }
    entries.push(RealtimeTranscriptEntry {
        role: role.to_owned(),
        text: text.to_owned(),
    });
}

fn append_handoff_input(entries: &mut Vec<RealtimeTranscriptEntry>, input: &str) {
    let input = input.trim();
    if input.is_empty()
        || entries
            .iter()
            .any(|entry| entry.role == "user" && entry.text.trim() == input)
    {
        return;
    }
    entries.push(RealtimeTranscriptEntry {
        role: "user".to_owned(),
        text: input.to_owned(),
    });
}

fn parse_event_value(
    value: &Value,
    protocol: RealtimeProtocol,
) -> Result<Option<RealtimeEvent>, RealtimeError> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let event = match protocol {
        RealtimeProtocol::V1 => parse_v1_event(value, kind)?,
        RealtimeProtocol::Direct => parse_direct_event(value, kind)?,
        RealtimeProtocol::Frameless => parse_frameless_event(value, kind),
    };
    Ok(event)
}

fn parse_v1_event(value: &Value, kind: &str) -> Result<Option<RealtimeEvent>, RealtimeError> {
    let event = match kind {
        "session.updated" => {
            value
                .pointer("/session/id")
                .and_then(Value::as_str)
                .map(|session_id| RealtimeEvent::SessionReady {
                    session_id: session_id.to_owned(),
                })
        }
        "conversation.output_audio.delta" => {
            let encoded = value
                .get("delta")
                .or_else(|| value.get("data"))
                .and_then(Value::as_str)
                .unwrap_or_default();
            let data = STANDARD
                .decode(encoded)
                .map_err(|error| RealtimeError::Message(error.to_string()))?;
            Some(RealtimeEvent::Audio(RealtimeAudio::pcm16_le(data)?))
        }
        "conversation.input_transcript.delta"
        | "conversation.item.input_audio_transcription.delta" => {
            string_field(value, "delta").map(RealtimeEvent::InputTranscriptDelta)
        }
        "conversation.input_transcript.turn_marked"
        | "conversation.item.input_audio_transcription.completed" => {
            string_field(value, "transcript").map(RealtimeEvent::InputTranscriptDone)
        }
        "conversation.output_transcript.delta"
        | "response.output_text.delta"
        | "response.output_audio_transcript.delta" => {
            string_field(value, "delta").map(RealtimeEvent::OutputTranscriptDelta)
        }
        "response.output_audio_transcript.done" => {
            string_field(value, "transcript").map(RealtimeEvent::OutputTranscriptDone)
        }
        "conversation.handoff.requested" => {
            let call_id = value.get("handoff_id").and_then(Value::as_str);
            let prompt = value.get("input_transcript").and_then(Value::as_str);
            call_id
                .zip(prompt)
                .map(|(call_id, prompt)| RealtimeEvent::AgentRequest {
                    call_id: call_id.to_owned(),
                    prompt: prompt.to_owned(),
                    transcript: Vec::new(),
                })
        }
        "error" => Some(parse_error(value)),
        _ => None,
    };
    Ok(event)
}

fn parse_direct_event(value: &Value, kind: &str) -> Result<Option<RealtimeEvent>, RealtimeError> {
    let event = match kind {
        "session.updated" => {
            value
                .pointer("/session/id")
                .and_then(Value::as_str)
                .map(|session_id| RealtimeEvent::SessionReady {
                    session_id: session_id.to_owned(),
                })
        }
        "input_audio_buffer.speech_started" => Some(RealtimeEvent::SpeechStarted),
        "conversation.input_transcript.delta" => {
            string_field(value, "delta").map(RealtimeEvent::InputTranscriptDelta)
        }
        "conversation.input_transcript.turn_marked" => {
            string_field(value, "transcript").map(RealtimeEvent::InputTranscriptDone)
        }
        "conversation.item.input_audio_transcription.delta" => {
            string_field(value, "delta").map(RealtimeEvent::InputTranscriptDelta)
        }
        "conversation.item.input_audio_transcription.completed" => {
            string_field(value, "transcript").map(RealtimeEvent::InputTranscriptDone)
        }
        "response.output_text.delta" | "response.output_audio_transcript.delta" => {
            string_field(value, "delta").map(RealtimeEvent::OutputTranscriptDelta)
        }
        "conversation.output_transcript.delta" => {
            string_field(value, "delta").map(RealtimeEvent::OutputTranscriptDelta)
        }
        "response.output_text.done" => {
            string_field(value, "text").map(RealtimeEvent::OutputTranscriptDone)
        }
        "response.output_audio_transcript.done" => {
            string_field(value, "transcript").map(RealtimeEvent::OutputTranscriptDone)
        }
        "response.output_audio.delta" | "response.audio.delta" => {
            let encoded = value
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let data = STANDARD
                .decode(encoded)
                .map_err(|error| RealtimeError::Message(error.to_string()))?;
            Some(RealtimeEvent::Audio(RealtimeAudio::pcm16_le(data)?))
        }
        "conversation.item.done" => parse_completed_item(value),
        "response.created" => Some(RealtimeEvent::ResponseStarted),
        "response.done" | "response.cancelled" => Some(RealtimeEvent::ResponseDone),
        "error" => Some(RealtimeEvent::Error(
            value
                .pointer("/error/message")
                .or_else(|| value.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("unknown GPT Realtime error")
                .to_owned(),
        )),
        _ => None,
    };
    Ok(event)
}

fn parse_frameless_event(value: &Value, kind: &str) -> Option<RealtimeEvent> {
    match kind {
        "session.started" | "session.updated" => value
            .pointer("/session/id")
            .and_then(Value::as_str)
            .map(|session_id| RealtimeEvent::SessionReady {
                session_id: session_id.to_owned(),
            }),
        "input_transcript.added" => value
            .pointer("/item/text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .map(RealtimeEvent::InputTranscriptDelta),
        "output_transcript.added" => value
            .pointer("/item/text")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .map(RealtimeEvent::OutputTranscriptDelta),
        "turn.done" => parse_frameless_turn(value),
        "delegation.created" => parse_frameless_delegation(value),
        "output_audio.delta" => value
            .get("audio")
            .or_else(|| value.get("delta"))
            .and_then(Value::as_str)
            .and_then(|audio| STANDARD.decode(audio).ok())
            .and_then(|audio| RealtimeAudio::pcm16_le(audio).ok())
            .map(RealtimeEvent::Audio),
        "error" => Some(parse_error(value)),
        _ => None,
    }
}

fn parse_frameless_turn(value: &Value) -> Option<RealtimeEvent> {
    let role = value.pointer("/turn/role")?.as_str()?;
    let transcript = value.pointer("/turn/transcript")?.as_str()?.to_owned();
    match role {
        "user" => Some(RealtimeEvent::InputTranscriptDone(transcript)),
        "assistant" => Some(RealtimeEvent::OutputTranscriptDone(transcript)),
        _ => None,
    }
}

fn parse_frameless_delegation(value: &Value) -> Option<RealtimeEvent> {
    let item = value.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("delegation")
        || item.get("target").and_then(Value::as_str) != Some("client")
    {
        return None;
    }
    let prompt = item
        .get("content")?
        .as_array()?
        .iter()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("input_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();
    Some(RealtimeEvent::AgentRequest {
        call_id: item.get("id")?.as_str()?.to_owned(),
        prompt,
        transcript: Vec::new(),
    })
}

fn parse_error(value: &Value) -> RealtimeEvent {
    RealtimeEvent::Error(
        value
            .pointer("/error/message")
            .or_else(|| value.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("unknown GPT Realtime error")
            .to_owned(),
    )
}

fn parse_completed_item(value: &Value) -> Option<RealtimeEvent> {
    let item = value.get("item")?;
    if item.get("type").and_then(Value::as_str) != Some("function_call") {
        return None;
    }
    let name = item.get("name").and_then(Value::as_str)?;
    let call_id = item
        .get("call_id")
        .or_else(|| item.get("id"))
        .and_then(Value::as_str)?
        .to_owned();
    match name {
        BACKGROUND_AGENT_TOOL => Some(RealtimeEvent::AgentRequest {
            call_id,
            prompt: delegated_prompt(item.get("arguments").and_then(Value::as_str)),
            transcript: Vec::new(),
        }),
        REMAIN_SILENT_TOOL => Some(RealtimeEvent::RemainSilent { call_id }),
        _ => None,
    }
}

fn delegated_prompt(arguments: Option<&str>) -> String {
    let Some(arguments) = arguments else {
        return String::new();
    };
    if let Ok(value) = serde_json::from_str::<Value>(arguments)
        && let Some(object) = value.as_object()
    {
        for key in ["input_transcript", "input", "text", "prompt", "query"] {
            if let Some(value) = object.get(key).and_then(Value::as_str) {
                let value = value.trim();
                if !value.is_empty() {
                    return value.to_owned();
                }
            }
        }
    }
    arguments.to_owned()
}

fn string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(Value::as_str).map(str::to_owned)
}

fn realtime_endpoint(
    api_base_url: &str,
    model: &str,
    version: RealtimeVersion,
) -> Result<String, RealtimeError> {
    let mut url =
        Url::parse(api_base_url).map_err(|error| RealtimeError::InvalidUrl(error.to_string()))?;
    match url.scheme() {
        "https" => url
            .set_scheme("wss")
            .map_err(|()| RealtimeError::InvalidUrl("could not select wss".to_owned()))?,
        "http" => url
            .set_scheme("ws")
            .map_err(|()| RealtimeError::InvalidUrl("could not select ws".to_owned()))?,
        "wss" | "ws" => {}
        scheme => {
            return Err(RealtimeError::InvalidUrl(format!(
                "unsupported URL scheme {scheme}"
            )));
        }
    }
    let path = url.path().to_owned();
    if version == RealtimeVersion::V3 {
        if path.is_empty() || path == "/" || path == "/v1" || path == "/v1/" {
            url.set_path("/v1/live");
        } else if let Some(prefix) = path.trim_end_matches('/').strip_suffix("/realtime") {
            url.set_path(&format!("{prefix}/live"));
        } else if path.ends_with("/live/") {
            url.set_path(path.trim_end_matches('/'));
        }
    } else {
        if path.is_empty() || path == "/" {
            url.set_path("/v1/realtime");
        } else if path.ends_with("/realtime/") {
            url.set_path(path.trim_end_matches('/'));
        } else if path.ends_with("/v1") {
            url.set_path(&format!("{path}/realtime"));
        } else if path.ends_with("/v1/") {
            url.set_path(&format!("{path}realtime"));
        }
    }
    let mut query = url.query_pairs_mut();
    if version == RealtimeVersion::V1 {
        query.append_pair("intent", "quicksilver");
    }
    query.append_pair("model", model);
    drop(query);
    Ok(url.into())
}

fn map_websocket_error(error: WebSocketError) -> RealtimeError {
    RealtimeError::WebSocket(error.to_string())
}
