use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use futures_util::{Sink, SinkExt, Stream, StreamExt, future};
use serde::Deserialize;
use serde_json::{Value, json, value::RawValue};
use tokio::{
    net::{TcpListener, TcpStream, ToSocketAddrs},
    sync::{mpsc, oneshot, watch},
};
use tokio_tungstenite::tungstenite::{
    self, Message,
    handshake::server::{ErrorResponse, Request, Response},
    protocol::{CloseFrame, frame::coding::CloseCode},
};

use super::AttachmentError;
use crate::{
    Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult,
    contract::{ToolError, ToolOutputWire},
};

const JSON_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;
const RECOVER_BATCH_LIMIT: usize = 100;
const PUBLIC_TOOL_NAME_LIMIT: usize = 128;
const PROTOCOL_IDENTIFIER_LIMIT: usize = 128;
const TURN_ID_LIMIT: usize = 256;
const WEBSOCKET_CLOSE_REASON_LIMIT: usize = 123;
const NAMESPACE_SEPARATOR: &str = "__";

#[derive(Clone)]
pub struct RemoteRunner {
    inner: Arc<Inner>,
}

struct Inner {
    namespace: Box<str>,
    catalog: watch::Sender<Option<Arc<[Arc<RemoteSpec>]>>>,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    generation: u64,
    connection: Option<Connection>,
    runtime_id: Option<Box<str>>,
    catalog: Option<Value>,
    pending: HashMap<Box<str>, Pending>,
    last_turn_ended: Option<[Box<str>; 3]>,
}

struct Connection {
    generation: u64,
    outbound: mpsc::UnboundedSender<Outbound>,
}

struct Pending {
    frame: Arc<str>,
    sent: bool,
    result: oneshot::Sender<Value>,
}

enum Outbound {
    Call(Box<str>),
    Frame(String),
}

struct RemoteSpec {
    remote_name: Box<str>,
    definition: ToolDefinition,
    parallel_safe: bool,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RunnerFrame {
    Catalog { runtime_id: String, tools: Value },
    Diagnostic {},
    Result { call_id: String, outcome: Value },
    Status { call_id: String, state: String },
    Drain {},
}

#[derive(Deserialize)]
struct CatalogEntry {
    definition: ToolDefinition,
    parallel_safe: bool,
    timeout_ms: u64,
}

#[derive(Deserialize)]
struct Outcome {
    status: OutcomeStatus,
    #[serde(default)]
    output: Option<Box<RawValue>>,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum OutcomeStatus {
    Completed,
    Unavailable,
    Ambiguous,
}

impl fmt::Debug for RemoteRunner {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteRunner")
            .field("namespace", &self.inner.namespace)
            .field("connected", &self.is_connected())
            .finish_non_exhaustive()
    }
}

impl RemoteRunner {
    pub fn new(namespace: impl Into<String>) -> Result<Self, AttachmentError> {
        let namespace = namespace.into();
        if !valid_namespace(&namespace) {
            return Err(AttachmentError::Catalog(
                format!(
                    "runner namespace `{namespace}` must start with an ASCII letter, contain only ASCII alphanumerics or `_`, and must not contain `__` or end with `_`"
                )
                .into(),
            ));
        }
        Ok(Self {
            inner: Arc::new(Inner {
                namespace: namespace.into(),
                catalog: watch::channel(None).0,
                state: Mutex::default(),
            }),
        })
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.inner.namespace
    }

    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.inner.state().connection.is_some()
    }

    pub async fn tools(&self) -> Result<RemoteTools, AttachmentError> {
        let mut catalog = self.inner.catalog.subscribe();
        let specs = catalog
            .wait_for(Option::is_some)
            .await
            .map_err(|_| AttachmentError::Closed)?
            .clone()
            .ok_or(AttachmentError::Closed)?;
        Ok(RemoteTools {
            tools: specs
                .iter()
                .map(|spec| RemoteTool {
                    runner: Arc::clone(&self.inner),
                    spec: Arc::clone(spec),
                })
                .collect(),
        })
    }

    pub async fn serve<T, E>(&self, mut transport: T) -> Result<(), AttachmentError>
    where
        T: Stream<Item = Result<String, E>> + Sink<String, Error = E> + Unpin,
        E: fmt::Display,
    {
        let first = match transport.next().await {
            Some(Ok(text)) => text,
            Some(Err(error)) => return Err(transport_error(error)),
            None => {
                return Err(AttachmentError::Transport(
                    "runner closed before publishing its catalog".into(),
                ));
            }
        };
        let RunnerFrame::Catalog { runtime_id, tools } = parse_frame(&first)? else {
            return Err(AttachmentError::Fenced(
                "runner must publish its catalog first".into(),
            ));
        };
        let (generation, mut outbound, backlog) = self.inner.attach(&runtime_id, tools)?;
        let result = self
            .connection(&mut transport, generation, &mut outbound, backlog)
            .await;
        self.inner.detach(generation);
        result
    }

    async fn connection<T, E>(
        &self,
        transport: &mut T,
        generation: u64,
        outbound: &mut mpsc::UnboundedReceiver<Outbound>,
        backlog: Vec<String>,
    ) -> Result<(), AttachmentError>
    where
        T: Stream<Item = Result<String, E>> + Sink<String, Error = E> + Unpin,
        E: fmt::Display,
    {
        send(transport, json!({"type": "ready"}).to_string()).await?;
        for frame in backlog {
            send(transport, frame).await?;
        }
        let mut draining = false;
        loop {
            tokio::select! {
                command = outbound.recv(), if !draining => {
                    let Some(command) = command else { return Ok(()) };
                    let frame = match command {
                        Outbound::Call(call_id) => match self.inner.claim(generation, &call_id) {
                            Some(frame) => frame,
                            None => continue,
                        },
                        Outbound::Frame(frame) => frame,
                    };
                    send(transport, frame).await?;
                }
                incoming = transport.next() => {
                    let text = match incoming {
                        Some(Ok(text)) => text,
                        Some(Err(error)) => return Err(transport_error(error)),
                        None => return Ok(()),
                    };
                    match parse_frame(&text)? {
                        RunnerFrame::Result { call_id, outcome } => {
                            self.inner.deliver(&call_id, outcome);
                            send(transport, json!({"type": "ack", "call_id": call_id}).to_string()).await?;
                        }
                        RunnerFrame::Status { call_id, state } => match state.as_str() {
                            "running" => {}
                            "missing" => {
                                if let Some(frame) = self.inner.resend(generation, &call_id) {
                                    send(transport, frame).await?;
                                }
                            }
                            state => {
                                return Err(AttachmentError::Fenced(
                                    format!("runner reported unknown call state `{state}`").into(),
                                ));
                            }
                        },
                        RunnerFrame::Diagnostic {} => {}
                        RunnerFrame::Drain {} => {
                            if draining {
                                return Err(AttachmentError::Fenced("runner drained twice".into()));
                            }
                            draining = true;
                            self.inner.detach(generation);
                            while let Ok(command) = outbound.try_recv() {
                                if let Outbound::Frame(frame) = command {
                                    send(transport, frame).await?;
                                }
                            }
                            send(transport, json!({"type": "draining"}).to_string()).await?;
                        }
                        RunnerFrame::Catalog { .. } => {
                            return Err(AttachmentError::Fenced(
                                "runner published a catalog after it was ready".into(),
                            ));
                        }
                    }
                }
            }
        }
    }
}

impl Inner {
    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn attach(
        &self,
        runtime_id: &str,
        tools: Value,
    ) -> Result<(u64, mpsc::UnboundedReceiver<Outbound>, Vec<String>), AttachmentError> {
        let specs = parse_catalog(&self.namespace, &tools)?;
        let mut state = self.state();
        if state
            .catalog
            .as_ref()
            .is_some_and(|pinned| pinned != &tools)
        {
            return Err(AttachmentError::Catalog(
                format!(
                    "runner `{}` published a catalog that differs from the pinned catalog",
                    self.namespace
                )
                .into(),
            ));
        }
        if state
            .runtime_id
            .as_deref()
            .is_some_and(|previous| previous != runtime_id)
        {
            let lost: Vec<_> = state
                .pending
                .iter()
                .filter(|(_, pending)| pending.sent)
                .map(|(call_id, _)| call_id.clone())
                .collect();
            for call_id in lost {
                if let Some(pending) = state.pending.remove(&call_id) {
                    let _ = pending.result.send(json!({
                        "status": "ambiguous",
                        "message": "runner restarted before returning a receipt",
                    }));
                }
            }
        }
        state.runtime_id = Some(runtime_id.into());
        state.generation += 1;
        let generation = state.generation;
        let (outbound, receiver) = mpsc::unbounded_channel();
        state.connection = Some(Connection {
            generation,
            outbound,
        });
        let mut backlog = Vec::new();
        let mut recover = Vec::new();
        for (call_id, pending) in &mut state.pending {
            if pending.sent {
                recover.push(call_id.clone());
            } else {
                pending.sent = true;
                backlog.push(pending.frame.to_string());
            }
        }
        for batch in recover.chunks(RECOVER_BATCH_LIMIT) {
            backlog.push(json!({"type": "recover", "call_ids": batch}).to_string());
        }
        let first = state.catalog.is_none();
        if first {
            state.catalog = Some(tools);
        }
        drop(state);
        if first {
            self.catalog.send_replace(Some(specs));
        }
        Ok((generation, receiver, backlog))
    }

    fn detach(&self, generation: u64) {
        let mut state = self.state();
        if state
            .connection
            .as_ref()
            .is_some_and(|connection| connection.generation == generation)
        {
            state.connection = None;
        }
    }

    fn claim(&self, generation: u64, call_id: &str) -> Option<String> {
        let mut state = self.state();
        if state
            .connection
            .as_ref()
            .is_none_or(|connection| connection.generation != generation)
        {
            return None;
        }
        let pending = state
            .pending
            .get_mut(call_id)
            .filter(|pending| !pending.sent)?;
        pending.sent = true;
        Some(pending.frame.to_string())
    }

    fn resend(&self, generation: u64, call_id: &str) -> Option<String> {
        let mut state = self.state();
        let current = state
            .connection
            .as_ref()
            .is_some_and(|connection| connection.generation == generation);
        let pending = state.pending.get_mut(call_id)?;
        pending.sent = current;
        current.then(|| pending.frame.to_string())
    }

    fn deliver(&self, call_id: &str, outcome: Value) {
        if let Some(pending) = self.state().pending.remove(call_id) {
            let _ = pending.result.send(outcome);
        }
    }

    fn cancel(&self, call_id: &str) {
        let mut state = self.state();
        if let Some(pending) = state.pending.remove(call_id)
            && pending.sent
            && let Some(connection) = &state.connection
        {
            let _ = connection.outbound.send(Outbound::Frame(
                json!({"type": "cancel", "call_id": call_id}).to_string(),
            ));
        }
    }

    async fn call(
        self: &Arc<Self>,
        spec: &RemoteSpec,
        input: Value,
        context: ToolContext<'_>,
    ) -> Result<Value, AttachmentError> {
        let session_id = context.session_id();
        if !valid_identifier(session_id) {
            return Err(AttachmentError::Transport(
                format!("session id `{session_id}` is not a valid attachment identifier").into(),
            ));
        }
        if context
            .turn_id()
            .is_some_and(|turn_id| turn_id.is_empty() || turn_id.len() > TURN_ID_LIMIT)
        {
            return Err(AttachmentError::Transport(
                "turn id is not a valid attachment turn identifier".into(),
            ));
        }
        let output_token_budget = u64::try_from(context.output_token_budget())
            .unwrap_or(u64::MAX)
            .min(JSON_SAFE_INTEGER_MAX);
        if output_token_budget == 0 {
            return Err(AttachmentError::Transport(
                "attached calls require a positive output token budget".into(),
            ));
        }
        let call_id: Box<str> = uuid::Uuid::new_v4().to_string().into();
        let now = now_ms();
        let deadline_at = now
            .saturating_add(spec.timeout_ms)
            .min(JSON_SAFE_INTEGER_MAX);
        let mut frame = json!({
            "type": "call",
            "session_id": session_id,
            "call_id": call_id,
            "model": context.model(),
            "name": spec.remote_name,
            "input": input,
            "output_token_budget": output_token_budget,
            "output_byte_budget": JSON_SAFE_INTEGER_MAX,
            "deadline_at": deadline_at,
        });
        if let Some(turn_id) = context.turn_id() {
            frame["turn_id"] = turn_id.into();
        }
        let (result, receipt) = oneshot::channel();
        {
            let mut state = self.state();
            state.pending.insert(
                call_id.clone(),
                Pending {
                    frame: frame.to_string().into(),
                    sent: false,
                    result,
                },
            );
            if let Some(connection) = &state.connection {
                let _ = connection.outbound.send(Outbound::Call(call_id.clone()));
            }
        }
        let guard = CancelOnDrop {
            inner: Arc::clone(self),
            call_id,
        };
        let waited = tokio::time::timeout(
            Duration::from_millis(deadline_at.saturating_sub(now)),
            receipt,
        )
        .await;
        drop(guard);
        match waited {
            Ok(Ok(outcome)) => Ok(outcome),
            Ok(Err(_)) => Err(AttachmentError::Closed),
            Err(_) => Err(AttachmentError::Transport(
                format!(
                    "runner `{}` returned no receipt before the call deadline",
                    self.namespace
                )
                .into(),
            )),
        }
    }

    fn turn_ended(
        &self,
        session_id: &str,
        turn_id: &str,
        hook_event_name: &str,
    ) -> Result<(), AttachmentError> {
        if !valid_identifier(session_id)
            || turn_id.is_empty()
            || turn_id.len() > TURN_ID_LIMIT
            || !matches!(hook_event_name, "Stop" | "Interrupt" | "SubagentStop")
        {
            return Err(AttachmentError::Transport(
                "turn end is not expressible in the attachment protocol".into(),
            ));
        }
        let mut state = self.state();
        let key = [session_id.into(), turn_id.into(), hook_event_name.into()];
        if state.last_turn_ended.as_ref() == Some(&key) {
            return Ok(());
        }
        state.last_turn_ended = Some(key);
        if let Some(connection) = &state.connection {
            let _ = connection.outbound.send(Outbound::Frame(
                json!({
                    "type": "turn_ended",
                    "session_id": session_id,
                    "turn_id": turn_id,
                    "hook_event_name": hook_event_name,
                })
                .to_string(),
            ));
        }
        Ok(())
    }
}

struct CancelOnDrop {
    inner: Arc<Inner>,
    call_id: Box<str>,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.inner.cancel(&self.call_id);
    }
}

#[derive(Clone)]
pub struct RemoteTools {
    tools: Vec<RemoteTool>,
}

impl fmt::Debug for RemoteTools {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_list()
            .entries(self.tools.iter().map(RemoteTool::name))
            .finish()
    }
}

impl RemoteTools {
    pub fn iter(&self) -> impl Iterator<Item = &RemoteTool> {
        self.tools.iter()
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RemoteTool> {
        self.tools.iter().find(|tool| tool.name() == name)
    }
}

impl IntoIterator for RemoteTools {
    type Item = RemoteTool;
    type IntoIter = std::vec::IntoIter<RemoteTool>;

    fn into_iter(self) -> Self::IntoIter {
        self.tools.into_iter()
    }
}

#[derive(Clone)]
pub struct RemoteTool {
    runner: Arc<Inner>,
    spec: Arc<RemoteSpec>,
}

impl fmt::Debug for RemoteTool {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RemoteTool")
            .field("name", &self.name())
            .field("remote_name", &self.remote_name())
            .finish()
    }
}

impl RemoteTool {
    #[must_use]
    pub fn name(&self) -> &str {
        self.spec.definition.name()
    }

    #[must_use]
    pub fn remote_name(&self) -> &str {
        &self.spec.remote_name
    }

    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.runner.namespace
    }
}

#[async_trait]
impl Tool for RemoteTool {
    fn definition(&self) -> ToolDefinition {
        self.spec.definition.clone()
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        self.spec.parallel_safe
    }

    async fn end_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        hook_event_name: &str,
    ) -> Result<(), ToolError> {
        Ok(self
            .runner
            .turn_ended(session_id, turn_id, hook_event_name)?)
    }

    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult {
        let input = match input {
            ToolInput::Function(input) => {
                let input: Value = serde_json::from_str(input.get())?;
                if !input.is_object() {
                    return Err(format!("`{}` requires an object input", self.name()).into());
                }
                input
            }
            ToolInput::Freeform(input) => Value::String(input),
        };
        let outcome = self.runner.call(&self.spec, input, context).await?;
        let Outcome {
            status,
            output,
            message,
        } = serde_json::from_str(&outcome.to_string())?;
        match (status, output) {
            (OutcomeStatus::Completed, Some(output)) => {
                Ok(ToolOutput::from_wire(serde_json::from_str::<
                    ToolOutputWire,
                >(output.get())?)?)
            }
            (OutcomeStatus::Completed, None) => Err(format!(
                "runner `{}` completed `{}` without an output",
                self.runner.namespace,
                self.remote_name()
            )
            .into()),
            (OutcomeStatus::Unavailable, _) => Err(format!(
                "runner `{}` could not run `{}`: {message}",
                self.runner.namespace,
                self.remote_name()
            )
            .into()),
            (OutcomeStatus::Ambiguous, _) => Err(format!(
                "runner `{}` call to `{}` has an unknown outcome: {message}",
                self.runner.namespace,
                self.remote_name()
            )
            .into()),
        }
    }
}

type Runners = Arc<Mutex<HashMap<Box<str>, RemoteRunner>>>;

pub struct RunnerHost {
    endpoint: Box<str>,
    runners: Runners,
    accept: tokio::task::JoinHandle<()>,
}

impl fmt::Debug for RunnerHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RunnerHost")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl Drop for RunnerHost {
    fn drop(&mut self) {
        self.accept.abort();
    }
}

impl RunnerHost {
    pub async fn bind(address: impl ToSocketAddrs) -> Result<Self, AttachmentError> {
        let listener = TcpListener::bind(address).await.map_err(transport_error)?;
        let address = listener.local_addr().map_err(transport_error)?;
        let runners = Runners::default();
        let accept = tokio::spawn(accept_runners(listener, Arc::clone(&runners)));
        Ok(Self {
            endpoint: format!("ws://{address}/").into(),
            runners,
            accept,
        })
    }

    #[must_use]
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn runner(
        &self,
        namespace: impl Into<String>,
        bearer: impl Into<String>,
    ) -> Result<RemoteRunner, AttachmentError> {
        let runner = RemoteRunner::new(namespace)?;
        let bearer = bearer.into();
        if bearer.trim().is_empty() {
            return Err(AttachmentError::Authentication(
                "bearer credential must not be empty".into(),
            ));
        }
        let mut runners = self.runners.lock().unwrap_or_else(PoisonError::into_inner);
        if runners
            .values()
            .any(|existing| existing.namespace() == runner.namespace())
        {
            return Err(AttachmentError::Catalog(
                format!(
                    "runner namespace `{}` is already registered",
                    runner.namespace()
                )
                .into(),
            ));
        }
        if runners.contains_key(bearer.as_str()) {
            return Err(AttachmentError::Authentication(
                "bearer credential is already assigned to another runner".into(),
            ));
        }
        runners.insert(bearer.into(), runner.clone());
        Ok(runner)
    }
}

async fn accept_runners(listener: TcpListener, runners: Runners) {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    connections.spawn(serve_socket(stream, Arc::clone(&runners)));
                }
                Err(error) => {
                    tracing::warn!(target: "nanocode_code_mode::attachment", %error, "runner host accept failed");
                }
            },
            Some(_) = connections.join_next(), if !connections.is_empty() => {}
        }
    }
}

async fn serve_socket(stream: TcpStream, runners: Runners) {
    if let Err(error) = stream.set_nodelay(true) {
        tracing::warn!(target: "nanocode_code_mode::attachment", %error, "runner socket nodelay failed");
    }
    let mut runner = None;
    let authenticate = |request: &Request, response: Response| {
        let found = request
            .headers()
            .get(http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .and_then(|bearer| {
                runners
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .get(bearer)
                    .cloned()
            });
        if let Some(found) = found {
            runner = Some(found);
            Ok(response)
        } else {
            let mut rejection = ErrorResponse::new(Some("unknown runner credential".into()));
            *rejection.status_mut() = http::StatusCode::UNAUTHORIZED;
            Err(rejection)
        }
    };
    let socket = match tokio_tungstenite::accept_hdr_async(stream, authenticate).await {
        Ok(socket) => socket,
        Err(error) => {
            tracing::info!(target: "nanocode_code_mode::attachment", %error, "runner handshake rejected");
            return;
        }
    };
    let Some(runner) = runner else { return };
    let mut frames = socket
        .with(|text: String| future::ready(Ok::<_, tungstenite::Error>(Message::text(text))))
        .filter_map(|message| {
            future::ready(match message {
                Ok(Message::Text(text)) => Some(Ok(text.as_str().to_owned())),
                Ok(Message::Binary(_)) => Some(Err(tungstenite::Error::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "runner sent a binary frame",
                )))),
                Ok(_) => None,
                Err(error) => Some(Err(error)),
            })
        });
    let result = runner.serve(&mut frames).await;
    let close = match &result {
        Ok(()) => None,
        Err(error) => {
            tracing::warn!(target: "nanocode_code_mode::attachment", namespace = runner.namespace(), %error, "runner connection ended");
            let code = if matches!(
                error,
                AttachmentError::Catalog(_) | AttachmentError::Fenced(_)
            ) {
                CloseCode::Policy
            } else {
                CloseCode::Error
            };
            Some(CloseFrame {
                code,
                reason: close_reason(&error.to_string()).to_owned().into(),
            })
        }
    };
    let _ = frames.get_mut().get_mut().close(close).await;
}

fn close_reason(reason: &str) -> &str {
    let mut end = reason.len().min(WEBSOCKET_CLOSE_REASON_LIMIT);
    while !reason.is_char_boundary(end) {
        end -= 1;
    }
    &reason[..end]
}

fn parse_catalog(
    namespace: &str,
    tools: &Value,
) -> Result<Arc<[Arc<RemoteSpec>]>, AttachmentError> {
    let entries: Vec<CatalogEntry> = serde_json::from_value(tools.clone())
        .map_err(|error| AttachmentError::Catalog(error.to_string().into()))?;
    entries
        .into_iter()
        .map(|entry| {
            let remote_name: Box<str> = entry.definition.name().into();
            let name = format!("{namespace}{NAMESPACE_SEPARATOR}{remote_name}");
            if name.len() > PUBLIC_TOOL_NAME_LIMIT {
                return Err(AttachmentError::Catalog(
                    format!("namespaced tool name `{name}` exceeds {PUBLIC_TOOL_NAME_LIMIT} bytes")
                        .into(),
                ));
            }
            let definition = match entry.definition {
                ToolDefinition::Function {
                    description,
                    strict,
                    parameters,
                    output_schema,
                    ..
                } => {
                    let mut definition = ToolDefinition::function(name, description, parameters);
                    if strict {
                        definition = definition.with_strict_parameters();
                    }
                    if let Some(output_schema) = output_schema {
                        definition = definition.with_output_schema(output_schema);
                    }
                    definition
                }
                ToolDefinition::Custom {
                    description,
                    format,
                    ..
                } => ToolDefinition::custom(name, description, format),
                definition => {
                    return Err(AttachmentError::Catalog(
                        format!(
                            "attached tool `{}` has an unsupported definition",
                            definition.name()
                        )
                        .into(),
                    ));
                }
            };
            Ok(Arc::new(RemoteSpec {
                remote_name,
                definition,
                parallel_safe: entry.parallel_safe,
                timeout_ms: entry.timeout_ms,
            }))
        })
        .collect()
}

fn parse_frame(text: &str) -> Result<RunnerFrame, AttachmentError> {
    serde_json::from_str(text)
        .map_err(|error| AttachmentError::Fenced(format!("invalid runner frame: {error}").into()))
}

async fn send<T, E>(transport: &mut T, frame: String) -> Result<(), AttachmentError>
where
    T: Sink<String, Error = E> + Unpin,
    E: fmt::Display,
{
    transport.send(frame).await.map_err(transport_error)
}

fn transport_error(error: impl fmt::Display) -> AttachmentError {
    AttachmentError::Transport(error.to_string().into())
}

fn valid_namespace(namespace: &str) -> bool {
    namespace
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_alphabetic)
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !namespace.contains(NAMESPACE_SEPARATOR)
        && !namespace.ends_with('_')
}

fn valid_identifier(value: &str) -> bool {
    value.len() <= PROTOCOL_IDENTIFIER_LIMIT
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
        })
}
