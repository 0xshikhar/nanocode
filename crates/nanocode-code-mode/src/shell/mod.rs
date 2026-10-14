mod foreground;
pub use foreground::{ForegroundOutput, run_bash};
mod output;
mod process;
mod selection;
mod tool;

#[cfg(feature = "mcp")]
pub(crate) use process::ProcessGroupGuard;
#[cfg(feature = "code-mode")]
pub use process::ambient_sensitive_environment;
pub(crate) use tool::{ExecCommandHandler, WriteStdinHandler};

use std::{
    collections::{HashMap, VecDeque},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
    time::{Duration, Instant},
};

use serde::Serialize;
use tokio::{sync::Mutex, task::JoinHandle, time::timeout};

pub(crate) const DEFAULT_EXEC_YIELD_MS: u64 = 10_000;
pub(crate) const DEFAULT_WRITE_YIELD_MS: u64 = 250;
pub(crate) const DEFAULT_POLL_YIELD_MS: u64 = 5_000;
pub(crate) const MAX_EXEC_YIELD_MS: u64 = 30_000;
pub(crate) const MAX_POLL_YIELD_MS: u64 = 300_000;
const DRAIN_GRACE: Duration = Duration::from_secs(2);

pub(crate) struct ExecCommand {
    script: String,
    workdir: Option<String>,
    shell: Option<String>,
    login: Option<bool>,
    tty: bool,
    yield_time_ms: Option<u64>,
    max_output_tokens: Option<usize>,
}

impl ExecCommand {
    pub(crate) const fn new(
        script: String,
        workdir: Option<String>,
        shell: Option<String>,
        login: Option<bool>,
        tty: bool,
        yield_time_ms: Option<u64>,
        max_output_tokens: Option<usize>,
    ) -> Self {
        Self {
            script,
            workdir,
            shell,
            login,
            tty,
            yield_time_ms,
            max_output_tokens,
        }
    }
}

pub(crate) struct WriteStdin {
    session_id: i32,
    chars: String,
    yield_time_ms: Option<u64>,
    max_output_tokens: Option<usize>,
}

impl WriteStdin {
    pub(crate) const fn new(
        session_id: i32,
        chars: String,
        yield_time_ms: Option<u64>,
        max_output_tokens: Option<usize>,
    ) -> Self {
        Self {
            session_id,
            chars,
            yield_time_ms,
            max_output_tokens,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct ExecCommandResult {
    #[serde(skip)]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    chunk_id: Option<String>,
    wall_time_seconds: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    original_token_count: Option<usize>,
    output: String,
}

pub(crate) struct ShellSessions {
    lifecycle: Mutex<()>,
    sessions: Mutex<HashMap<i64, Arc<Session>>>,
    next_session_id: AtomicI64,
    default_shell: selection::Shell,
    environment: Arc<Vec<(OsString, OsString)>>,
}

impl ShellSessions {

    pub(crate) fn with_environment(environment: Arc<Vec<(OsString, OsString)>>) -> Self {
        Self {
            lifecycle: Mutex::new(()),
            sessions: Mutex::new(HashMap::new()),
            next_session_id: AtomicI64::new(1),
            default_shell: selection::default_user_shell(),
            environment,
        }
    }

    #[cfg(feature = "code-mode")]
    pub(crate) const fn default_shell_name(&self) -> &'static str {
        self.default_shell.name()
    }

    pub(crate) async fn execute(
        &self,
        command: ExecCommand,
        workspace: &Path,
    ) -> ExecCommandResult {
        let started_at = Instant::now();
        let session_id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        let workdir = resolve_workdir(workspace, command.workdir.as_deref());
        let shell = command.shell.as_deref().map_or_else(
            || self.default_shell.clone(),
            selection::get_shell_by_model_provided_path,
        );
        let (environment, secrets) = process::sanitized_environment(&self.environment);
        // Registration and full cleanup are one linearized lifecycle. Once a
        // process has spawned, terminate_all must either observe it in the
        // store or have completed before this execution began spawning.
        let lifecycle_guard = self.lifecycle.lock().await;
        let spawned = match process::spawn(
            &command.script,
            &workdir,
            &shell,
            command.login.unwrap_or(true),
            command.tty,
            &environment,
        ) {
            Ok(spawned) => spawned,
            Err(error) => {
                return ExecCommandResult::failed(
                    started_at.elapsed(),
                    format!(
                        "exec_command failed for `{}`: CreateProcess {{ message: {:?} }}",
                        command.script,
                        error.to_string()
                    ),
                );
            }
        };
        let session = Session::new(session_id, spawned, secrets);
        let _interaction_guard = session.interaction.lock().await;
        self.sessions
            .lock()
            .await
            .insert(session_id, Arc::clone(&session));
        drop(lifecycle_guard);

        let yield_time = duration_ms(
            command.yield_time_ms,
            DEFAULT_EXEC_YIELD_MS,
            250,
            MAX_EXEC_YIELD_MS,
        );
        let result = session
            .wait_for_output(yield_time, command.max_output_tokens, started_at)
            .await;
        if result.exit_code.is_some() {
            self.sessions.lock().await.remove(&session_id);
        }
        result
    }

    pub(crate) async fn write_stdin(&self, request: WriteStdin) -> ExecCommandResult {
        let started_at = Instant::now();
        let session_id = i64::from(request.session_id);
        let session = self.sessions.lock().await.get(&session_id).cloned();
        let Some(session) = session else {
            return ExecCommandResult::failed(
                started_at.elapsed(),
                format!(
                    "write_stdin failed: Unknown process id {}",
                    request.session_id
                ),
            );
        };

        let _interaction_guard = session.interaction.lock().await;
        if !request.chars.is_empty() {
            let written = if !session.tty {
                if request.chars == "\u{3}" {
                    session.interrupt().await
                } else {
                    return ExecCommandResult::failed(
                        started_at.elapsed(),
                        "write_stdin failed: stdin is closed for this session; rerun exec_command with tty=true to keep stdin open"
                            .to_owned(),
                    );
                }
            } else {
                session.write(&request.chars).await
            };
            if let Err(error) = written {
                return ExecCommandResult::failed(
                    started_at.elapsed(),
                    format!("write_stdin failed: {error}"),
                );
            }
        }
        let (default, minimum, maximum) = if request.chars.is_empty() {
            (DEFAULT_POLL_YIELD_MS, 5_000, MAX_POLL_YIELD_MS)
        } else {
            (DEFAULT_WRITE_YIELD_MS, 250, MAX_EXEC_YIELD_MS)
        };
        let yield_time = duration_ms(request.yield_time_ms, default, minimum, maximum);
        let result = session
            .wait_for_output(yield_time, request.max_output_tokens, started_at)
            .await;
        if result.exit_code.is_some() {
            self.sessions.lock().await.remove(&session_id);
        }
        result
    }

    pub(crate) async fn terminate_all(&self) {
        // Keep concurrent cleanup acknowledgements behind the same boundary,
        // and do not allow a process to spawn between draining and reaping.
        let _lifecycle_guard = self.lifecycle.lock().await;
        let sessions = {
            let mut store = self.sessions.lock().await;
            store
                .drain()
                .map(|(_, session)| session)
                .collect::<Vec<_>>()
        };
        for session in sessions {
            session.terminate().await;
        }
    }
}

struct Session {
    id: i64,
    tty: bool,
    interaction: Mutex<()>,
    child: Mutex<process::ProcessChild>,
    stdin: Mutex<Option<process::ProcessStdin>>,
    process_group: process::SharedProcessGroup,
    drains: Mutex<Option<Vec<JoinHandle<()>>>>,
    captured: Arc<Mutex<CapturedOutput>>,
    secrets: Vec<String>,
}

impl Session {
    fn new(id: i64, spawned: process::SpawnedProcess, secrets: Vec<String>) -> Arc<Self> {
        let tty = matches!(spawned.stdin.as_ref(), Some(process::ProcessStdin::Pty(_)));
        let captured = Arc::new(Mutex::new(CapturedOutput::default()));
        let drains = match spawned.output {
            process::ProcessOutput::Pipes { stdout, stderr } => vec![
                tokio::spawn(output::drain(stdout, Arc::clone(&captured), usize::MAX)),
                tokio::spawn(output::drain(stderr, Arc::clone(&captured), usize::MAX)),
            ],
            process::ProcessOutput::Pty(reader) => vec![output::drain_blocking(
                reader,
                Arc::clone(&captured),
                usize::MAX,
            )],
        };
        Arc::new(Self {
            id,
            tty,
            interaction: Mutex::new(()),
            child: Mutex::new(spawned.child),
            stdin: Mutex::new(spawned.stdin),
            process_group: spawned.process_group,
            drains: Mutex::new(Some(drains)),
            captured,
            secrets,
        })
    }

    async fn terminate(&self) {
        // Signal first: a direct caller can hold the interaction lock while
        // awaiting this child, and waiting for that lock before terminating
        // would make cancellation wait for the command's full yield timeout.
        if let Err(error) = self.process_group.terminate_and_disarm() {
            tracing::warn!(
                shell.session.id = self.id,
                %error,
                "failed to terminate shell process group"
            );
        }
        // The active interaction observes the signal and releases the child
        // wait path. Own it before the idempotent reap/drain cleanup so a
        // cancellation acknowledgement is a real process-cleanup boundary.
        let _interaction = self.interaction.lock().await;
        // No write can remain active after taking the interaction lock, so the
        // stdin handle can now be dropped without delaying the kill signal.
        self.stdin.lock().await.take();
        if let Err(error) = self.child.lock().await.wait().await {
            tracing::warn!(
                shell.session.id = self.id,
                %error,
                "failed to reap terminated shell process"
            );
        }
        self.finish_drains().await;
    }

    async fn interrupt(&self) -> std::io::Result<()> {
        self.process_group.interrupt()
    }

    async fn write(&self, chars: &str) -> std::io::Result<()> {
        let mut stdin = self.stdin.lock().await;
        let stdin = stdin.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin is closed")
        })?;
        stdin.write(chars.as_bytes()).await
    }

    async fn wait_for_output(
        &self,
        yield_time: Duration,
        max_output_tokens: Option<usize>,
        started_at: Instant,
    ) -> ExecCommandResult {
        let status = {
            let mut child = self.child.lock().await;
            timeout(yield_time, child.wait()).await
        };
        let exit_code = match status {
            Ok(Ok(exit_code)) => {
                let _ = self.process_group.terminate_and_disarm();
                self.finish_drains().await;
                Some(exit_code)
            }
            Ok(Err(error)) => {
                let _ = self.process_group.terminate_and_disarm();
                let message = format!("failed to wait for shell command: {error}");
                self.captured
                    .lock()
                    .await
                    .push(message.as_bytes(), usize::MAX);
                Some(1)
            }
            Err(_) => None,
        };
        let (output, original_token_count) = self.take_output(max_output_tokens).await;
        let chunk_id = match generate_chunk_id() {
            Ok(chunk_id) => Some(chunk_id),
            Err(error) => {
                tracing::warn!(%error, "failed to generate shell output chunk ID");
                None
            }
        };
        ExecCommandResult {
            error: None,
            chunk_id,
            wall_time_seconds: started_at.elapsed().as_secs_f64(),
            exit_code,
            session_id: exit_code.is_none().then_some(self.id),
            original_token_count,
            output,
        }
    }

    async fn finish_drains(&self) {
        let handles = self.drains.lock().await.take();
        let Some(handles) = handles else {
            return;
        };
        let deadline = tokio::time::Instant::now() + DRAIN_GRACE;
        for mut handle in handles {
            if tokio::time::timeout_at(deadline, &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = handle.await;
            }
        }
    }

    async fn take_output(&self, max_output_tokens: Option<usize>) -> (String, Option<usize>) {
        let captured = self.captured.lock().await.take();
        let raw = String::from_utf8_lossy(&captured.with_omission_marker()).into_owned();
        let limit = output::effective_token_limit(max_output_tokens);
        let (output, _) = output::redact_and_limit(raw, &self.secrets, limit);
        (output, Some(captured.total_bytes.saturating_add(3) / 4))
    }
}

#[derive(Default)]
pub(super) struct CapturedOutput {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    omitted_bytes: usize,
}

impl CapturedOutput {
    pub(super) fn push(&mut self, bytes: &[u8], limit: usize) {
        if bytes.is_empty() {
            return;
        }
        if limit == 0 {
            self.omitted_bytes = self.omitted_bytes.saturating_add(bytes.len());
            return;
        }

        let head_budget = limit / 2;
        let tail_budget = limit.saturating_sub(head_budget);
        let head_len = head_budget.saturating_sub(self.head.len()).min(bytes.len());
        self.head.extend_from_slice(&bytes[..head_len]);
        let tail_bytes = &bytes[head_len..];
        if tail_bytes.len() >= tail_budget {
            let kept_from = tail_bytes.len().saturating_sub(tail_budget);
            self.omitted_bytes = self
                .omitted_bytes
                .saturating_add(self.tail.len())
                .saturating_add(kept_from);
            self.tail.clear();
            self.tail.extend(&tail_bytes[kept_from..]);
        } else {
            self.tail.extend(tail_bytes);
            let excess = self.tail.len().saturating_sub(tail_budget);
            if excess > 0 {
                drop(self.tail.drain(..excess));
                self.omitted_bytes = self.omitted_bytes.saturating_add(excess);
            }
        }
    }

    fn take(&mut self) -> CapturedChunk {
        let head = std::mem::take(&mut self.head);
        let tail = std::mem::take(&mut self.tail);
        let omitted_bytes = std::mem::take(&mut self.omitted_bytes);
        CapturedChunk {
            total_bytes: head
                .len()
                .saturating_add(tail.len())
                .saturating_add(omitted_bytes),
            head,
            tail,
            omitted_bytes,
        }
    }
}

struct CapturedChunk {
    head: Vec<u8>,
    tail: VecDeque<u8>,
    omitted_bytes: usize,
    total_bytes: usize,
}

impl CapturedChunk {
    fn with_omission_marker(&self) -> Vec<u8> {
        let marker = (self.omitted_bytes > 0)
            .then(|| format!("... {} bytes omitted ...", self.omitted_bytes));
        let marker_length = marker.as_ref().map_or(0, |marker| marker.len() + 2);
        let mut output = Vec::with_capacity(
            self.head
                .len()
                .saturating_add(self.tail.len())
                .saturating_add(marker_length),
        );
        output.extend_from_slice(&self.head);
        if let Some(marker) = marker {
            output.push(b'\n');
            output.extend_from_slice(marker.as_bytes());
            output.push(b'\n');
        }
        output.extend(self.tail.iter().copied());
        output
    }
}

impl ExecCommandResult {
    fn failed(wall_time: Duration, output: String) -> Self {
        Self {
            error: Some(output.clone()),
            chunk_id: None,
            wall_time_seconds: wall_time.as_secs_f64(),
            exit_code: Some(1),
            session_id: None,
            original_token_count: None,
            output,
        }
    }
}

fn resolve_workdir(workspace: &Path, requested: Option<&str>) -> PathBuf {
    let requested = requested.filter(|workdir| !workdir.is_empty());
    match requested.map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => workspace.join(path),
        None => workspace.to_owned(),
    }
}

fn duration_ms(requested: Option<u64>, default: u64, minimum: u64, maximum: u64) -> Duration {
    let requested = requested.unwrap_or(default);
    Duration::from_millis(requested.clamp(minimum, maximum))
}

fn generate_chunk_id() -> Result<String, getrandom::Error> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0_u8; 3];
    getrandom::fill(&mut bytes)?;
    Ok(bytes
        .into_iter()
        .flat_map(|byte| {
            [
                char::from(HEX[usize::from(byte >> 4)]),
                char::from(HEX[usize::from(byte & 0x0f)]),
            ]
        })
        .collect())
}
