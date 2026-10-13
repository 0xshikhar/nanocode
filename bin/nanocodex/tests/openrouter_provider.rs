//! Shipped-CLI journey: `--provider.openrouter` drives a Code Mode turn through
//! OpenRouter Chat Completions. The free-form `exec` tool is offered as a
//! function tool, the returned call runs as a custom tool, and its history is
//! replayed as Chat tool messages, following the managed gateway contract in
//! docs/AGGREGATOR_ROUTING.md (classified errors, refused redirects, fixed
//! endpoint, closed catalog). Only Chat Completions inference is synthetic.
#![cfg(unix)]
#[path = "support/local_cli.rs"]
mod local_cli;

use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::post,
};
use eyre::{Result, eyre};
use futures_util::StreamExt as _;
use serde_json::{Value, json};
use tokio::{net::TcpListener, process::Command, time::timeout};

const CELL: &str = "const r = await tools.exec_command({cmd: \"printf ok > marker.txt\", login: false});\n\
text(JSON.stringify(r));";

/// One received request: its Authorization header and JSON body.
type Request = (Option<String>, Value);

/// How the synthetic OpenRouter endpoint answers.
#[derive(Clone, Copy, Default, PartialEq)]
enum Behaviour {
    /// A tool turn then a final turn, without `usage.cost`.
    #[default]
    Plain,
    /// The same turns, each carrying OpenRouter's `usage.cost`.
    Costed,
    /// The first attempt goes silent after one chunk; later attempts answer.
    StallFirst,
    /// HTTP 401 whose body echoes the key and an Authorization header.
    Reject,
    /// HTTP 307 to another server.
    Redirect,
    /// The first attempt names an unregistered tool alias; the retry returns
    /// the exec call with its cell unwrapped from the `input` JSON.
    BadAliasFirst,
}

#[derive(Clone, Default)]
struct Recorded(Arc<Mutex<Vec<Request>>>, Behaviour, Option<String>);

/// Turn 1 charge: $0.000123 (prompt $0.0001). Turn 2: $0.000045.
const TURN_COSTS: [(f64, f64); 2] = [(0.000_123, 0.000_1), (0.000_045, 0.000_04)];
const SECRET: &str = "sk-or-v1-0123456789abcdef";

#[tokio::test]
async fn openrouter_turn_runs_exec_as_a_custom_tool_and_replays_it() -> Result<()> {
    let recorded = Recorded::default();
    let base = serve(recorded.clone()).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) =
        run_cli(workspace.path(), &base, &["--model", "glm-5.3"]).await?;
    assert!(success, "CLI failed: {stderr}");
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("marker.txt"))?,
        "ok",
        "the exec cell did not run"
    );
    let requests = recorded.0.lock().unwrap().clone();
    assert_eq!(
        requests.len(),
        2,
        "expected one tool turn and one final turn"
    );

    let (authorization, first) = &requests[0];
    assert_eq!(authorization.as_deref(), Some("Bearer test-key"));
    assert_eq!(first["model"], "z-ai/glm-5.3");
    assert_eq!(first["stream"], true);
    assert_eq!(first["provider"], json!({ "require_parameters": true }));
    assert!(
        first.get("input").is_none(),
        "Responses input leaked: {first}"
    );
    assert!(
        !first.to_string().contains("additional_tools"),
        "additional_tools leaked to the provider"
    );
    let exec = exec_alias(first)?;

    let (_, second) = &requests[1];
    let messages = second["messages"]
        .as_array()
        .ok_or_else(|| eyre!("no messages"))?;
    let call = messages
        .iter()
        .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
        .find(|call| call["id"] == "call_exec")
        .ok_or_else(|| eyre!("the exec call was not replayed: {second}"))?;
    assert_eq!(call["function"]["name"], exec.as_str());
    let replayed: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap_or_default())?;
    assert_eq!(replayed, json!({ "input": CELL }));
    let output = messages
        .iter()
        .find(|message| message["role"] == "tool" && message["tool_call_id"] == "call_exec")
        .ok_or_else(|| eyre!("the exec output was not replayed: {second}"))?;
    assert!(
        output["content"]
            .as_str()
            .is_some_and(|text| text.contains("Script completed")),
        "unexpected exec output: {output}"
    );

    let exec_call = events
        .iter()
        .find(|event| event["type"] == "tool.call" && event["payload"]["tool"] == "exec")
        .ok_or_else(|| eyre!("no exec tool.call: {events:?}"))?;
    assert_eq!(exec_call["payload"]["arguments"], CELL);
    let completed = events
        .iter()
        .find(|event| event["type"] == "run.completed")
        .ok_or_else(|| eyre!("run.completed missing: {events:?}"))?;
    assert_eq!(completed["payload"]["model"], "@cf/zai-org/glm-5.3");
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "assistant.message"
                && event["payload"]["text"] == "The marker is written."),
        "final answer missing: {events:?}"
    );
    Ok(())
}

#[tokio::test]
async fn openrouter_reported_charges_replace_rate_estimates() -> Result<()> {
    let completed = |events: &[Value]| {
        events
            .iter()
            .find(|event| event["type"] == "run.completed")
            .map(|event| event["payload"].clone())
            .ok_or_else(|| eyre!("run.completed missing: {events:?}"))
    };
    // With usage.cost on every reply, the run reports exactly OpenRouter's charge.
    let base = serve(Recorded(Arc::default(), Behaviour::Costed, None)).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) = run_cli(workspace.path(), &base, &[]).await?;
    assert!(success, "CLI failed: {stderr}");
    let payload = completed(&events)?;
    assert_eq!(payload["cost_status"], "reported_by_provider");
    assert_eq!(payload["estimated_cost"]["usd"], "0.000168");
    assert_eq!(payload["estimated_cost"]["input_usd"], "0.00014");
    assert_eq!(payload["estimated_cost"]["output_usd"], "0.000028");

    // Without it, the built-in GLM-5.3 rates still price the same tokens.
    let base = serve(Recorded::default()).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) = run_cli(workspace.path(), &base, &[]).await?;
    assert!(success, "CLI failed: {stderr}");
    let payload = completed(&events)?;
    assert_eq!(payload["cost_status"], "estimated_from_usage");
    assert_ne!(payload["estimated_cost"]["usd"], "0.000168");
    Ok(())
}

#[tokio::test]
async fn openrouter_stalled_stream_fails_the_attempt_and_retries() -> Result<()> {
    let recorded = Recorded(Arc::default(), Behaviour::StallFirst, None);
    let base = serve(recorded.clone()).await?;
    let workspace = tempfile::tempdir()?;
    let started = std::time::Instant::now();
    let (success, events, stderr) = run_cli(
        workspace.path(),
        &base,
        &["--provider.openrouter.stall-timeout", "1"],
    )
    .await?;
    assert!(success, "CLI failed after the stall: {stderr}");
    assert!(
        started.elapsed() < Duration::from_secs(30),
        "the stall was not cut short"
    );
    let failed = events
        .iter()
        .find(|event| event["type"] == "model.attempt.failed")
        .ok_or_else(|| eyre!("the stalled attempt did not fail: {events:?}"))?;
    assert_eq!(failed["payload"]["error_class"], "https_timeout");
    assert_eq!(failed["payload"]["retryable"], true);
    assert!(
        failed["payload"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("sent no data for 1 s")),
        "unclear stall error: {failed}"
    );
    assert_eq!(
        recorded.0.lock().unwrap().len(),
        3,
        "stalled attempt, retry, final turn"
    );
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("marker.txt"))?,
        "ok"
    );
    Ok(())
}

#[tokio::test]
async fn openrouter_rejections_and_redirects_are_classified_without_bodies() -> Result<()> {
    // As on the managed route, no provider body text reaches any output.
    let base = serve(Recorded(Arc::default(), Behaviour::Reject, None)).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) =
        run_cli_with_key(workspace.path(), &base, Some(SECRET), &[]).await?;
    assert!(!success, "a rejected key succeeded");
    let output = format!("{events:?}{stderr}");
    assert!(!output.contains(SECRET), "the key leaked into an error");
    assert!(
        !output.contains("No auth credentials"),
        "the provider body was quoted"
    );
    assert!(
        output.contains("the gateway rejected the request"),
        "no classified error: {output}"
    );
    assert!(output.contains("401"), "the status was lost: {output}");

    // A redirect is refused and never followed.
    let elsewhere = Recorded::default();
    let target = serve(elsewhere.clone()).await?;
    let base = serve(Recorded(
        Arc::default(),
        Behaviour::Redirect,
        Some(format!("{target}/chat/completions")),
    ))
    .await?;
    let (success, events, stderr) = run_cli(workspace.path(), &base, &[]).await?;
    assert!(!success, "a redirected request succeeded");
    assert!(
        format!("{events:?}{stderr}").contains("redirect, which is refused"),
        "no redirect refusal: {events:?} {stderr}"
    );
    assert!(
        elsewhere.0.lock().unwrap().is_empty(),
        "the redirect was followed"
    );
    Ok(())
}

#[tokio::test]
async fn openrouter_unknown_tool_alias_fails_closed_and_raw_custom_input_runs() -> Result<()> {
    let recorded = Recorded(Arc::default(), Behaviour::BadAliasFirst, None);
    let base = serve(recorded.clone()).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) = run_cli(workspace.path(), &base, &[]).await?;
    assert!(success, "CLI failed: {stderr}");
    let failed = events
        .iter()
        .find(|event| event["type"] == "model.attempt.failed")
        .ok_or_else(|| eyre!("the unknown alias was dispatched: {events:?}"))?;
    assert!(
        failed["payload"]["error"]
            .as_str()
            .is_some_and(|error| error.contains("normalize_tool_alias_unregistered_wire")),
        "unexpected failure: {failed}"
    );
    // The retry's unwrapped cell still ran exactly as written.
    assert_eq!(
        std::fs::read_to_string(workspace.path().join("marker.txt"))?,
        "ok"
    );
    let exec = events
        .iter()
        .find(|event| event["type"] == "tool.call" && event["payload"]["tool"] == "exec")
        .ok_or_else(|| eyre!("no exec call"))?;
    assert_eq!(exec["payload"]["arguments"], CELL);
    Ok(())
}

#[tokio::test]
async fn openrouter_sends_the_managed_catalog_wire_ids() -> Result<()> {
    for (model, wire) in [
        ("kimi-k3", "moonshotai/kimi-k3"),
        ("mimo-v2.6-pro", "xiaomi/mimo-v2.6-pro"),
        ("gpt-6-astra", "openai/gpt-6-astra"),
    ] {
        let recorded = Recorded::default();
        let base = serve(recorded.clone()).await?;
        let workspace = tempfile::tempdir()?;
        let (success, _, stderr) = run_cli(workspace.path(), &base, &["--model", model]).await?;
        assert!(success, "{model} failed: {stderr}");
        let requests = recorded.0.lock().unwrap().clone();
        assert!(
            requests.iter().all(|(_, body)| body["model"] == wire
                && body["provider"] == json!({ "require_parameters": true })
                && body.get("parallel_tool_calls").is_none()),
            "{model}: unexpected request: {:?}",
            requests.first()
        );
    }
    Ok(())
}

#[tokio::test]
async fn openrouter_refuses_missing_keys_and_unserved_models() -> Result<()> {
    let recorded = Recorded::default();
    let base = serve(recorded.clone()).await?;
    let workspace = tempfile::tempdir()?;

    let (success, _, stderr) = run_cli_with_key(workspace.path(), &base, None, &[]).await?;
    assert!(!success);
    assert!(
        stderr.contains("OPENROUTER_API_KEY"),
        "unexpected error: {stderr}"
    );

    // A model the gateway does not serve is refused before anything is sent.
    let (success, _, stderr) = run_cli(workspace.path(), &base, &["--model", "sol"]).await?;
    assert!(!success, "sol unexpectedly succeeded");
    assert!(
        stderr.contains(
            "--provider.openrouter serves glm-5.3, gpt-6-astra, kimi-k3, and mimo-v2.6-pro"
        ),
        "unexpected error: {stderr}"
    );

    // The gateway endpoint is fixed; only a loopback test base may replace it.
    let (success, _, stderr) =
        run_cli(workspace.path(), "https://gateway.example/api/v1", &[]).await?;
    assert!(!success, "a foreign endpoint was accepted");
    assert!(
        stderr.contains("the gateway endpoint is fixed"),
        "unexpected error: {stderr}"
    );

    // The passthrough model must be an OpenRouter slug.
    let (success, _, stderr) = run_cli(
        workspace.path(),
        &base,
        &["--provider.openrouter.model", "glm 5.3 flash"],
    )
    .await?;
    assert!(!success);
    assert!(
        stderr.contains("is not an OpenRouter model slug"),
        "unexpected error: {stderr}"
    );
    assert!(
        recorded.0.lock().unwrap().is_empty(),
        "a refused request reached the provider"
    );
    Ok(())
}

#[tokio::test]
async fn openrouter_passthrough_changes_only_the_wire_model() -> Result<()> {
    let recorded = Recorded::default();
    let base = serve(recorded.clone()).await?;
    let workspace = tempfile::tempdir()?;
    let (success, events, stderr) = run_cli(
        workspace.path(),
        &base,
        &["--provider.openrouter.model", "z-ai/glm-5.3-flash"],
    )
    .await?;
    assert!(success, "CLI failed: {stderr}");
    let requests = recorded.0.lock().unwrap().clone();
    assert!(
        requests
            .iter()
            .all(|(_, body)| body["model"] == "z-ai/glm-5.3-flash")
    );
    let completed = events
        .iter()
        .find(|event| event["type"] == "run.completed")
        .ok_or_else(|| eyre!("run.completed missing"))?;
    // The canonical model, its policy and identity are unchanged.
    assert_eq!(completed["payload"]["model"], "@cf/zai-org/glm-5.3");
    Ok(())
}

fn exec_alias(request: &Value) -> Result<String> {
    request["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|tool| {
            tool["function"]["description"]
                .as_str()
                .is_some_and(|text| text.starts_with("exec\n"))
        })
        .and_then(|tool| {
            assert_eq!(
                tool["function"]["parameters"]["properties"],
                json!({ "input": { "type": "string" } }),
                "exec must take one string input"
            );
            tool["function"]["name"].as_str().map(str::to_owned)
        })
        .ok_or_else(|| eyre!("exec was not offered as a function tool: {request}"))
}

async fn serve(recorded: Recorded) -> Result<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}/api/v1", listener.local_addr()?);
    let app = Router::new()
        .route("/api/v1/chat/completions", post(completions))
        .with_state(recorded);
    tokio::spawn(async move { axum::serve(listener, app).await });
    Ok(base)
}

async fn completions(
    State(recorded): State<Recorded>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let request: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let turn = {
        let mut requests = recorded.0.lock().unwrap();
        requests.push((authorization, request.clone()));
        requests.len()
    };
    let behaviour = recorded.1;
    if behaviour == Behaviour::Reject {
        let body = format!(
            "{{\"error\":{{\"message\":\"No auth credentials found for Bearer {SECRET}\",\"code\":401,\"metadata\":{{\"headers\":{{\"Authorization\":\"Bearer {SECRET}\"}},\"detail\":\"{}\"}}}}}}",
            "x".repeat(5000)
        );
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body))
            .unwrap();
    }
    if behaviour == Behaviour::Redirect {
        return Response::builder()
            .status(StatusCode::TEMPORARY_REDIRECT)
            .header(header::LOCATION, recorded.2.unwrap_or_default())
            .body(Body::empty())
            .unwrap();
    }
    if behaviour == Behaviour::StallFirst && turn == 1 {
        // One chunk, then silence with the connection held open.
        let first = format!(
            ": OPENROUTER PROCESSING\n\ndata: {}\n\n",
            json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "reasoning": "Thinking" } }] })
        );
        let stream = futures_util::stream::iter([Ok::<_, std::io::Error>(first)])
            .chain(futures_util::stream::pending());
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from_stream(stream))
            .unwrap();
    }
    if behaviour == Behaviour::BadAliasFirst && turn <= 2 {
        let (name, arguments) = if turn == 1 {
            ("tool_999".to_owned(), json!({ "input": CELL }).to_string())
        } else {
            (exec_alias(&request).unwrap_or_default(), CELL.to_owned())
        };
        return sse(vec![
            json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_exec", "type": "function", "function": { "name": name, "arguments": arguments } }] }, "finish_reason": "tool_calls" }] }),
        ]);
    }
    // A failed first attempt is replayed: count turns from the retry.
    let turn = if matches!(behaviour, Behaviour::StallFirst | Behaviour::BadAliasFirst) {
        turn - 1
    } else {
        turn
    };
    let cost = |index: usize| {
        (behaviour == Behaviour::Costed).then(|| {
            let (total, prompt) = TURN_COSTS[index];
            json!({
                "cost": total,
                "is_byok": false,
                "cost_details": {
                    "upstream_inference_cost": total,
                    "upstream_inference_prompt_cost": prompt,
                    "upstream_inference_completions_cost": total - prompt,
                },
            })
        })
    };
    let usage = |index: usize, mut usage: Value| {
        if let (Some(cost), Some(fields)) = (cost(index), usage.as_object_mut()) {
            fields.extend(cost.as_object().cloned().unwrap_or_default());
        }
        usage
    };
    let chunks = if turn == 1 {
        let exec = exec_alias(&request).unwrap_or_default();
        // Arguments arrive in fragments, as OpenRouter streams them.
        let arguments = json!({ "input": CELL }).to_string();
        let (head, tail) = arguments.split_at(arguments.len() / 2);
        vec![
            json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "reasoning": "Write the marker." } }] }),
            json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "id": "call_exec", "type": "function", "function": { "name": exec, "arguments": head } }] } }] }),
            json!({ "choices": [{ "index": 0, "delta": { "tool_calls": [{ "index": 0, "function": { "arguments": tail } }] }, "finish_reason": "tool_calls" }] }),
            json!({ "choices": [{ "index": 0, "delta": {}, "finish_reason": "tool_calls" }], "usage": usage(0, json!({ "prompt_tokens": 1200, "completion_tokens": 40 })) }),
        ]
    } else {
        vec![
            json!({ "choices": [{ "index": 0, "delta": { "role": "assistant", "content": "The marker " } }] }),
            json!({ "choices": [{ "index": 0, "delta": { "content": "is written." }, "finish_reason": "stop" }] }),
            json!({ "choices": [], "usage": usage(1, json!({ "prompt_tokens": 1400, "completion_tokens": 6, "prompt_tokens_details": { "cached_tokens": 1100 } })) }),
        ]
    };
    sse(chunks)
}

fn sse(chunks: Vec<Value>) -> Response {
    let mut stream = String::from(": OPENROUTER PROCESSING\n\n");
    for chunk in chunks {
        stream.push_str(&format!("data: {chunk}\n\n"));
    }
    stream.push_str("data: [DONE]\n\n");
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/event-stream")
        .body(Body::from(stream))
        .unwrap()
}

async fn run_cli(
    workspace: &Path,
    base: &str,
    extra: &[&str],
) -> Result<(bool, Vec<Value>, String)> {
    run_cli_with_key(workspace, base, Some("test-key"), extra).await
}

async fn run_cli_with_key(
    workspace: &Path,
    base: &str,
    key: Option<&str>,
    extra: &[&str],
) -> Result<(bool, Vec<Value>, String)> {
    let mut command = Command::new(local_cli::local_cli());
    command
        .current_dir(workspace)
        .env("HOME", workspace)
        .env("CODEX_HOME", workspace.join(".codex"))
        .env("NANOCODEX_COMPUTER", "off")
        .env_remove("OPENAI_API_KEY")
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("OPENAI_MODEL")
        .args([
            "run",
            "--browser=none",
            "--provider.openrouter",
            "--api-base-url",
            base,
        ])
        .arg("--cwd")
        .arg(workspace)
        .args([
            "--rollouts",
            "false",
            "--mcp-defaults",
            "false",
            "--web-search",
            "false",
        ])
        .args(["--image-generation", "false", "--subagents", "false"])
        .args(extra)
        .arg("write the marker")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(key) = key {
        command.env("OPENROUTER_API_KEY", key);
    }
    let output = timeout(Duration::from_secs(60), command.output())
        .await
        .map_err(|_| eyre!("CLI did not exit"))??;
    let events = String::from_utf8(output.stdout)?
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    Ok((
        output.status.success(),
        events,
        String::from_utf8_lossy(&output.stderr).into_owned(),
    ))
}
