//! Claude without the `tools` feature: the default file tools are direct
//! client tools, and an explicit Code Mode request is rejected before any
//! provider request (the client points at a closed local port).
use nanocodex::{ClaudeHarness, Nanocodex, NanocodexError, claude::ClaudeClient};

#[tokio::test]
async fn claude_without_code_mode_runtime_rejects_explicit_code_mode() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let workspace =
        std::env::temp_dir().join(format!("nanocodex-claude-minimal-{}", std::process::id()));
    std::fs::create_dir_all(&workspace).unwrap();
    let claude = ClaudeClient::new(
        reqwest::Client::new(),
        "http://127.0.0.1:9/v1/messages",
        "synthetic-key",
    );
    let explicit = Nanocodex::builder()
        .claude(ClaudeHarness::new(claude.clone()).code_mode(true))
        .workspace(&workspace)
        .build()
        .unwrap();
    let error = explicit
        .spawn()
        .build()
        .await
        .err()
        .expect("Code Mode is unavailable");
    assert!(
        matches!(&error, NanocodexError::InvalidRequest(message) if message.contains("Code Mode")),
        "{error}"
    );
    println!("EXPLICIT_CODE_MODE rejected: {error}");
    let default = Nanocodex::builder()
        .claude(claude)
        .workspace(&workspace)
        .build()
        .unwrap();
    let (_agent, _events) = default.spawn().build().await.unwrap();
    default.shutdown().await.unwrap();
    println!("DEFAULT direct Claude file tools agent constructed");
    std::fs::remove_dir_all(&workspace).unwrap();
}
