use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use crate::{
    ClaudeAccessToken, ClaudeAuthFuture, ClaudeAuthUnavailable, ClaudeClient, ClaudeTokenSource,
    RefreshingClaudeAuth,
};

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";

struct ClaudeCodeLogin;

impl ClaudeCodeLogin {
    async fn stored() -> Result<Vec<u8>, ClaudeAuthUnavailable> {
        let mut keychain = None;
        if cfg!(target_os = "macos") {
            let output = tokio::process::Command::new("security")
                .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
                .output()
                .await
                .map_err(|error| unavailable(&format!("keychain lookup failed: {error}")))?;
            if output.status.success() {
                return Ok(output.stdout);
            }
            keychain = Some(format!(
                "macOS keychain item {KEYCHAIN_SERVICE:?}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let home = nanocode_home::AgentHome::from_env()
            .map_err(|error| unavailable(&error.to_string()))?;
        let path = home.claude_home().join(".credentials.json");
        tokio::fs::read(&path).await.map_err(|error| {
            let file = format!("{}: {error}", path.display());
            unavailable(&match keychain {
                Some(keychain) => format!(
                    "no Claude Code login found ({keychain}; {file}); run `claude` to log in"
                ),
                None => format!("no Claude Code login found ({file}); run `claude` to log in"),
            })
        })
    }
}

impl ClaudeTokenSource for ClaudeCodeLogin {
    fn refresh(&self) -> ClaudeAuthFuture<'_, Result<ClaudeAccessToken, ClaudeAuthUnavailable>> {
        Box::pin(async {
            let stored: serde_json::Value = serde_json::from_slice(&Self::stored().await?)
                .map_err(|error| unavailable(&format!("invalid Claude Code login: {error}")))?;
            let oauth = &stored["claudeAiOauth"];
            let token = oauth["accessToken"]
                .as_str()
                .ok_or_else(|| unavailable("Claude Code login has no access token"))?;
            let expires_at = oauth["expiresAt"]
                .as_u64()
                .ok_or_else(|| unavailable("Claude Code login has no expiry"))?;
            let expires_at = UNIX_EPOCH + Duration::from_millis(expires_at);
            if expires_at <= web_time::SystemTime::now() {
                return Err(unavailable(
                    "Claude Code login expired; run `claude` to refresh it",
                ));
            }
            ClaudeAccessToken::new(token, expires_at)
        })
    }
}

fn unavailable(reason: &str) -> ClaudeAuthUnavailable {
    tracing::warn!(target: "nanocodex_claude", reason, "Claude Code login unavailable");
    ClaudeAuthUnavailable::new(reason.to_owned())
}

impl Default for ClaudeClient {
    fn default() -> Self {
        nanocode_tls::install_default_rustls_crypto_provider();
        Self::subscription(
            reqwest::Client::new(),
            Arc::new(RefreshingClaudeAuth::new(
                Arc::new(ClaudeCodeLogin),
                Duration::ZERO,
            )),
        )
    }
}
