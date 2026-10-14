use super::{
    model::{AgentDescriptor, AgentId, AgentStatus},
    runtime::{ChildSession, OutputContract},
};
use nanocode_agent::{Lineage, ModelSelection, SessionCheckpoint, Thinking};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

#[cfg(not(target_family = "wasm"))]
pub type SubagentStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[cfg(target_family = "wasm")]
pub type SubagentStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

pub trait SubagentStore: Send + Sync {
    fn pending_turn<'a>(
        &'a self,
        _session_id: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<Option<(String, Value)>>> {
        Box::pin(async { Ok(None) })
    }

    fn load<'a>(
        &'a self,
        root_session_id: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<Option<String>>>;

    fn save<'a>(
        &'a self,
        root_session_id: &'a str,
        payload: String,
        records: Vec<Arc<str>>,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>>;

    fn record_session<'a>(
        &'a self,
        _root_session_id: &'a str,
        _checkpoint: SessionCheckpoint,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn load_record<'a>(
        &'a self,
        root_session_id: &'a str,
        key: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<String>>;
}

#[derive(Clone, Default)]
pub struct MemorySubagentStore {
    values: Arc<Mutex<HashMap<String, String>>>,
    sessions: Arc<Mutex<HashMap<String, SessionCheckpoint>>>,
    records: Arc<Mutex<MemoryRecords>>,
}

type MemoryRecords = HashMap<(String, String), Arc<str>>;

impl MemorySubagentStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn session(&self, session_id: &str) -> Option<SessionCheckpoint> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned()
    }

    #[must_use]
    pub fn sessions(&self) -> Vec<String> {
        let mut ids = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }
}

impl SubagentStore for MemorySubagentStore {
    fn load<'a>(
        &'a self,
        root_session_id: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<Option<String>>> {
        Box::pin(async move {
            Ok(self
                .values
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(root_session_id)
                .cloned())
        })
    }

    fn save<'a>(
        &'a self,
        root_session_id: &'a str,
        payload: String,
        records: Vec<Arc<str>>,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>> {
        Box::pin(async move {
            let mut stored = self
                .records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for record in records {
                stored.insert(
                    (root_session_id.to_owned(), checkpoint_key(&record)),
                    record,
                );
            }
            self.values
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(root_session_id.to_owned(), payload);
            Ok(())
        })
    }

    fn record_session<'a>(
        &'a self,
        _root_session_id: &'a str,
        checkpoint: SessionCheckpoint,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>> {
        Box::pin(async move {
            self.sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(checkpoint.session_id().to_owned(), checkpoint);
            Ok(())
        })
    }

    fn load_record<'a>(
        &'a self,
        root_session_id: &'a str,
        key: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<String>> {
        Box::pin(async move {
            self.records
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(&(root_session_id.to_owned(), key.to_owned()))
                .map(|record| record.to_string())
                .ok_or_else(|| std::io::Error::other(format!("missing subagent checkpoint {key}")))
        })
    }
}

pub(super) const JOURNAL_VERSION: u32 = 3;

pub(super) const MIN_JOURNAL_VERSION: u32 = 1;

#[must_use]
pub fn checkpoint_key(json: &str) -> String {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut key = String::with_capacity(64);
    for byte in Sha256::digest(json.as_bytes()) {
        key.push(HEX[usize::from(byte >> 4)] as char);
        key.push(HEX[usize::from(byte & 15)] as char);
    }
    key
}

#[derive(Clone)]
pub(super) struct JournalCheckpoint {
    pub(super) key: Arc<str>,

    pub(super) pending: Option<Arc<str>>,

    pub(super) checkpoint: Option<SessionCheckpoint>,
}

impl JournalCheckpoint {
    pub(super) fn encode(checkpoint: &SessionCheckpoint) -> std::io::Result<Self> {
        let json: Arc<str> = serde_json::to_string(checkpoint)
            .map_err(std::io::Error::other)?
            .into();
        Ok(Self {
            key: checkpoint_key(&json).into(),
            pending: Some(json),
            checkpoint: Some(checkpoint.clone()),
        })
    }

    pub(super) fn stored(key: String, checkpoint: Option<SessionCheckpoint>) -> Self {
        Self {
            key: key.into(),
            pending: None,
            checkpoint,
        }
    }
}

pub(super) fn decode_checkpoint(json: &str, key: &str) -> std::io::Result<SessionCheckpoint> {
    if checkpoint_key(json) != key {
        return Err(std::io::Error::other(format!(
            "subagent checkpoint {key} does not match its record"
        )));
    }
    let checkpoint: SessionCheckpoint = serde_json::from_str(json).map_err(|error| {
        std::io::Error::other(format!("invalid subagent checkpoint {key}: {error}"))
    })?;
    checkpoint.validate().map_err(std::io::Error::other)?;
    Ok(checkpoint)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyCheckpointRecord {
    #[serde(default)]
    checkpoint: Option<Value>,
    #[serde(default)]
    native_checkpoint: Option<PersistedNative>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct PersistedScope {
    pub(super) version: u32,
    pub(super) agents: Vec<PersistedAgent>,
    #[serde(default)]
    pub(super) messages: PersistedMessages,
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct PersistedMessages {
    pub(super) next_message_id: u64,
    pub(super) records: Vec<String>,
    pub(super) pending: Vec<super::model::MessageId>,
}

impl PersistedMessages {
    pub(super) async fn hydrate(
        self,
        store: &dyn SubagentStore,
        root_session_id: &str,
    ) -> std::io::Result<super::message::MessageThreads> {
        let mut messages = Vec::with_capacity(self.records.len());
        for key in self.records {
            let json = store.load_record(root_session_id, &key).await?;
            if checkpoint_key(&json) != key {
                return Err(std::io::Error::other(
                    "persisted message record does not match its key",
                ));
            }
            messages.push(serde_json::from_str(&json).map_err(std::io::Error::other)?);
        }
        super::message::MessageThreads::restore(self.next_message_id, messages, self.pending)
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct PersistedAgent {
    pub(super) descriptor: AgentDescriptor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) binding_task: Option<String>,
    pub(super) status: AgentStatus,
    pub(super) output_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) host_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) last_output: Option<Value>,
    #[serde(default)]
    pub(super) next_instruction_revision: u64,

    #[serde(default)]
    pub(super) turn_in_flight: bool,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub(super) resume_attempts: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) checkpoint: Option<JournaledCheckpoint>,

    #[serde(default, skip_serializing)]
    pub(super) native_checkpoint: Option<PersistedNative>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) checkpoint_ref: Option<String>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(super) in_flight_calls: Vec<InFlightCall>,

    #[serde(default, skip_serializing_if = "is_zero")]
    pub(super) in_flight_omitted: u32,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) accepted_output: Option<Value>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) accepted_revision: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) pending_completion: Option<u64>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) settled_revision: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct InFlightCall {
    pub(super) call_id: String,
    pub(super) tool: String,

    pub(super) arguments: String,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) result_recorded: bool,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(super) yielded: bool,

    #[serde(skip)]
    pub(super) parent_call_id: Option<String>,

    #[serde(skip)]
    pub(super) live: bool,

    #[serde(skip)]
    pub(super) settled: bool,

    #[serde(skip)]
    pub(super) committable: bool,
}

#[derive(Clone, Debug, Default)]
pub(super) struct OmittedProgress {
    pub(super) settled: u32,

    pub(super) committable: u32,

    pub(super) nested: HashMap<String, u32>,
}

impl InFlightCall {
    pub(super) fn new(
        call_id: &str,
        tool: &str,
        arguments: Option<&Value>,
        parent_call_id: Option<&str>,
    ) -> Self {
        let arguments = match arguments {
            Some(Value::String(text)) => text.clone(),
            Some(value) => value.to_string(),
            None => String::new(),
        };
        Self {
            call_id: call_id.to_owned(),
            tool: tool.to_owned(),
            arguments,
            result_recorded: false,
            yielded: false,
            parent_call_id: parent_call_id.map(str::to_owned),
            live: true,
            settled: false,
            committable: false,
        }
    }
}

pub(super) fn retain_call(calls: &mut Vec<InFlightCall>, call: InFlightCall) {
    calls.retain(|existing| existing.call_id != call.call_id);
    calls.push(call);
}

pub(super) fn in_flight_evidence(calls: &[InFlightCall], omitted: u32) -> Option<String> {
    if calls.is_empty() && omitted == 0 {
        return None;
    }
    let mut lines = calls
        .iter()
        .map(|call| {
            let state = if call.yielded {
                "yielded; still running at the restart"
            } else if call.result_recorded {
                "a result was observed before the restart"
            } else {
                "started; no result was observed"
            };
            format!(
                "- {} (call_id {}; {state}): {}",
                call.tool, call.call_id, call.arguments
            )
        })
        .collect::<Vec<_>>();
    if omitted > 0 {
        lines.push(format!(
            "- {omitted} additional observed call(s) omitted; this is not a complete history."
        ));
    }
    Some(format!(
        "Tool calls observed during the interrupted turn whose results are not known to be in \
         your restored history (calls already committed to it are not listed); any that are \
         absent from it may or may not have taken effect. This summary is \
         not a receipt: consult your retained history and reconcile tool receipts or external \
         effects before repeating any of them:\n{}",
        lines.join("\n")
    ))
}

pub(super) enum JournaledCheckpoint {
    Current(SessionCheckpoint),

    LegacyCodex(Value),
}

impl Serialize for JournaledCheckpoint {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Current(checkpoint) => checkpoint.serialize(serializer),
            Self::LegacyCodex(value) => value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for JournaledCheckpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        if value.get("format").is_some() {
            serde_json::from_value(value)
                .map(Self::Current)
                .map_err(serde::de::Error::custom)
        } else {
            Ok(Self::LegacyCodex(value))
        }
    }
}

impl JournaledCheckpoint {
    fn checkpoint(&self, lineage: &Lineage) -> std::io::Result<SessionCheckpoint> {
        match self {
            Self::Current(checkpoint) => {
                checkpoint.validate().map_err(std::io::Error::other)?;
                Ok(checkpoint.clone())
            }
            Self::LegacyCodex(value) => Ok(legacy_codex_checkpoint(value, lineage)?),
        }
    }
}

#[derive(Debug)]
pub(super) enum LegacyCheckpointError {
    Codex(String),

    Native(String),
}

impl std::fmt::Display for LegacyCheckpointError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codex(detail) => {
                write!(formatter, "invalid journaled Codex checkpoint: {detail}")
            }
            Self::Native(detail) => {
                write!(formatter, "invalid journaled native checkpoint: {detail}")
            }
        }
    }
}

impl std::error::Error for LegacyCheckpointError {}

impl From<LegacyCheckpointError> for std::io::Error {
    fn from(error: LegacyCheckpointError) -> Self {
        Self::new(std::io::ErrorKind::InvalidData, error)
    }
}

fn legacy_field<T: serde::de::DeserializeOwned>(
    fields: &mut serde_json::Map<String, Value>,
    name: &str,
) -> Result<T, LegacyCheckpointError> {
    let value = fields
        .remove(name)
        .ok_or_else(|| LegacyCheckpointError::Codex(format!("missing {name}")))?;
    serde_json::from_value(value)
        .map_err(|error| LegacyCheckpointError::Codex(format!("invalid {name}: {error}")))
}

fn legacy_codex_checkpoint(
    value: &Value,
    lineage: &Lineage,
) -> Result<SessionCheckpoint, LegacyCheckpointError> {
    let Value::Object(fields) = value else {
        return Err(LegacyCheckpointError::Codex("expected an object".into()));
    };
    let mut payload = fields.clone();
    let session_id: String = legacy_field(&mut payload, "session_id")?;
    let model: nanocode_agent::Model = legacy_field(&mut payload, "model")?;
    let thinking: Thinking = legacy_field(&mut payload, "thinking")?;
    let conversation = payload
        .get("conversation")
        .filter(|conversation| !conversation.is_null());
    let has_conversation = conversation.is_some();
    let conversation_id = match conversation {
        Some(conversation) => conversation
            .get("lineage_id")
            .and_then(Value::as_str)
            .ok_or_else(|| LegacyCheckpointError::Codex("conversation has no lineage".into()))?
            .to_owned(),
        None => session_id.clone(),
    };
    Ok(SessionCheckpoint::native(
        session_id,
        ModelSelection::Codex(model),
        thinking,
        lineage.clone(),
        conversation_id,
        None,
        has_conversation,
        Value::Object(payload),
    ))
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PersistedNative {
    model: String,
    session_id: String,
    thinking: Thinking,
    payload: String,
    has_conversation: bool,
}

impl PersistedNative {
    fn into_checkpoint(
        self,
        lineage: &Lineage,
    ) -> Result<SessionCheckpoint, LegacyCheckpointError> {
        let model = self.model.parse::<ModelSelection>().map_err(|error| {
            LegacyCheckpointError::Native(format!("invalid journaled model: {error}"))
        })?;
        if self.session_id.trim().is_empty() {
            return Err(LegacyCheckpointError::Native("empty session ID".into()));
        }
        let payload = serde_json::from_str(&self.payload).map_err(|error| {
            LegacyCheckpointError::Native(format!("payload is not JSON: {error}"))
        })?;
        Ok(SessionCheckpoint::native(
            self.session_id.clone(),
            model,
            self.thinking,
            lineage.clone(),
            self.session_id,
            None,
            self.has_conversation,
            payload,
        ))
    }
}

pub(super) struct JournalStore(pub(super) Arc<dyn nanocode_agent::backend::ChildJournalStore>);

impl SubagentStore for JournalStore {
    fn pending_turn<'a>(
        &'a self,
        session_id: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<Option<(String, Value)>>> {
        self.0.pending_turn(session_id.to_owned())
    }

    fn load<'a>(
        &'a self,
        _root_session_id: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<Option<String>>> {
        self.0.load()
    }

    fn save<'a>(
        &'a self,
        _root_session_id: &'a str,
        payload: String,
        records: Vec<Arc<str>>,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>> {
        self.0.save(payload, records)
    }

    fn load_record<'a>(
        &'a self,
        _root_session_id: &'a str,
        key: &'a str,
    ) -> SubagentStoreFuture<'a, std::io::Result<String>> {
        self.0.load_record(key.to_owned())
    }

    fn record_session<'a>(
        &'a self,
        _root_session_id: &'a str,
        checkpoint: SessionCheckpoint,
    ) -> SubagentStoreFuture<'a, std::io::Result<()>> {
        self.0.record_child(checkpoint)
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RestoreReport {
    pub restored: usize,

    pub interrupted: Vec<AgentId>,

    pub unrecoverable: Vec<AgentId>,

    pub completed: Vec<AgentId>,
}

pub(super) const RESUME_MESSAGE: &str = "The sub-agent runtime restarted while your previous \
turn was running. Your conversation was restored from its latest committed checkpoint: every \
step in your history completed, but a tool call that was still running at the restart is not \
shown and may or may not have taken effect. Inspect the current workspace state where that \
matters, do not repeat side effects that already happened, and continue your delegated task \
from where your history ends. Submit a result only once the whole delegated task is complete.";

pub(super) fn persist_agent(
    session: &ChildSession,
    checkpoint: Option<&JournalCheckpoint>,
    records: &mut Vec<(Arc<str>, Arc<str>)>,
) -> std::io::Result<PersistedAgent> {
    let fallback = match (checkpoint, &session.stored_runtime) {
        (None, Some(snapshot)) => Some(JournalCheckpoint::encode(snapshot)?),
        _ => None,
    };
    let checkpoint_ref = checkpoint.or(fallback.as_ref()).map(|checkpoint| {
        if let Some(json) = &checkpoint.pending {
            records.push((Arc::clone(&checkpoint.key), Arc::clone(json)));
        }
        checkpoint.key.to_string()
    });
    Ok(PersistedAgent {
        descriptor: session.descriptor.clone(),
        binding_task: Some(session.binding_task.clone()),
        status: session.status.clone(),
        output_schema: session.output_schema.clone(),
        host_context: session.host_context.as_deref().map(str::to_owned),
        last_output: session.last_output.clone(),
        next_instruction_revision: session.next_instruction_revision,
        turn_in_flight: session.active || matches!(session.status, AgentStatus::Running),
        resume_attempts: session.resume_attempts,
        checkpoint: None,
        native_checkpoint: None,
        checkpoint_ref,
        in_flight_calls: session.in_flight_calls.clone(),
        in_flight_omitted: session.in_flight_omitted,
        accepted_output: session
            .active
            .then(|| session.submitted_output.clone())
            .flatten(),
        accepted_revision: (session.active && session.submitted_output.is_some())
            .then_some(session.active_instruction_revision)
            .flatten(),
        pending_completion: session.pending_completion,
        settled_revision: session.settled_revision,
    })
}

impl PersistedAgent {
    pub(super) async fn hydrate(
        &mut self,
        store: &dyn SubagentStore,
        root_session_id: &str,
    ) -> std::io::Result<Option<JournalCheckpoint>> {
        let Some(key) = self.checkpoint_ref.take() else {
            return Ok(None);
        };
        let json = store.load_record(root_session_id, &key).await?;
        if checkpoint_key(&json) != key {
            return Err(std::io::Error::other(format!(
                "subagent checkpoint {key} does not match its record"
            )));
        }
        let invalid = |error: serde_json::Error| {
            std::io::Error::other(format!("invalid subagent checkpoint {key}: {error}"))
        };
        let record: Value = serde_json::from_str(&json).map_err(invalid)?;
        if record.get("format").is_some() {
            let checkpoint: SessionCheckpoint = serde_json::from_value(record).map_err(invalid)?;
            checkpoint.validate().map_err(std::io::Error::other)?;
            self.checkpoint = Some(JournaledCheckpoint::Current(checkpoint.clone()));
            return Ok(Some(JournalCheckpoint::stored(key, Some(checkpoint))));
        }
        let record: LegacyCheckpointRecord = serde_json::from_value(record).map_err(invalid)?;
        self.checkpoint = record.checkpoint.map(JournaledCheckpoint::LegacyCodex);
        self.native_checkpoint = record.native_checkpoint;
        Ok(None)
    }

    pub(super) fn snapshot(&self, lineage: &Lineage) -> std::io::Result<Option<SessionCheckpoint>> {
        if let Some(checkpoint) = &self.checkpoint {
            return checkpoint.checkpoint(lineage).map(Some);
        }
        Ok(self
            .native_checkpoint
            .clone()
            .map(|native| native.into_checkpoint(lineage))
            .transpose()?)
    }
}

pub(super) fn journaled_lineage(
    root_session_id: &str,
    agents: &[PersistedAgent],
    id: AgentId,
) -> Lineage {
    let parent_of = |id: AgentId| {
        agents
            .iter()
            .find(|agent| agent.descriptor.id == id)
            .and_then(|agent| agent.descriptor.parent)
    };
    let parent_session_id = parent_of(id)
        .and_then(|parent| agents.iter().find(|agent| agent.descriptor.id == parent))
        .map_or_else(
            || root_session_id.to_owned(),
            |parent| parent.descriptor.session_id.clone(),
        );
    let mut depth = 1;
    let mut current = parent_of(id);
    while let Some(parent) = current
        && depth <= agents.len()
    {
        depth += 1;
        current = parent_of(parent);
    }
    Lineage::new(
        root_session_id,
        Some(parent_session_id),
        nanocode_agent::Origin::Subagent,
        u32::try_from(depth).unwrap_or(u32::MAX),
    )
}

const fn is_zero(value: &u32) -> bool {
    *value == 0
}

pub(super) fn restored_session(
    agent: PersistedAgent,
    snapshot: Option<SessionCheckpoint>,
) -> std::io::Result<(ChildSession, bool, bool)> {
    let contract = OutputContract::compile(&agent.output_schema)?;
    let accepted_revision = agent.accepted_revision;
    let pending_completion = agent.pending_completion;
    let settled_revision = agent.settled_revision;
    let root_child = agent.descriptor.parent.is_none();

    let journaled = snapshot.is_none() && agent.checkpoint_ref.is_some();
    let recoverable = snapshot.is_some() || journaled;
    let terminal = matches!(agent.status, AgentStatus::Closing | AgentStatus::Closed);
    let running = !terminal
        && (agent.turn_in_flight
            || matches!(agent.status, AgentStatus::Running | AgentStatus::Pending));

    let accepted = running.then_some(agent.accepted_output).flatten();
    let in_flight = running && accepted.is_none();
    let completed_by_acceptance = accepted.is_some();
    let evidence = in_flight_evidence(&agent.in_flight_calls, agent.in_flight_omitted);
    let status = if terminal {
        AgentStatus::Closed
    } else if in_flight && !recoverable {
        let mut error = "subagent could not be restored after a runtime restart: no portable \
                         checkpoint was available"
            .to_owned();
        if let Some(evidence) = &evidence {
            error.push_str("\n\n");
            error.push_str(evidence);
        }
        AgentStatus::Failed { error }
    } else if in_flight {
        AgentStatus::Interrupted
    } else if let Some(output) = &accepted {
        AgentStatus::Completed {
            output: output.clone(),
        }
    } else {
        agent.status
    };
    let resume = in_flight && recoverable;
    let resume_attempts = if resume {
        agent
            .resume_attempts
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("subagent recovery attempt counter overflow"))?
    } else {
        0
    };
    let binding_task = agent.binding_task;
    let mut session = ChildSession::restored(
        agent.descriptor,
        agent.host_context.map(Arc::from),
        status,
        contract,
        agent.output_schema,
        snapshot,
        agent.next_instruction_revision,
        accepted.or(agent.last_output),
    );
    if let Some(task) = binding_task {
        session.binding_task = task;
    }
    session.resume_attempts = resume_attempts;

    let accepted_revision = accepted_revision.unwrap_or(agent.next_instruction_revision);
    session.settled_revision = if completed_by_acceptance {
        Some(accepted_revision)
    } else {
        settled_revision
    };
    session.pending_completion = if !root_child {
        None
    } else if completed_by_acceptance {
        Some(accepted_revision)
    } else {
        pending_completion
    };
    session.journaled_runtime = journaled;

    if in_flight {
        session.in_flight_calls = agent.in_flight_calls;
        session.in_flight_omitted = agent.in_flight_omitted;
    }
    Ok((session, resume, !recoverable && !terminal))
}
