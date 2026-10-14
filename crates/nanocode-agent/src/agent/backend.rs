use super::*;

/// Input that selects the concrete builder returned by [`Nanocodex::builder`].
///
/// Backend types use the associated builder to expose their own deliberate
/// policy while sharing the same lifecycle handle after build.
pub trait BuilderBackend {
    /// Builder configured by this backend input.
    type Builder;

    /// Converts the backend input into its concrete builder.
    fn into_builder(self) -> Self::Builder;
}

/// Backend operation future used at the lifecycle-erasure boundary.
#[cfg(not(target_family = "wasm"))]
pub type BackendFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

/// Backend operation future used at the lifecycle-erasure boundary.
#[cfg(target_family = "wasm")]
pub type BackendFuture<T> = Pin<Box<dyn Future<Output = T> + 'static>>;

/// Opaque key assigned by the common agent handle before backend admission.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct BackendTurnKey(pub u64);

/// Validated prompt input handed to one lifecycle backend.
#[doc(hidden)]
pub struct BackendPrompt {
    /// Common handle identity for control operations.
    pub key: BackendTurnKey,
    /// Validated prompt input.
    pub prompt: Prompt,
    /// Optional caller-owned durable operation identity.
    pub request_id: Option<String>,
    /// Whether the prompt must be durably cancelled as part of admission.
    pub cancel_on_admission: bool,
    /// Canonical publisher routing events to both session and turn streams.
    pub events: nanocode_types::events::AgentEventPublisher,
}

/// One admitted backend turn and its independently awaitable result.
#[doc(hidden)]
pub struct BackendTurn {
    /// Durable request identity selected during admission, when any.
    pub request_id: Option<String>,
    /// Independently awaitable terminal result.
    pub result: BackendFuture<Result<TurnResult>>,
}

/// Atomic live-routing decision made by a backend driver.
#[doc(hidden)]
pub enum BackendPromptRoute {
    /// The input started a new turn.
    Started(BackendTurn),
    /// The input was steered into the active turn.
    Steered,
}

/// Host persistence for one durable root's subagent task-tree journal.
///
/// Durability adapters supply this beside the root's execution state. Values
/// are opaque, Rust-owned JSON. The journal itself stays small: child
/// checkpoints are separate immutable records addressed by the lowercase hex
/// SHA-256 of their exact JSON text.
pub trait ChildJournalStore: Send + Sync + 'static {
    /// Loads the latest journal value.
    fn load(&self) -> BackendFuture<std::io::Result<Option<String>>>;
    /// Atomically stores `records` and replaces the journal value that
    /// references them. Records already stored may be supplied again.
    fn save(&self, payload: String, records: Vec<Arc<str>>) -> BackendFuture<std::io::Result<()>>;
    /// Loads one record referenced by a saved journal.
    fn load_record(&self, key: String) -> BackendFuture<std::io::Result<String>>;
    /// Records a child's latest committed checkpoint as that child's own
    /// durable session (catalog entry plus resumable state), keyed by its
    /// distinct session ID and carrying its `Origin::Subagent` lineage.
    ///
    /// Called after the journal containing the same boundary was saved.
    /// Hosts without a session catalog keep only the journal.
    fn record_child(&self, _checkpoint: SessionCheckpoint) -> BackendFuture<std::io::Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// A durable root's task-tree journal, exposed on its [`AgentHandle`].
///
/// Any harness whose builder attaches durability exposes this on its root
/// handle, so a subagent registry makes that root's children durable without
/// host wiring. The first session to claim it owns it.
#[derive(Clone)]
pub struct ChildJournal {
    store: Arc<dyn ChildJournalStore>,
    owner: Arc<std::sync::OnceLock<Arc<str>>>,
}

impl ChildJournal {
    /// Wraps a host journal store.
    pub fn new(store: Arc<dyn ChildJournalStore>) -> Self {
        Self {
            store,
            owner: Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Binds the journal to its root session; false for any other session.
    pub fn claim(&self, session_id: &str) -> bool {
        &**self.owner.get_or_init(|| Arc::from(session_id)) == session_id
    }

    /// The underlying host store.
    pub fn store(&self) -> Arc<dyn ChildJournalStore> {
        Arc::clone(&self.store)
    }
}

impl std::fmt::Debug for ChildJournal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChildJournal").finish_non_exhaustive()
    }
}

/// Embedding-owned construction of clean native children.
///
/// Implementations retain provider recipes and approved host capabilities. The
/// supplied parent is weak and never extends the owning driver's lifetime.
pub trait AgentFactory: Send + Sync + 'static {
    /// Builds the selected native family without starting its first turn.
    fn spawn(
        &self,
        parent: AgentHandle,
        options: SpawnOptions,
        host_context: Option<Arc<str>>,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>>;

    /// Rejects construction after the native owning runtime begins shutdown.
    fn ensure_available(&self, parent: AgentHandle) -> BackendFuture<Result<()>> {
        let settings = self.settings(parent);
        Box::pin(async move { settings.await.map(|_| ()) })
    }

    /// Constructs an ordered clean batch and observes each materialized child.
    /// Native runtimes may override this to retain atomic admission and rollback.
    fn spawn_many(
        &self,
        parent: AgentHandle,
        count: usize,
        observer: Arc<dyn Fn(&str) + Send + Sync>,
        host_context: Option<Arc<str>>,
    ) -> BackendFuture<Result<Vec<(Nanocodex, AgentEvents)>>> {
        let pending = (0..count)
            .map(|_| self.spawn(parent.clone(), SpawnOptions::new(), host_context.clone()))
            .collect::<Vec<_>>();
        Box::pin(async move {
            let mut children: Vec<(Nanocodex, AgentEvents)> = Vec::with_capacity(count);
            for child in pending {
                match child.await {
                    Ok(child) => children.push(child),
                    Err(error) => {
                        for (child, _) in &children {
                            let _ = child.shutdown().await;
                        }
                        return Err(error);
                    }
                }
            }
            // All or nothing: observers see only a batch that started completely.
            for (child, _) in &children {
                observer(child.session_id());
            }
            Ok(children)
        })
    }

    /// Reads the owning runtime's current settings for model-boundary inheritance.
    fn settings(
        &self,
        parent: AgentHandle,
    ) -> BackendFuture<Result<(crate::HarnessModel, Thinking)>> {
        Box::pin(async move {
            let model = parent.harness_model();
            Ok((model.clone(), model.default_thinking()))
        })
    }

    /// Forks the parent's native conversation when this factory supports it.
    fn fork(
        &self,
        _parent: AgentHandle,
        _request: ForkRequest,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        unsupported("fork")
    }

    /// Rebinds approved host capabilities to a checkpoint of this factory's family,
    /// keeping the checkpoint's session identity.
    fn restore(
        &self,
        _parent: AgentHandle,
        _checkpoint: SessionCheckpoint,
        _host_context: Option<Arc<str>>,
    ) -> BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        unsupported("restore")
    }
}

fn unsupported<T: 'static>(capability: &'static str) -> BackendFuture<Result<T>> {
    Box::pin(async move { Err(NanocodexError::UnsupportedCapability { capability }) })
}

/// Backend implementor contract behind the common `Nanocodex` lifecycle.
///
/// This trait erases only lifecycle control after a concrete driver has been
/// built. Provider Tower services remain concrete, driver-owned types.
#[doc(hidden)]
pub trait LifecycleBackend: Send + Sync + 'static {
    /// Immutable native agent-loop family.
    fn harness_family(&self) -> crate::HarnessFamily;

    /// Lifecycle operations this backend supports.
    fn capabilities(&self) -> Capabilities;

    /// Where this session is persisted, when it is.
    fn persistence(&self) -> Option<Persistence> {
        None
    }

    /// Admits one prompt and returns the complete accepted turn.
    fn submit(&self, prompt: BackendPrompt) -> BackendFuture<Result<BackendTurn>>;

    /// Atomically starts a turn or steers the active one.
    fn route(&self, prompt: BackendPrompt) -> BackendFuture<Result<BackendPromptRoute>>;

    /// Steers one exact active turn.
    fn steer(&self, key: BackendTurnKey, prompt: Prompt) -> BackendFuture<Result<()>>;

    /// Admits a steer with a caller-owned identity unique within this turn.
    fn steer_with_id(
        &self,
        _key: BackendTurnKey,
        _id: String,
        _prompt: Prompt,
    ) -> BackendFuture<Result<()>> {
        unsupported("identified_steering")
    }

    /// Withdraws the latest steer if it has not reached a model boundary.
    fn withdraw_steer(&self, _key: BackendTurnKey, _id: String) -> BackendFuture<Result<bool>> {
        unsupported("identified_steering")
    }

    /// Cancels one exact unfinished turn.
    fn cancel(&self, key: BackendTurnKey) -> BackendFuture<Result<()>>;

    /// Changes the model within this backend's family, subject to
    /// [`Capabilities::model`].
    fn set_harness_model(&self, model: crate::HarnessModel) -> BackendFuture<Result<()>>;

    /// Changes reasoning policy for later turns.
    fn set_thinking(&self, thinking: Thinking) -> BackendFuture<Result<()>>;

    /// Changes priority policy for later turns.
    fn set_fast_mode(&self, enabled: bool) -> BackendFuture<Result<()>>;

    /// Changes processing policy for later turns.
    ///
    /// Backends with only a priority switch reject Ultrafast unless they override this method.
    fn set_service_tier(&self, service_tier: ServiceTier) -> BackendFuture<Result<()>> {
        match service_tier {
            ServiceTier::Standard => self.set_fast_mode(false),
            ServiceTier::Priority | ServiceTier::Fast => self.set_fast_mode(true),
            ServiceTier::Ultrafast => unsupported("ultrafast_service_tier"),
        }
    }

    /// Compacts retained context.
    fn compact(&self) -> BackendFuture<Result<()>>;

    /// Appends adapter-owned developer context.
    fn append_developer_message(&self, text: String) -> BackendFuture<Result<AgentSessionContext>>;

    /// Reads the latest safe model-visible context.
    fn context(&self) -> BackendFuture<Result<AgentSessionContext>>;

    /// Captures the latest committed boundary without waiting for an active turn.
    fn checkpoint(&self) -> BackendFuture<Result<SessionCheckpoint>>;

    /// Waits until a just-created durable child's first checkpoint is
    /// persisted, so the child is listed and resumable before its first turn.
    /// Backends without durable children have nothing to persist.
    fn persist_initial(&self) -> BackendFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Retracts the first checkpoint of a child whose creation was abandoned,
    /// never history the child held before it was created or restored.
    fn discard_initial(&self) -> BackendFuture<Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Starts a clean sibling lifecycle.
    fn spawn(&self, options: SpawnOptions) -> BackendFuture<Result<(Nanocodex, AgentEvents)>>;

    /// Forks from the requested boundary with the requested provenance.
    fn fork(&self, request: ForkRequest) -> BackendFuture<Result<(Nanocodex, AgentEvents)>>;

    /// Flushes backend-owned persistence.
    fn flush(&self) -> BackendFuture<Result<()>>;

    /// Disconnects local resources without requesting cancellation of durable work.
    ///
    /// Backends without detached durable execution fall back to ordinary
    /// shutdown.
    fn disconnect(&self) -> BackendFuture<Result<()>> {
        self.shutdown()
    }

    /// Idempotently shuts down local resources.
    fn shutdown(&self) -> BackendFuture<Result<()>>;
}

/// Backend-neutral construction context for the common agent handle.
#[doc(hidden)]
pub struct BackendRuntime {
    agent_id: Arc<str>,
    session_id: Arc<str>,
    lineage: Lineage,
    driver_turn_ids: bool,
    events: nanocode_types::events::AgentEventPublisher,
}

impl BackendRuntime {
    /// Creates one session event channel before the concrete driver starts.
    #[must_use]
    pub fn new(session_id: impl Into<Arc<str>>) -> (Self, AgentEvents) {
        let session_id = session_id.into();
        Self::with_agent_id(Arc::clone(&session_id), session_id)
    }

    /// Creates one session event channel with a distinct durable agent identity.
    #[must_use]
    pub fn with_agent_id(
        agent_id: impl Into<Arc<str>>,
        session_id: impl Into<Arc<str>>,
    ) -> (Self, AgentEvents) {
        let agent_id = agent_id.into();
        let session_id = session_id.into();
        let (events, stream) =
            nanocode_types::events::AgentEventPublisher::channel(session_id.to_string());
        (
            Self {
                lineage: Lineage::root(session_id.as_ref()),
                agent_id,
                session_id,
                driver_turn_ids: false,
                events,
            },
            stream,
        )
    }

    /// Records where this session came from; defaults to a fresh root.
    #[must_use]
    pub fn with_lineage(mut self, lineage: Lineage) -> Self {
        self.lineage = lineage;
        self
    }

    /// Preserve driver-generated turn identities instead of external request IDs.
    #[must_use]
    pub fn with_driver_turn_ids(mut self) -> Self {
        self.driver_turn_ids = true;
        self
    }

    /// Returns the canonical event publisher consumed by the concrete driver.
    #[must_use]
    pub fn events(&self) -> nanocode_types::events::AgentEventPublisher {
        self.events.clone()
    }

    /// Erases one concrete lifecycle implementation into the common handle.
    #[must_use]
    pub fn bind<B>(self, backend: B) -> Nanocodex
    where
        B: LifecycleBackend,
    {
        let session = SessionInfo {
            session_id: self.session_id.to_string(),
            family: backend.harness_family(),
            lineage: self.lineage,
        };
        Nanocodex {
            backend: Arc::new(backend),
            events: self.events,
            next_turn: Arc::new(AtomicU64::new(1)),
            agent_id: self.agent_id,
            session: Arc::new(session),
            driver_turn_ids: self.driver_turn_ids,
        }
    }
}
