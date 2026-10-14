use std::{
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

use crate::{
    ClaudeAccessToken, ClaudeAuthFuture, ClaudeAuthUnavailable, ClaudeClient, ClaudeTokenSource,
    RefreshingClaudeAuth,
};

const KEYCHAIN_SERVICE: &str = "Claude Code-credentials";
const SECURE_STORAGE_DIR_ENV: &str = "CLAUDE_SECURESTORAGE_CONFIG_DIR";

struct ClaudeCodeLogin;

fn nfc(path: &std::ffi::OsStr) -> String {
    use unicode_normalization::UnicodeNormalization as _;
    path.to_string_lossy().nfc().collect()
}

fn keychain_service() -> String {
    use sha2::Digest as _;
    let scoped = match std::env::var_os(SECURE_STORAGE_DIR_ENV) {
        Some(directory) => Some(directory).filter(|directory| !directory.is_empty()),
        None => std::env::var_os(nanocode_home::CLAUDE_CONFIG_DIR_ENV)
            .filter(|directory| !directory.is_empty()),
    };
    match scoped {
        Some(directory) => {
            let digest = sha2::Sha256::digest(nfc(&directory).as_bytes());
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            format!("{KEYCHAIN_SERVICE}-{}", &hex[..8])
        }
        None => KEYCHAIN_SERVICE.to_owned(),
    }
}

fn keychain_account() -> Option<String> {
    let user = std::env::var("USER").ok().filter(|user| !user.is_empty())?;
    Some(
        if user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        {
            user
        } else {
            "claude-code-user".to_owned()
        },
    )
}

fn credentials_path(home: &nanocode_home::AgentHome) -> std::path::PathBuf {
    let directory = match std::env::var_os(SECURE_STORAGE_DIR_ENV) {
        Some(directory) if !directory.is_empty() => std::path::PathBuf::from(nfc(&directory)),
        Some(_) => home
            .user_home()
            .map_or_else(|| home.claude_home().to_path_buf(), |user| user.join(".claude")),
        None => home.claude_home().to_path_buf(),
    };
    directory.join(".credentials.json")
}

impl ClaudeCodeLogin {
    async fn stored() -> Result<Vec<u8>, ClaudeAuthUnavailable> {
        let mut keychain = None;
        if cfg!(target_os = "macos") {
            let service = keychain_service();
            let mut command = tokio::process::Command::new("security");
            command.arg("find-generic-password");
            if let Some(account) = keychain_account() {
                command.args(["-a", &account]);
            }
            let output = command
                .args(["-w", "-s", &service])
                .output()
                .await
                .map_err(|error| unavailable(&format!("keychain lookup failed: {error}")))?;
            if output.status.success() {
                return Ok(output.stdout);
            }
            keychain = Some(format!(
                "macOS keychain item {service:?}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let home = nanocode_home::AgentHome::from_env()
            .map_err(|error| unavailable(&error.to_string()))?;
        let path = credentials_path(&home);
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
