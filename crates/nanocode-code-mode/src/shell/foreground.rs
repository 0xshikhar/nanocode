//! Bounded foreground execution shared by provider tool adapters.
use super::{process, selection};
use std::{path::Path, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Output of a completed foreground command.
pub struct ForegroundOutput {
    /// Bounded standard output.
    pub stdout: String,
    /// Bounded standard error.
    pub stderr: String,
    /// Command exit status.
    pub exit_code: i32,
    /// Whether either output stream exceeded its limit.
    pub truncated: bool,
}

/// Runs Bash with the shared shell environment and process lifecycle.
/// This is local execution with host permissions, not an OS sandbox.
/// Cancellation and deadlines terminate the process group; capture stays bounded.
pub async fn run_bash(
    workspace: &Path,
    command: &str,
    timeout: Duration,
    max_stdout: usize,
    max_stderr: usize,
    session: Option<&crate::SessionEnvironment>,
) -> Result<ForegroundOutput, String> {
    let overrides = session
        .map(crate::SessionEnvironment::os_variables)
        .unwrap_or_default();
    let (environment, _) = process::sanitized_environment(&overrides);
    let shell = selection::bash_shell().ok_or("Bash executable not found")?;
    let mut spawned = process::spawn(command, workspace, &shell, false, false, &environment)
        .map_err(|error| error.to_string())?;
    struct Cleanup(process::SharedProcessGroup);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.terminate_and_disarm();
        }
    }
    let cleanup = Cleanup(spawned.process_group);
    let process::ProcessOutput::Pipes { stdout, stderr } = spawned.output else {
        return Err("foreground execution requires pipes".into());
    };
    let outcome = tokio::time::timeout(timeout, async {
        let (status, stdout, stderr) = tokio::join!(
            spawned.child.wait(),
            capture(stdout, max_stdout),
            capture(stderr, max_stderr)
        );
        let (stdout, stdout_cut) = stdout?;
        let (stderr, stderr_cut) = stderr?;
        Ok::<_, std::io::Error>(ForegroundOutput {
            stdout,
            stderr,
            exit_code: status?,
            truncated: stdout_cut || stderr_cut,
        })
    })
    .await;
    drop(cleanup);
    match outcome {
        Ok(result) => result.map_err(|error| error.to_string()),
        Err(_) => {
            let _ = spawned.child.wait().await;
            Err("Bash command timed out".into())
        }
    }
}

async fn capture<R: AsyncRead + Unpin>(
    pipe: Option<R>,
    limit: usize,
) -> std::io::Result<(String, bool)> {
    let Some(mut pipe) = pipe else {
        return Ok((String::new(), false));
    };
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 8192];
    let mut truncated = false;
    loop {
        let read = pipe.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let keep = read.min(limit.saturating_sub(bytes.len()));
        bytes.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok((String::from_utf8_lossy(&bytes).into_owned(), truncated))
}
