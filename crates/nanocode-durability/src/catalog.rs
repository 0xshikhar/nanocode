use std::path::PathBuf;

use nanocode_agent::{HarnessFamily, Lineage, ModelSelection};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecord {
    pub session_id: String,

    pub model: ModelSelection,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<PathBuf>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,

    pub lineage: Lineage,

    #[serde(default)]
    pub created_at_ms: u64,

    #[serde(default)]
    pub updated_at_ms: u64,

    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub history_pruned: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<BranchBoundary>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) start_checkpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) initial: Option<InitialSettings>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InitialSettings {
    pub(crate) thinking: nanocode_agent::Thinking,
    pub(crate) service_tier: nanocode_agent::ServiceTier,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchBoundary {
    pub source_session_id: String,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub through_turn: Option<String>,
}

impl SessionRecord {
    #[must_use]
    pub fn root(
        session_id: impl Into<String>,
        model: ModelSelection,
        workspace: Option<PathBuf>,
    ) -> Self {
        let session_id = session_id.into();
        let now = now_ms();
        Self {
            lineage: Lineage::root(session_id.clone()),
            session_id,
            model,
            workspace,
            title: None,
            created_at_ms: now,
            updated_at_ms: now,
            history_pruned: false,
            branch: None,
            start_checkpoint: None,
            initial: None,
        }
    }

    #[must_use]
    pub fn family(&self) -> HarnessFamily {
        self.model.family()
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn with_lineage(mut self, lineage: Lineage) -> Self {
        self.lineage = lineage;
        self
    }

    #[must_use]
    pub fn derive(&self, session_id: impl Into<String>, origin: nanocode_agent::Origin) -> Self {
        let session_id = session_id.into();
        let now = now_ms();
        Self {
            lineage: Lineage::child_of(&self.lineage, self.session_id.clone(), origin),
            session_id,
            model: self.model.clone(),
            workspace: self.workspace.clone(),
            title: self.title.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            history_pruned: false,
            branch: None,
            start_checkpoint: None,
            initial: None,
        }
    }

    pub(crate) fn touch(&mut self) {
        let now = now_ms();
        if now > self.updated_at_ms {
            self.updated_at_ms = now;
        }
    }

    pub(crate) fn merged(&self, next: Self) -> Self {
        let lineage = if next.lineage == Lineage::root(next.session_id.clone()) {
            self.lineage.clone()
        } else {
            next.lineage
        };
        Self {
            session_id: next.session_id,
            model: next.model,
            workspace: next.workspace.or_else(|| self.workspace.clone()),
            title: next.title.or_else(|| self.title.clone()),
            lineage,
            created_at_ms: if self.created_at_ms == 0 {
                next.created_at_ms
            } else {
                self.created_at_ms
            },
            updated_at_ms: self.updated_at_ms,
            history_pruned: self.history_pruned || next.history_pruned,

            branch: self.branch.clone().or(next.branch),
            start_checkpoint: next
                .start_checkpoint
                .or_else(|| self.start_checkpoint.clone()),
            initial: next.initial.or(self.initial),
        }
    }
}

fn now_ms() -> u64 {
    #[cfg(not(target_family = "wasm"))]
    use std::time::{SystemTime, UNIX_EPOCH};
    #[cfg(target_family = "wasm")]
    use web_time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
        })
}

pub use store::*;

mod store {
    use std::{collections::HashSet, path::PathBuf};

    use nanocode_agent::{HarnessFamily, Lineage, ModelSelection, Origin};
    use nanocode_backend_codex::CodexCheckpointExt as _;
    use serde::Serialize;
    use serde_json::Value;

    use super::{BranchBoundary, SessionRecord};
    use crate::{
        DurableSession, DurableState, EncodedPayload, Error, OperationStatus, OwnerId, Result,
        StateStore, Transition, session::reduce_peeked, shared_store::SharedStore,
    };

    pub use nanocode_agent::session::TranscriptItem;

    pub const LIST_LIMIT: usize = 1000;

    #[derive(Clone, Debug, PartialEq, Eq, Serialize)]
    pub struct SessionSummary {
        pub record: SessionRecord,

        pub preview: Option<String>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
    #[serde(rename_all = "snake_case")]
    pub enum TurnStatus {
        Pending,

        Completed,

        Failed,

        Cancelled,
    }

    #[derive(Clone, Debug, PartialEq, Serialize)]
    pub struct StoredTurn {
        pub id: String,

        pub preview: Option<String>,

        pub status: TurnStatus,

        pub input: Value,
    }

    #[derive(Clone, Debug)]
    pub struct StoredSession {
        pub summary: SessionSummary,

        pub transcript: Vec<TranscriptItem>,

        pub turns: Vec<StoredTurn>,

        pub checkpoint: Option<Value>,
    }

    impl StoredSession {
        pub fn session_checkpoint(&self) -> Result<Option<nanocode_agent::SessionCheckpoint>> {
            let Some(checkpoint) = &self.checkpoint else {
                return Ok(None);
            };
            let record = &self.summary.record;
            if is_portable(checkpoint) {
                let portable = portable_checkpoint(checkpoint)?;
                portable
                    .require_family(record.family())
                    .map_err(|error| Error::InvalidState(error.to_string()))?;

                let rebound = nanocode_agent::SessionCheckpoint::native(
                    record.session_id.clone(),
                    portable.model(),
                    portable.thinking(),
                    record.lineage.clone(),
                    portable.conversation_id(),
                    portable.turn_id().map(str::to_owned),
                    portable.has_conversation(),
                    portable.payload().clone(),
                );
                rebound
                    .validate()
                    .map_err(|error| Error::InvalidState(error.to_string()))?;
                return Ok(Some(rebound));
            }
            match record.family() {
                family if family == HarnessFamily::Codex => {
                    let snapshot: nanocode_backend_codex::session::SessionSnapshot =
                        serde_json::from_value(checkpoint.clone())
                            .map_err(Error::InvalidPayload)?;
                    nanocode_agent::SessionCheckpoint::codex(
                        record.session_id.clone(),
                        record.lineage.clone(),
                        record.model.default_thinking(),
                        snapshot,
                    )
                    .map(Some)
                    .map_err(|error| Error::InvalidState(error.to_string()))
                }
                #[cfg(feature = "claude")]
                family if family == HarnessFamily::Claude => {
                    nanocode_backend_claude::session_checkpoint(
                        &record.session_id,
                        record.lineage.clone(),
                        checkpoint.clone(),
                    )
                    .map(Some)
                    .map_err(|error| Error::InvalidState(error.to_string()))
                }
                #[cfg(not(feature = "claude"))]
                family if family == HarnessFamily::Claude => Err(Error::InvalidState(
                    "portable Claude checkpoints require the claude feature of nanocode-durability"
                        .into(),
                )),
                family => Err(Error::InvalidState(format!(
                    "portable checkpoints for {family} require the owning backend adapter"
                ))),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub enum BranchPoint {
        Latest,

        Through(String),

        Before(String),
    }

    #[derive(Clone)]
    pub struct SessionStore {
        store: SharedStore,
    }

    impl SessionStore {
        #[must_use]
        pub fn same_store(&self, other: &Self) -> bool {
            self.store.same_store(&other.store)
        }

        pub fn new<S: StateStore + 'static>(store: S) -> Result<Self> {
            Ok(Self {
                store: SharedStore::new(store)?,
            })
        }

        #[cfg(all(feature = "sqlite", not(target_family = "wasm")))]
        #[must_use]
        pub fn path(codex_home: &std::path::Path) -> PathBuf {
            codex_home.join("sessions.sqlite")
        }

        #[cfg(all(feature = "sqlite", not(target_family = "wasm")))]
        pub fn open(codex_home: &std::path::Path) -> Result<Self> {
            std::fs::create_dir_all(codex_home).map_err(|error| {
                Error::Store(crate::StoreError::NotCommitted(format!(
                    "failed to create {}: {error}",
                    codex_home.display()
                )))
            })?;
            Self::open_path(Self::path(codex_home))
        }

        #[cfg(all(feature = "sqlite", not(target_family = "wasm")))]
        pub fn open_path(path: impl AsRef<std::path::Path>) -> Result<Self> {
            Self::new(crate::SqliteStore::open(path)?)
        }

        pub async fn list(&self) -> Result<Vec<SessionSummary>> {
            let ids = self.store.clone().list_states(usize::MAX).await?;
            let mut sessions = Vec::new();
            for id in ids {
                if is_child_journal(&id) {
                    continue;
                }
                if let Ok(Some(summary)) = self.summary(&id).await {
                    sessions.push(summary);
                }
            }
            sessions.sort_by_key(|session| std::cmp::Reverse(session.record.updated_at_ms));
            sessions.truncate(LIST_LIMIT);
            Ok(sessions)
        }

        pub async fn summary(&self, id: &str) -> Result<Option<SessionSummary>> {
            let Some(state) = self.peek(id).await? else {
                return Ok(None);
            };

            let created_child = state.session().is_some_and(|record| {
                matches!(
                    record.lineage.origin,
                    Origin::Fork | Origin::SideConversation | Origin::Subagent
                )
            });
            if state.operations().is_empty()
                && state.latest_checkpoint().is_none()
                && !created_child
            {
                return Ok(None);
            }
            let preview = self.first_prompt(id, &state).await;
            let record = match state.session() {
                Some(record) => record.clone(),
                None => {
                    let Some(checkpoint) = self.native_checkpoint(id, &state).await? else {
                        return Ok(None);
                    };
                    infer_record(id, &checkpoint)?
                }
            };
            Ok(Some(SessionSummary { record, preview }))
        }

        pub async fn load(&self, id: &str) -> Result<StoredSession> {
            let state = self.require(id).await?;
            let checkpoint = self.native_checkpoint(id, &state).await?;
            let record = match (state.session(), &checkpoint) {
                (Some(record), _) => record.clone(),
                (None, Some(checkpoint)) => infer_record(id, checkpoint)?,
                (None, None) => {
                    return Err(Error::SessionNotFound {
                        session_id: id.to_owned(),
                    });
                }
            };
            let turns = self.state_turns(id, &state).await?;
            let prompts = admitted_prompts(&turns, &state);
            let transcript = checkpoint
                .as_ref()
                .map(|checkpoint| transcript(checkpoint, record.family(), &prompts))
                .unwrap_or_default();
            let preview = turns
                .iter()
                .find_map(|turn| turn.preview.clone())
                .or_else(|| {
                    transcript.iter().find_map(|item| match item {
                        TranscriptItem::User(text) => Some(text.clone()),
                        _ => None,
                    })
                });
            Ok(StoredSession {
                summary: SessionSummary { record, preview },
                transcript,
                turns,
                checkpoint,
            })
        }

        pub async fn turns(&self, id: &str) -> Result<Vec<StoredTurn>> {
            let state = self.require(id).await?;
            self.state_turns(id, &state).await
        }

        pub async fn session(&self, record: SessionRecord) -> Result<DurableSession> {
            let session =
                DurableSession::open_shared(self.store.clone(), record.session_id.clone(), None)
                    .await?;
            session.describe(record).await?;
            Ok(session)
        }

        pub async fn recovery_checkpoint(
            &self,
            journal: nanocode_agent::SessionCheckpoint,
        ) -> Result<nanocode_agent::SessionCheckpoint> {
            journal
                .validate()
                .map_err(|error| Error::InvalidState(error.to_string()))?;
            let stored = match self.load(journal.session_id()).await {
                Ok(stored) => stored,
                Err(Error::SessionNotFound { .. }) => return Ok(journal),
                Err(error) => return Err(error),
            };
            let record = &stored.summary.record;
            if record.family() != journal.family() || record.lineage != *journal.lineage() {
                return Err(Error::InvalidState(
                    "child journal identity differs from its durable session".into(),
                ));
            }
            Ok(stored.session_checkpoint()?.unwrap_or(journal))
        }

        pub async fn resume(&self, id: &str) -> Result<DurableSession> {
            let state = self.require(id).await?;
            let session =
                DurableSession::open_shared(self.store.clone(), id.to_owned(), None).await?;
            if state.session().is_none()
                && let Some(checkpoint) = self.native_checkpoint(id, &state).await?
            {
                session.describe(infer_record(id, &checkpoint)?).await?;
            }
            Ok(session)
        }

        pub async fn branch(
            &self,
            id: &str,
            at: BranchPoint,
            workspace: Option<PathBuf>,
        ) -> Result<SessionSummary> {
            self.branch_with(id, at, workspace, |selected, _| Ok(selected))
                .await
        }

        pub async fn branch_with(
            &self,
            id: &str,
            at: BranchPoint,
            workspace: Option<PathBuf>,
            transform: impl FnOnce(Option<Value>, Option<&Value>) -> Result<Option<Value>>,
        ) -> Result<SessionSummary> {
            let state = self.require(id).await?;

            if !state.pending_operations().is_empty() {
                return Err(Error::InvalidState(
                    "conversation rewind refuses pending operations; settle or reconcile them first"
                        .into(),
                ));
            }
            let mut record = match state.session() {
                Some(record) => record.clone(),
                None => {
                    let checkpoint =
                        self.native_checkpoint(id, &state).await?.ok_or_else(|| {
                            Error::InvalidState(
                                "source session has no checkpoint to branch from".into(),
                            )
                        })?;
                    infer_record(id, &checkpoint)?
                }
            };
            let selected = match &at {
                BranchPoint::Latest => state
                    .latest_checkpoint()
                    .cloned()
                    .map(|payload| (id.to_owned(), payload)),
                BranchPoint::Through(turn) => {
                    let operation = state.operation(turn).ok_or_else(unknown_turn)?;
                    Some(
                        settled_checkpoint(&operation.status)
                            .cloned()
                            .map(|payload| (id.to_owned(), payload))
                            .ok_or_else(|| {
                                Error::InvalidState(
                                    "the selected turn has no settled checkpoint".into(),
                                )
                            })?,
                    )
                }
                BranchPoint::Before(turn) => {
                    let selected = state.operation(turn).ok_or_else(unknown_turn)?;
                    let prior = state
                        .operations()
                        .values()
                        .filter(|op| op.accepted_order < selected.accepted_order)
                        .max_by_key(|op| op.accepted_order);

                    let complete_history = state
                        .session()
                        .map_or(selected.accepted_order == 1, |record| {
                            !record.history_pruned
                        });
                    match prior {
                        None if complete_history => Self::start_checkpoint(id, &state)?,
                        Some(prior) => Some(
                            settled_checkpoint(&prior.status)
                                .cloned()
                                .map(|payload| (id.to_owned(), payload))
                                .ok_or_else(|| {
                                    Error::InvalidState(
                                        "checkpoint before the selected turn is unavailable".into(),
                                    )
                                })?,
                        ),
                        None => {
                            return Err(Error::InvalidState(
                                "retained history before the selected turn was pruned".into(),
                            ));
                        }
                    }
                }
            };
            let boundary = self.boundary(id, &state, &at).await?;
            let branch_id = uuid::Uuid::now_v7().to_string();
            let selected = match selected {
                Some((holder, payload)) => Some(
                    self.hydrate(&holder, &payload, Some(record.family()))
                        .await?,
                ),
                None => None,
            };
            let latest = self.native_checkpoint(id, &state).await?;
            let mut native = transform(selected, latest.as_ref())?;
            if let (Some(workspace), Some(Value::Object(object))) = (&workspace, native.as_mut())
                && object.contains_key("workspace")
            {
                object.insert(
                    "workspace".into(),
                    Value::String(workspace.display().to_string()),
                );
            }
            record = record.derive(branch_id, Origin::Branch);
            record.branch = Some(boundary);
            if workspace.is_some() {
                record.workspace = workspace;
            }
            let checkpoint = native
                .map(|value| encode_native(record.family(), value))
                .transpose()?;

            record.start_checkpoint = checkpoint.as_ref().map(|payload| payload.key.to_string());
            self.publish(record.clone(), checkpoint).await?;
            let preview = match &at {
                BranchPoint::Before(_) => None,
                _ => self.first_prompt(id, &state).await,
            };
            Ok(SessionSummary { record, preview })
        }

        fn start_checkpoint(
            id: &str,
            state: &DurableState,
        ) -> Result<Option<(String, EncodedPayload)>> {
            let Some(record) = state.session() else {
                return Ok(None);
            };
            if matches!(record.lineage.origin, Origin::Root | Origin::Subagent) {
                return Ok(None);
            }
            match (&record.start_checkpoint, &record.branch) {
                (Some(key), _) => Ok(Some((id.to_owned(), EncodedPayload::from_key(key)))),
                (None, Some(_)) => Ok(None),
                (None, None) => Err(Error::InvalidState(
                    "history before this session's first turn is held by its source session, which this older session record does not pin".into(),
                )),
            }
        }

        async fn boundary(
            &self,
            id: &str,
            state: &DurableState,
            at: &BranchPoint,
        ) -> Result<BranchBoundary> {
            let order = |turn: &str| state.operation(turn).map(|op| op.accepted_order);
            let limit = match at {
                BranchPoint::Latest => state
                    .operations()
                    .values()
                    .filter(|op| settled_checkpoint(&op.status).is_some())
                    .map(|op| op.accepted_order)
                    .max()
                    .unwrap_or(0),
                BranchPoint::Through(turn) => order(turn).ok_or_else(unknown_turn)?,
                BranchPoint::Before(turn) => {
                    order(turn).ok_or_else(unknown_turn)?.saturating_sub(1)
                }
            };
            let through_turn = self
                .state_turns(id, state)
                .await?
                .into_iter()
                .rev()
                .find(|turn| order(&turn.id).is_some_and(|order| order <= limit))
                .map(|turn| turn.id);
            Ok(BranchBoundary {
                source_session_id: id.to_owned(),
                through_turn,
            })
        }

        async fn publish(
            &self,
            record: SessionRecord,
            checkpoint: Option<EncodedPayload>,
        ) -> Result<()> {
            let mut store = self.store.clone();
            let id = record.session_id.clone();
            let target = store.acquire(&id, OwnerId::new()).await?;
            if target.state.revision != 0 || target.state.payload.is_some() {
                return Err(Error::InvalidState(
                    "new branch identity unexpectedly exists".into(),
                ));
            }
            let mut state = DurableState::default();
            match checkpoint {
                Some(checkpoint) => {
                    state.apply_transition(1, Transition::CheckpointCommitted { checkpoint })?;
                }
                None => state.advance_revision(1)?,
            }
            state.set_session(Some(record));
            let records = state.stage_records();
            store
                .replace(
                    &id,
                    &target.owner,
                    0,
                    &state.checkpoint_payload()?,
                    &records,
                )
                .await?;
            Ok(())
        }

        async fn peek(&self, id: &str) -> Result<Option<DurableState>> {
            let stored = self.store.clone().peek(id).await?;
            if stored.payload.is_none() {
                return Ok(None);
            }
            reduce_peeked(stored).map(Some)
        }

        async fn require(&self, id: &str) -> Result<DurableState> {
            self.peek(id).await?.ok_or_else(|| Error::SessionNotFound {
                session_id: id.to_owned(),
            })
        }

        async fn load_payload(&self, id: &str, payload: &EncodedPayload) -> Result<EncodedPayload> {
            payload.load(&mut self.store.clone(), id).await
        }

        async fn hydrate(
            &self,
            id: &str,
            payload: &EncodedPayload,
            family: Option<HarnessFamily>,
        ) -> Result<Value> {
            let value: Value = self.load_payload(id, payload).await?.decode()?;
            if is_portable(&value)
                || family.is_some_and(|family| family != HarnessFamily::Codex)
                || is_claude(&value)
            {
                return Ok(value);
            }
            let saved: crate::context::Snapshot =
                serde_json::from_value(value).map_err(Error::InvalidPayload)?;
            let snapshot = crate::context::restore_snapshot(
                crate::context::Reader::Store(&self.store, id),
                saved,
            )
            .await?;
            serde_json::to_value(snapshot).map_err(Error::InvalidPayload)
        }

        async fn native_checkpoint(&self, id: &str, state: &DurableState) -> Result<Option<Value>> {
            match state.latest_checkpoint() {
                Some(payload) => self
                    .hydrate(id, payload, state.session().map(SessionRecord::family))
                    .await
                    .map(Some),
                None => Ok(None),
            }
        }

        async fn state_turns(&self, id: &str, state: &DurableState) -> Result<Vec<StoredTurn>> {
            let mut operations: Vec<_> = state.operations().iter().collect();
            operations.sort_by_key(|(_, op)| op.accepted_order);
            let mut turns = Vec::new();
            for (turn, operation) in operations {
                let input: Value = self.load_payload(id, &operation.input).await?.decode()?;
                if input
                    .get("kind")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind != "prompt")
                {
                    continue;
                }
                turns.push(StoredTurn {
                    id: turn.clone(),
                    preview: prompt_text(&input),
                    status: match operation.status {
                        OperationStatus::Pending => TurnStatus::Pending,
                        OperationStatus::Completed { .. } => TurnStatus::Completed,
                        OperationStatus::Failed { .. } => TurnStatus::Failed,
                        OperationStatus::Cancelled { .. } => TurnStatus::Cancelled,
                    },
                    input,
                });
            }
            Ok(turns)
        }

        async fn first_prompt(&self, id: &str, state: &DurableState) -> Option<String> {
            let (_, first) = state
                .operations()
                .iter()
                .min_by_key(|(_, op)| op.accepted_order)?;
            let input: Value = self
                .load_payload(id, &first.input)
                .await
                .ok()?
                .decode()
                .ok()?;
            prompt_text(&input)
        }
    }

    fn unknown_turn() -> Error {
        Error::InvalidState("unknown or expired turn".into())
    }

    const fn settled_checkpoint(status: &OperationStatus) -> Option<&EncodedPayload> {
        match status {
            OperationStatus::Completed { checkpoint, .. }
            | OperationStatus::Failed { checkpoint, .. }
            | OperationStatus::Cancelled {
                checkpoint: Some(checkpoint),
            } => Some(checkpoint),
            _ => None,
        }
    }

    fn encode_native(family: HarnessFamily, value: Value) -> Result<EncodedPayload> {
        if is_portable(&value) {
            portable_checkpoint(&value)?
                .require_family(family)
                .map_err(|error| Error::InvalidState(error.to_string()))?;
            return EncodedPayload::encode(&value);
        }
        match family {
            family if family == HarnessFamily::Claude => EncodedPayload::encode(&value),
            family if family == HarnessFamily::Codex => {
                let snapshot: nanocode_backend_codex::session::SessionSnapshot =
                    serde_json::from_value(value).map_err(Error::InvalidPayload)?;
                Ok(crate::context::prepare_snapshot(snapshot, &HashSet::new())?.payload)
            }
            _ => EncodedPayload::encode(&value),
        }
    }

    fn is_child_journal(id: &str) -> bool {
        id.ends_with(":subagents")
    }

    fn is_claude(value: &Value) -> bool {
        value.get("provider").and_then(Value::as_str) == Some("claude")
    }

    fn is_portable(value: &Value) -> bool {
        value
            .get("format")
            .and_then(Value::as_str)
            .is_some_and(|format| format.starts_with("nanocode-session-checkpoint/"))
    }

    fn portable_checkpoint(value: &Value) -> Result<nanocode_agent::SessionCheckpoint> {
        let checkpoint: nanocode_agent::SessionCheckpoint =
            serde_json::from_value(value.clone()).map_err(Error::InvalidPayload)?;
        checkpoint
            .validate()
            .map_err(|error| Error::InvalidState(error.to_string()))?;
        Ok(checkpoint)
    }

    fn infer_record(id: &str, checkpoint: &Value) -> Result<SessionRecord> {
        if is_portable(checkpoint) {
            let checkpoint = portable_checkpoint(checkpoint)?;
            let mut record = SessionRecord::root(id, checkpoint.model(), None)
                .with_lineage(checkpoint.lineage().clone());
            record.created_at_ms = 0;
            record.updated_at_ms = 0;
            return Ok(record);
        }
        let model = checkpoint
            .get("model")
            .and_then(Value::as_str)
            .and_then(|model| model.parse::<ModelSelection>().ok())
            .ok_or_else(|| Error::InvalidState("stored checkpoint has no known model".into()))?;
        if is_claude(checkpoint) != (model.family() == HarnessFamily::Claude) {
            return Err(Error::InvalidState(
                "stored checkpoint model belongs to another family".into(),
            ));
        }
        let workspace = checkpoint
            .get("workspace")
            .and_then(Value::as_str)
            .filter(|workspace| !workspace.is_empty())
            .map(PathBuf::from);
        Ok(SessionRecord {
            session_id: id.to_owned(),
            model,
            workspace,
            title: None,
            lineage: Lineage::root(id),
            created_at_ms: 0,
            updated_at_ms: 0,
            history_pruned: false,
            branch: None,
            start_checkpoint: None,
            initial: None,
        })
    }

    fn prompt_text(input: &Value) -> Option<String> {
        fn find(value: &Value, depth: usize) -> Option<&str> {
            if depth > 8 {
                return None;
            }
            match value {
                Value::Object(object) => {
                    for key in ["instruction", "text", "prompt", "message"] {
                        if let Some(text) = object.get(key).and_then(Value::as_str)
                            && !text.trim().is_empty()
                        {
                            return Some(text);
                        }
                    }
                    object.values().find_map(|value| find(value, depth + 1))
                }
                Value::Array(items) => items.iter().find_map(|value| find(value, depth + 1)),
                _ => None,
            }
        }
        find(input, 0).map(|text| text.chars().take(500).collect())
    }

    fn transcript(
        checkpoint: &Value,
        family: HarnessFamily,
        prompts: &[AdmittedPrompt],
    ) -> Vec<TranscriptItem> {
        let checkpoint = if is_portable(checkpoint) {
            &checkpoint["payload"]
        } else {
            checkpoint
        };
        if family == HarnessFamily::Claude {
            claude_transcript(checkpoint, prompts)
        } else if family == HarnessFamily::Codex {
            codex_transcript(checkpoint)
        } else {
            Vec::new()
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    enum PromptPart {
        Text(String),
        Media,
    }

    struct AdmittedPrompt {
        parts: Vec<PromptPart>,
        steers: usize,
    }

    fn admitted_prompts(turns: &[StoredTurn], state: &DurableState) -> Vec<AdmittedPrompt> {
        turns
            .iter()
            .filter_map(|turn| {
                let input = &turn.input;
                if input["provider"] != "claude" || input["kind"] != "prompt" {
                    return None;
                }
                let parts = match &input["prompt"]["instruction"] {
                    Value::String(text) => vec![PromptPart::Text(text.clone())],
                    Value::Array(items) => items
                        .iter()
                        .map(|item| match item["type"].as_str() {
                            Some("text") => PromptPart::Text(
                                item["text"].as_str().unwrap_or_default().to_owned(),
                            ),
                            _ => PromptPart::Media,
                        })
                        .collect(),
                    _ => return None,
                };

                let steers = state
                    .operations()
                    .iter()
                    .find(|(id, _)| id.as_str() == turn.id)
                    .map_or(0, |(_, operation)| {
                        operation.retired_steers as usize + operation.steers.len()
                    });
                Some(AdmittedPrompt { parts, steers })
            })
            .collect()
    }

    fn blocks(message: &Value) -> impl Iterator<Item = &Value> {
        message["content"].as_array().into_iter().flatten()
    }

    fn prompt_parts(message: &Value) -> Vec<PromptPart> {
        blocks(message)
            .filter_map(|block| match block["type"].as_str()? {
                "text" => Some(PromptPart::Text(block["text"].as_str()?.to_owned())),
                "image" | "document" => Some(PromptPart::Media),
                _ => None,
            })
            .collect()
    }

    fn prompt_display(message: &Value) -> String {
        let (mut text, mut images, mut documents) = (String::new(), 0, 0);
        for block in blocks(message) {
            match block["type"].as_str() {
                Some("text") => text.push_str(block["text"].as_str().unwrap_or_default()),
                Some("image") => {
                    images += 1;
                    text.push_str(&format!("[Image #{images}]"));
                }
                Some("document") => {
                    documents += 1;
                    text.push_str(&format!("[Document #{documents}]"));
                }
                _ => {}
            }
        }
        text
    }

    fn recovery_notice(message: &Value, notices: &[&str]) -> bool {
        let mut text = String::new();
        for block in blocks(message) {
            match block["text"].as_str() {
                Some(part) if block["type"] == "text" => text.push_str(part),
                _ => return false,
            }
        }
        notices.contains(&text.as_str())
    }

    fn tool_output(content: &Value) -> String {
        match content {
            Value::String(text) => text.clone(),
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match item["type"].as_str()? {
                    "text" => item["text"].as_str().map(str::to_owned),
                    "image" => Some("[image]".to_owned()),
                    "document" => Some("[document]".to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            _ => String::new(),
        }
    }

    fn user_rows(messages: &[&Value], notices: &[&str], items: &mut Vec<TranscriptItem>) {
        for message in messages {
            if recovery_notice(message, notices) {
                continue;
            }
            let text = prompt_display(message);
            if !text.is_empty() {
                items.push(TranscriptItem::User(text));
            }
        }
    }

    fn steer_rows(
        segment: &[&Value],
        steers: usize,
        notices: &[&str],
        items: &mut Vec<TranscriptItem>,
    ) {
        user_rows(
            &segment[segment.len().saturating_sub(steers)..],
            notices,
            items,
        );
    }

    fn claude_assistant_rows(message: &Value, items: &mut Vec<TranscriptItem>) {
        for block in blocks(message) {
            match block["type"].as_str() {
                Some("text") => {
                    if let Some(text) = block["text"].as_str() {
                        items.push(TranscriptItem::Assistant(text.into()));
                    }
                }

                Some("thinking") => {
                    if let Some(text) = block["thinking"]
                        .as_str()
                        .filter(|text| !text.trim().is_empty())
                    {
                        items.push(TranscriptItem::Reasoning(text.into()));
                    }
                }
                Some("tool_use" | "server_tool_use") => items.push(TranscriptItem::Tool {
                    call_id: block["id"].as_str().unwrap_or_default().into(),
                    name: block["name"].as_str().unwrap_or_default().into(),
                    arguments: block["input"].to_string(),
                    parent_call_id: None,
                }),
                Some("mcp_tool_use") => items.push(TranscriptItem::Tool {
                    call_id: block["id"].as_str().unwrap_or_default().into(),
                    name: format!(
                        "mcp__{}__{}",
                        block["server_name"].as_str().unwrap_or_default(),
                        block["name"].as_str().unwrap_or_default()
                    ),
                    arguments: block["input"].to_string(),
                    parent_call_id: None,
                }),
                _ => {}
            }
        }
    }

    fn claude_transcript(checkpoint: &Value, prompts: &[AdmittedPrompt]) -> Vec<TranscriptItem> {
        use nanocode_backend_codex::session::ToolOutcome;
        let mut items = Vec::new();
        let conversation = &checkpoint["conversation"];
        if let Some(summary) = conversation["summary"]
            .as_str()
            .filter(|summary| !summary.is_empty())
        {
            items.push(TranscriptItem::Assistant(format!(
                "Retained conversation summary:\n{summary}"
            )));
        }
        let notices = conversation["recovery_notices"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>();
        let messages = conversation["messages"]
            .as_array()
            .map_or(&[][..], Vec::as_slice);
        let receipt = |message: &Value| blocks(message).any(|block| block["type"] == "tool_result");

        let mut children = std::collections::HashMap::<&str, Vec<(&str, &Value)>>::new();
        let mut outcomes = std::collections::HashMap::<&str, &str>::new();
        for round in conversation["code_calls"].as_array().into_iter().flatten() {
            let Some(receipt_id) = round["tool_use_id"].as_str() else {
                continue;
            };
            let origin = round["origin_call_id"].as_str();
            for call in round["calls"].as_array().into_iter().flatten() {
                if let Some(call_id) = call["call_id"].as_str() {
                    outcomes.insert(call_id, call["status"].as_str().unwrap_or("unknown"));
                    let cell = call["parent_call_id"]
                        .as_str()
                        .or(origin)
                        .unwrap_or(receipt_id);
                    children.entry(receipt_id).or_default().push((cell, call));
                }
            }
        }
        let mut replayed_children = std::collections::HashSet::new();
        let mut next_prompt = 0;
        let mut index = 0;
        while let Some(message) = messages.get(index) {
            if message["role"].as_str() == Some("assistant") {
                claude_assistant_rows(message, &mut items);
                index += 1;
                continue;
            }
            if receipt(message) {
                for block in blocks(message).filter(|block| block["type"] == "tool_result") {
                    let parent = block["tool_use_id"].as_str().unwrap_or_default();
                    for (cell, call) in children.get(parent).into_iter().flatten() {
                        let call_id = call["call_id"].as_str().unwrap_or_default();
                        if !replayed_children.insert(call_id) {
                            continue;
                        }
                        items.push(TranscriptItem::Tool {
                            call_id: call_id.into(),
                            name: call["name"].as_str().unwrap_or_default().into(),
                            arguments: call["input"].to_string(),
                            parent_call_id: Some((*cell).into()),
                        });
                        let outcome = match outcomes.get(call_id).copied() {
                            Some("completed") => ToolOutcome::Completed,
                            Some("failed") => ToolOutcome::Failed,
                            _ => ToolOutcome::Unknown,
                        };
                        items.push(TranscriptItem::tool_result(call_id, "", outcome));
                    }
                    items.push(TranscriptItem::tool_result(
                        parent,
                        &tool_output(&block["content"]),
                        if block["is_error"].as_bool() == Some(true) {
                            ToolOutcome::Failed
                        } else {
                            ToolOutcome::Completed
                        },
                    ));
                }

                let content = blocks(message)
                    .filter(|block| block["type"] != "tool_result")
                    .cloned()
                    .collect::<Vec<_>>();
                if !content.is_empty() {
                    let shared = serde_json::json!({ "content": content });
                    user_rows(&[&shared], &notices, &mut items);
                }
                index += 1;
                continue;
            }

            let end = messages[index..]
                .iter()
                .position(|message| message["role"].as_str() != Some("user") || receipt(message))
                .map_or(messages.len(), |offset| index + offset);
            let run = &messages[index..end];
            index = end;
            let mut segment = Vec::new();
            let mut steers = None;
            for message in run {
                let parts = prompt_parts(message);
                let Some(offset) = prompts
                    .get(next_prompt..)
                    .and_then(|rest| rest.iter().position(|prompt| prompt.parts == parts))
                else {
                    segment.push(message);
                    continue;
                };
                match steers {
                    Some(steers) => steer_rows(&segment, steers, &notices, &mut items),

                    None if offset == 0 => {}
                    None => user_rows(&segment, &notices, &mut items),
                }
                segment.clear();
                steers = Some(prompts[next_prompt + offset].steers);
                next_prompt += offset + 1;
                items.push(TranscriptItem::User(prompt_display(message)));
            }

            match steers {
                Some(steers) => steer_rows(&segment, steers, &notices, &mut items),
                None => user_rows(&segment, &notices, &mut items),
            }
        }
        items
    }

    fn codex_transcript(snapshot: &Value) -> Vec<TranscriptItem> {
        let mut items = Vec::new();
        for item in snapshot["history"].as_array().into_iter().flatten() {
            match item["type"].as_str() {
                Some("message") => {
                    let role = item["role"].as_str();
                    for content in item["content"].as_array().into_iter().flatten() {
                        let Some(text) = content["text"].as_str().filter(|text| !text.is_empty())
                        else {
                            continue;
                        };
                        match (role, content["type"].as_str()) {
                            (Some("user"), Some("input_text")) if !is_injected_context(text) => {
                                items.push(TranscriptItem::User(text.into()));
                            }
                            (Some("assistant"), _) => {
                                items.push(TranscriptItem::Assistant(text.into()));
                            }
                            _ => {}
                        }
                    }
                }
                Some("reasoning") => {
                    for summary in item["summary"].as_array().into_iter().flatten() {
                        if let Some(text) = summary["text"].as_str().filter(|text| !text.is_empty())
                        {
                            items.push(TranscriptItem::Reasoning(text.into()));
                        }
                    }
                }
                Some("function_call") => items.push(TranscriptItem::Tool {
                    call_id: item["call_id"].as_str().unwrap_or_default().into(),
                    name: item["name"].as_str().unwrap_or_default().into(),
                    arguments: item["arguments"].as_str().unwrap_or_default().into(),
                    parent_call_id: None,
                }),
                Some("custom_tool_call") => items.push(TranscriptItem::Tool {
                    call_id: item["call_id"].as_str().unwrap_or_default().into(),
                    name: item["name"].as_str().unwrap_or_default().into(),
                    arguments: item["input"].as_str().unwrap_or_default().into(),
                    parent_call_id: None,
                }),
                _ => {}
            }
        }
        items
    }

    fn is_injected_context(text: &str) -> bool {
        let text = text.trim_start();
        text.starts_with("<environment_context")
            || text.starts_with("<user_instructions")
            || text.starts_with("# AGENTS.md")
    }
}
