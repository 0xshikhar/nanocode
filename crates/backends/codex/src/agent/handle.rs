use super::*;
use nanocode_agent::backend::{created, created_batch};

pub(super) struct OpenAiAgentFactory {
    pub(super) commands: mpsc::WeakSender<Command>,
    pub(super) shutdown: DriverShutdown,
    pub(super) conversation_id: Arc<str>,
}

impl super::backend::AgentFactory for OpenAiAgentFactory {
    fn ensure_available(&self, _parent: AgentHandle) -> super::backend::BackendFuture<Result<()>> {
        let available = self.shutdown.is_running()
            && self
                .commands
                .upgrade()
                .is_some_and(|commands| !commands.is_closed());
        Box::pin(async move {
            if available {
                Ok(())
            } else {
                Err(NanocodexError::AgentStopped)
            }
        })
    }
    fn spawn(
        &self,
        _parent: AgentHandle,
        options: SpawnOptions,
        host_context: Option<Arc<str>>,
    ) -> super::backend::BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let commands = self.commands.upgrade();
        let shutdown = self.shutdown.clone();
        Box::pin(async move {
            options.validate_harness()?;
            if options
                .selected_family()
                .is_some_and(|family| family != crate::HarnessFamily::Codex)
            {
                return Err(NanocodexError::InvalidRequest(
                    "Claude harness requires a configured child factory".into(),
                ));
            }
            let commands = commands.ok_or(NanocodexError::AgentStopped)?;
            created(
                request_spawn_with_host_context(&commands, &shutdown, options, host_context).await,
            )
            .await
        })
    }
    fn spawn_many(
        &self,
        _parent: AgentHandle,
        count: usize,
        observer: Arc<dyn Fn(&str) + Send + Sync>,
        host_context: Option<Arc<str>>,
    ) -> super::backend::BackendFuture<Result<Vec<(Nanocodex, AgentEvents)>>> {
        let commands = self.commands.upgrade();
        let shutdown = self.shutdown.clone();
        Box::pin(async move {
            let commands = commands.ok_or(NanocodexError::AgentStopped)?;
            created_batch(
                request_spawn_many(&commands, &shutdown, count, Some(observer), host_context).await,
            )
            .await
        })
    }
    fn settings(
        &self,
        _parent: AgentHandle,
    ) -> super::backend::BackendFuture<Result<(crate::ModelSelection, Thinking)>> {
        let commands = self.commands.upgrade();
        let shutdown = self.shutdown.clone();
        Box::pin(async move {
            let commands = commands.ok_or(NanocodexError::AgentStopped)?;
            let snapshot = request_command(&commands, &shutdown, |result| Command::ChildSnapshot {
                result,
            })
            .await?;
            Ok((
                crate::ModelSelection::Codex(snapshot.model),
                snapshot.thinking,
            ))
        })
    }
    fn restore(
        &self,
        _parent: AgentHandle,
        checkpoint: SessionCheckpoint,
        host_context: Option<Arc<str>>,
    ) -> super::backend::BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let commands = self.commands.upgrade();
        let shutdown = self.shutdown.clone();
        Box::pin(async move {
            let snapshot = ChildState::from_checkpoint(checkpoint)?;
            let commands = commands.ok_or(NanocodexError::AgentStopped)?;
            let restored = request_command(&commands, &shutdown, |result| Command::Spawn {
                options: SpawnOptions::new()
                    .model(snapshot.model)
                    .thinking(snapshot.thinking),
                restore: Some(snapshot),
                host_context,
                result,
            })
            .await;
            created(restored).await
        })
    }
    fn fork(
        &self,
        _parent: AgentHandle,
        request: ForkRequest,
    ) -> super::backend::BackendFuture<Result<(Nanocodex, AgentEvents)>> {
        let commands = self.commands.upgrade();
        let shutdown = self.shutdown.clone();
        let conversation_id = Arc::clone(&self.conversation_id);
        Box::pin(async move {
            let commands = commands.ok_or(NanocodexError::AgentStopped)?;
            let (point, origin) = request.into_parts();
            let point = super::backend::resolve_fork_point(point, &conversation_id)?;
            created(request_fork(&commands, &shutdown, point, origin, SessionId::new(), None).await)
                .await
        })
    }
}

pub(super) async fn request_fork(
    commands: &mpsc::Sender<Command>,
    shutdown: &DriverShutdown,
    point: ForkFrom,
    origin: crate::Origin,
    session_id: SessionId,
    policy: Option<Arc<dyn execution::ExecutionPolicy>>,
) -> Result<(Nanocodex, AgentEvents)> {
    if !matches!(
        origin,
        crate::Origin::Fork | crate::Origin::SideConversation
    ) {
        return Err(NanocodexError::InvalidRequest(
            "a fork must be a fork or a side conversation".into(),
        ));
    }
    request_command(commands, shutdown, |result| Command::Fork {
        origin,
        point,
        session_id,
        policy,
        result,
    })
    .await
}

async fn request_spawn_with_host_context(
    commands: &mpsc::Sender<Command>,
    shutdown: &DriverShutdown,
    options: SpawnOptions,
    host_context: Option<Arc<str>>,
) -> Result<(Nanocodex, AgentEvents)> {
    request_command(commands, shutdown, |result| Command::Spawn {
        restore: None,
        options,
        host_context,
        result,
    })
    .await
}

async fn request_spawn_many(
    commands: &mpsc::Sender<Command>,
    shutdown: &DriverShutdown,
    count: usize,
    observer: Option<Arc<SpawnObserver>>,
    host_context: Option<Arc<str>>,
) -> Result<Vec<(Nanocodex, AgentEvents)>> {
    request_command(commands, shutdown, |result| Command::SpawnBatch {
        count,
        observer,
        host_context,
        result,
    })
    .await
}

pub(super) async fn request_command<T>(
    commands: &mpsc::Sender<Command>,
    shutdown: &DriverShutdown,
    command: impl FnOnce(oneshot::Sender<Result<T>>) -> Command,
) -> Result<T> {
    let (result, receiver) = oneshot::channel();
    if commands.send(command(result)).await.is_err() {
        return Err(shutdown.stopped_error().await);
    }
    match receiver.await {
        Ok(Err(NanocodexError::AgentStopped)) | Err(_) => Err(shutdown.stopped_error().await),
        Ok(outcome) => outcome,
    }
}
