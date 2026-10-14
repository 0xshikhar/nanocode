//! Claude-style Bash capability adapter backed by an injected executor.
//!
//! A host supplies its authorized execution environment. The executor enforces the requested
//! wall-clock deadline (including termination of the command and descendants)
//! and bound output *while capturing it*. Truncating a returned value here is
//! only a second line of defense; it cannot bound an executor's own memory use.
//! No subprocess, current directory, or host shell is created by this module.

use crate::SessionEnvironment;
use serde_json::{Value, json};

/// Maximum command size, measured in UTF-8 bytes.
pub const MAX_COMMAND_BYTES: usize = 1024 * 1024;
/// Maximum description size, measured in UTF-8 bytes.
pub const MAX_DESCRIPTION_BYTES: usize = 1024;
/// Maximum returned bytes for each of stdout and stderr.
pub const MAX_STREAM_BYTES: usize = 30_000;
/// Default deadline passed to the sandbox executor, in milliseconds.
pub const DEFAULT_TIMEOUT_MS: u64 = 120_000;
/// Maximum accepted deadline, in milliseconds.
pub const MAX_TIMEOUT_MS: u64 = 600_000;

/// One foreground command, with limits that the host executor must enforce.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BashRequest {
    /// UTF-8 shell command, to interpret in the host-authorized environment.
    pub command: String,
    /// Optional human-readable intent (never shell input).
    pub description: Option<String>,
    /// Deadline including command termination and cleanup.
    pub timeout_ms: u64,
    /// Maximum stdout bytes to capture before truncation.
    pub max_stdout_bytes: usize,
    /// Maximum stderr bytes to capture before truncation.
    pub max_stderr_bytes: usize,
    /// Launching session identity. Executors must export it to the command
    /// with [`Self::apply_session`], which overrides caller-supplied values
    /// and clears inherited ones when no session is bound.
    pub session: Option<SessionEnvironment>,
}

impl BashRequest {
    /// Exports this request's session identity (`CODEX_THREAD_ID` and
    /// `NANOCODEX_ROOT_SESSION_ID`) to `command`. Call it after any other
    /// environment configuration so a spoofed value cannot survive.
    #[cfg(not(target_family = "wasm"))]
    pub fn apply_session(&self, command: &mut std::process::Command) {
        SessionEnvironment::apply_or_clear(self.session.as_ref(), command);
    }
}

/// Bounded command result supplied by the executor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BashResult {
    /// Captured UTF-8 stdout (lossy decoding, if needed, is the host's job).
    pub stdout: String,
    /// Captured UTF-8 stderr (lossy decoding, if needed, is the host's job).
    pub stderr: String,
    /// Process exit status; a nonzero code remains a result, not an adapter error.
    pub exit_code: i32,
    /// Whether either stream was truncated by the executor.
    pub truncated: bool,
}

/// Host-implemented, explicitly sandboxed asynchronous command capability.
///
/// This trait does not grant shell access by itself. Implementors must isolate
/// the command, cap capture before allocating unbounded output, and kill the
/// command (including descendants) on timeout or cancellation, and export the
/// request's session identity with [`BashRequest::apply_session`]. An executor
/// failure, including a timeout, is returned as an error.
pub trait SandboxBashExecutor: Send + Sync {
    /// Execute a foreground command in the host-authorized sandbox.
    fn execute(
        &self,
        request: BashRequest,
    ) -> impl std::future::Future<Output = Result<BashResult, String>> + Send;
}

/// Outcome of a foreground or background-capable executor.
pub enum BashExecution {
    /// The command has finished.
    Completed(BashResult),
    /// The command continues as a session-owned task.
    Background {
        /// Identifier accepted by TaskOutput and TaskStop.
        task_id: String,
    },
}

/// Host-authorized command execution, local or isolated by the embedding host.
/// Implementations bound capture and terminate commands on timeout/cancellation.
pub trait BashExecutor: Send + Sync {
    /// Execute with optional background ownership. Basic executors support foreground only.
    fn execute_with_background(
        &self,
        request: BashRequest,
        background: bool,
    ) -> impl std::future::Future<Output = Result<BashExecution, String>> + Send {
        async move {
            if background {
                return Err("this executor does not support background commands".into());
            }
            self.execute(request).await.map(BashExecution::Completed)
        }
    }

    /// Execute a bounded foreground command.
    fn execute(
        &self,
        request: BashRequest,
    ) -> impl std::future::Future<Output = Result<BashResult, String>> + Send;
}

impl<E: SandboxBashExecutor> BashExecutor for E {
    async fn execute(&self, request: BashRequest) -> Result<BashResult, String> {
        SandboxBashExecutor::execute(self, request).await
    }
}

/// A Claude Bash tool facade backed by the supplied execution capability.
pub struct ClaudeBash<E: BashExecutor> {
    executor: E,
}

impl<E: BashExecutor> ClaudeBash<E> {
    /// Install a host-authorized executor.
    pub const fn new(executor: E) -> Self {
        Self { executor }
    }

    /// Model-visible Claude Bash tool definition; no Codex schema is exposed.
    #[must_use]
    pub fn definitions() -> Vec<Value> {
        vec![json!({
            "name": "Bash",
            "description": "Run a foreground command in the host-authorized execution environment. Use run_in_background for a task ID, then TaskOutput or TaskStop. Sandbox bypass is unavailable.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "command": {"type": "string", "minLength": 1, "maxLength": MAX_COMMAND_BYTES},
                    "description": {"type": "string", "maxLength": MAX_DESCRIPTION_BYTES},
                    "timeout": {"type": "integer", "minimum": 1, "maximum": 7_200_000, "default": DEFAULT_TIMEOUT_MS},
                    "run_in_background": {"type": "boolean", "description": "Return a task ID immediately; retrieve or stop it with TaskOutput/TaskStop."},
                    "dangerouslyDisableSandbox": {"type": "boolean", "description": "Unsupported: sandbox bypass is never permitted."}
                },
                "required": ["command"],
                "additionalProperties": false
            }
        })]
    }

    /// Validate a Bash invocation and delegate it to the injected executor.
    ///
    /// Completed commands return JSON text containing `stdout`, `stderr`,
    /// `exit_code`, and `truncated`. Escaped JSON is under 64 KiB even for
    /// control-character-heavy output. A command's nonzero exit is reported in
    /// the result instead of being turned into an adapter error. Background
    /// execution returns a `task_id` and running status.
    pub async fn execute(&self, name: &str, input: Value) -> Result<String, String> {
        self.execute_in_session(name, input, None).await
    }

    /// Like [`Self::execute`], launching the command on behalf of `session`.
    ///
    /// The executor receives the identity in [`BashRequest::session`] and
    /// exports it as `CODEX_THREAD_ID` and `NANOCODEX_ROOT_SESSION_ID`.
    pub async fn execute_in_session(
        &self,
        name: &str,
        input: Value,
        session: Option<SessionEnvironment>,
    ) -> Result<String, String> {
        if name != "Bash" {
            return Err(format!("unknown Claude Bash tool: {name}"));
        }
        let fields = input.as_object().ok_or("Bash input must be an object")?;
        for key in fields.keys() {
            if !matches!(
                key.as_str(),
                "command"
                    | "description"
                    | "timeout"
                    | "run_in_background"
                    | "dangerouslyDisableSandbox"
            ) {
                return Err(format!("unsupported Bash option: {key}"));
            }
        }
        // Presence is rejected, even when false: callers cannot negotiate a bypass.
        if fields.contains_key("dangerouslyDisableSandbox") {
            return Err("dangerouslyDisableSandbox is never supported".into());
        }
        let background = fields
            .get("run_in_background")
            .map_or(Some(false), Value::as_bool)
            .ok_or("invalid run_in_background")?;
        let command = fields
            .get("command")
            .and_then(Value::as_str)
            .ok_or("missing or invalid command")?;
        if command.trim().is_empty() || command.len() > MAX_COMMAND_BYTES {
            return Err(format!(
                "command must be nonblank and at most {MAX_COMMAND_BYTES} bytes"
            ));
        }
        let description = match fields.get("description") {
            None => None,
            Some(value) => {
                let text = value.as_str().ok_or("invalid description")?;
                if text.len() > MAX_DESCRIPTION_BYTES {
                    return Err(format!("description exceeds {MAX_DESCRIPTION_BYTES} bytes"));
                }
                Some(text.to_owned())
            }
        };
        let timeout_ms = fields
            .get("timeout")
            .map_or(
                Some(if background {
                    1_800_000
                } else {
                    DEFAULT_TIMEOUT_MS
                }),
                Value::as_u64,
            )
            .ok_or("invalid timeout")?;
        let maximum = if background {
            7_200_000
        } else {
            MAX_TIMEOUT_MS
        };
        if !(1..=maximum).contains(&timeout_ms) {
            return Err(format!("timeout must be 1 to {maximum} milliseconds"));
        }
        let request = BashRequest {
            command: command.to_owned(),
            description,
            timeout_ms,
            max_stdout_bytes: MAX_STREAM_BYTES,
            max_stderr_bytes: MAX_STREAM_BYTES,
            session,
        };
        let result = self
            .executor
            .execute_with_background(request, background)
            .await
            .map_err(|e| {
                let (bounded, _) = cap_utf8(&e, MAX_STREAM_BYTES);
                format!("command executor: {bounded}")
            })?;
        let result = match result {
            BashExecution::Completed(result) => result,
            BashExecution::Background { task_id } => {
                return Ok(json!({
                    "task_id": task_id, "status": "running", "background": true,
                })
                .to_string());
            }
        };
        let (stdout, stdout_cut) = cap_utf8(&result.stdout, MAX_STREAM_BYTES);
        let (stderr, stderr_cut) = cap_utf8(&result.stderr, MAX_STREAM_BYTES);
        Ok(json!({
            "stdout": stdout,
            "stderr": stderr,
            "exit_code": result.exit_code,
            "interrupted": false,
            "truncated": result.truncated || stdout_cut || stderr_cut
        })
        .to_string())
    }
}

fn cap_utf8(value: &str, max: usize) -> (&str, bool) {
    if value.len() <= max {
        return (value, false);
    }
    let mut end = max;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], true)
}
