//! Public journey for the runtime's default tools and one shared workspace.
//! Only the providers are local HTTP fixtures; the runtime, recipes, the
//! standard Codex tools, the canonical Claude file tools and child routing
//! run normally against a temporary workspace.
use axum::{Json, Router, routing::post};
use nanocode::{
    ClaudeHarness, CodexHarness, HarnessFamily, Nanocodex, NanocodexError, OpenAi,
    agent::SpawnOptions, claude::ClaudeClient, oai::transport::ResponsesTransport, tools::Tools,
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

const DEADLINE: Duration = Duration::from_secs(60);
const CLAUDE_FILE_TOOLS: [&str; 6] = ["Read", "Edit", "Write", "Glob", "Grep", "Bash"];
type Transcript = Arc<Mutex<Vec<Value>>>;

/// The file named after a marker in the latest prompt.
fn marked(text: &str, marker: &str) -> Option<String> {
    let start = text.find(marker)? + marker.len();
    let name: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
        .collect();
    (!name.is_empty()).then_some(name)
}

fn tool_names(tools: &Value) -> Vec<String> {
    tools.as_array().map_or_else(Vec::new, |tools| {
        tools
            .iter()
            .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
            .collect()
    })
}

/// Codex reads the marked file with `exec_command` and edits it with
/// `apply_patch`, both from one Code Mode cell.
fn responses(body: &Value, ordinal: usize) -> String {
    let input = body["input"].as_array().unwrap();
    let continuation = input
        .last()
        .is_some_and(|item| item["type"] == "custom_tool_call_output");
    let prompt = input
        .iter()
        .rev()
        .find(|item| item["role"] == "user")
        .map(Value::to_string)
        .unwrap_or_default();
    let output = match marked(&prompt, "[codex-files] ") {
        Some(name) if !continuation => {
            let code = format!(
                "const read = await tools.exec_command({{cmd: \"cat {name}\"}}); \
                 await tools.apply_patch(\"*** Begin Patch\\n*** Update File: {name}\\n@@\\n-alpha\\n+codex-edited\\n*** End Patch\"); \
                 text(\"read:\" + read.output);"
            );
            json!([{"type":"custom_tool_call","call_id":format!("codex-call-{ordinal}"),"name":"exec","input":code}])
        }
        _ => {
            json!([{"type":"message","role":"assistant","content":[{"type":"output_text","text":"codex files done"}]}])
        }
    };
    let frame = json!({"type":"response.completed","response":{"id":format!("response-{ordinal}"),"status":"completed","output":output}});
    format!("data: {frame}\n\ndata: [DONE]\n\n")
}

/// Claude reads the marked file with `Read`, then edits it with `Edit`: from
/// one Code Mode cell when `exec` is declared, otherwise as direct calls.
fn messages(body: &Value, ordinal: usize) -> String {
    let code_mode = tool_names(&body["tools"]).contains(&"exec".to_owned());
    let history = body["messages"].as_array().unwrap();
    let prompt = history
        .iter()
        .rposition(|message| {
            message["role"] == "user" && message.to_string().contains("[claude-files] ")
        })
        .map(|index| {
            (
                index,
                marked(&history[index].to_string(), "[claude-files] "),
            )
        });
    let call = match prompt {
        Some((index, Some(name))) => match history.len() - index - 1 {
            0 if code_mode => Some((
                "exec",
                json!({"code": format!(
                    "const read = await tools.Read({{file_path: \"{name}\"}}); \
                     await tools.Edit({{file_path: \"{name}\", old_string: \"alpha\", new_string: \"claude-edited\"}}); \
                     const bash = await tools.Bash({{command: \"printf bash-ok\"}}); text(JSON.stringify(bash)); \
                     text(\"read:\" + JSON.stringify(read));"
                )}),
            )),
            0 => Some(("Read", json!({"file_path": name}))),
            2 if !code_mode => Some((
                "Edit",
                json!({"file_path": name, "old_string": "alpha", "new_string": "claude-edited"}),
            )),
            _ => None,
        },
        _ => None,
    };
    let block = match &call {
        None => json!({"type":"text","text":"claude files done"}),
        Some((name, _)) => {
            json!({"type":"tool_use","id":format!("claude-call-{ordinal}"),"name":name,"input":{}})
        }
    };
    let mut frames = vec![
        json!({"type":"message_start","message":{"id":format!("message-{ordinal}"),"role":"assistant","model":body["model"],"content":[],"usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":block}),
    ];
    if let Some((_, input)) = &call {
        frames.push(json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":input.to_string()}}));
    }
    frames.extend([
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":if call.is_some() {"tool_use"} else {"end_turn"}},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ]);
    frames
        .into_iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect()
}

async fn serve(transcript: Transcript, trace: PathBuf) -> std::net::SocketAddr {
    let record = move |transport: &'static str, body: &Value| {
        let mut transcript = transcript.lock().unwrap();
        transcript.push(json!({"transport": transport, "request": body}));
        std::fs::write(&trace, serde_json::to_vec_pretty(&*transcript).unwrap()).unwrap();
        transcript.len()
    };
    let codex = record.clone();
    let app = Router::new()
        .route(
            "/responses",
            post(move |Json(body): Json<Value>| {
                let ordinal = codex("responses", &body);
                async move {
                    (
                        [("content-type", "text/event-stream")],
                        responses(&body, ordinal),
                    )
                }
            }),
        )
        .route(
            "/v1/messages",
            post(move |Json(body): Json<Value>| {
                let ordinal = record("messages", &body);
                async move {
                    (
                        [("content-type", "text/event-stream")],
                        messages(&body, ordinal),
                    )
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    address
}

/// Every provider request that carried this agent's marked file.
fn requests(transcript: &Transcript, transport: &str, name: &str) -> Vec<Value> {
    transcript
        .lock()
        .unwrap()
        .iter()
        .filter(|frame| frame["transport"] == transport)
        .map(|frame| frame["request"].clone())
        .filter(|request| request.to_string().contains(name))
        .collect()
}

/// Runs one prompt that reads and edits `name` and checks the
/// workspace file, the observed read and the declared catalog.
async fn edits(agent: &nanocode::Agent, transcript: &Transcript, workspace: &Path, name: &str) {
    let family = agent.harness_family();
    let (marker, transport, edited) = match family {
        HarnessFamily::Codex => ("[codex-files]", "responses", "codex-edited\n"),
        HarnessFamily::Claude => ("[claude-files]", "messages", "claude-edited\n"),
    };
    std::fs::write(workspace.join(name), "alpha\n").unwrap();
    agent
        .prompt(format!("{marker} {name}"))
        .await
        .unwrap()
        .result()
        .await
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(workspace.join(name)).unwrap(),
        edited,
        "{family} edited {name} in the runtime workspace"
    );
    let requests = requests(transcript, transport, name);
    let observed = requests.last().unwrap().to_string();
    match family {
        HarnessFamily::Codex => {
            assert!(observed.contains("read:alpha"), "{observed}");
            // Responses declares the Code Mode tools as additional tools.
            let declared = requests[0]["input"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|item| item["type"] == "additional_tools")
                .flat_map(|item| tool_names(&item["tools"]))
                .collect::<Vec<_>>();
            assert_eq!(
                declared,
                ["exec", "wait"],
                "standard Codex tools stay behind Code Mode"
            );
        }
        HarnessFamily::Claude if tool_names(&requests[0]["tools"]) == ["exec", "wait"] => {
            // Code Mode declares only exec/wait and nests the file tools.
            assert!(
                observed.contains("read:") && observed.contains("alpha"),
                "{observed}"
            );
            let results = requests[1]["messages"]
                .as_array()
                .unwrap()
                .iter()
                .filter_map(|message| message["content"].as_array())
                .flatten()
                .filter(|block| block["type"] == "tool_result")
                .map(Value::to_string)
                .collect::<String>();
            assert!(results.contains("bash-ok"), "Bash must execute: {results}");
            let exec = requests[0]["tools"][0]["description"].to_string();
            for name in CLAUDE_FILE_TOOLS {
                assert!(exec.contains(name), "exec describes nested {name}: {exec}");
            }
        }
        HarnessFamily::Claude => {
            // The direct Read result preceded the Edit.
            assert!(requests[2].to_string().contains("alpha"), "{observed}");
            for request in &requests {
                assert_eq!(tool_names(&request["tools"]), CLAUDE_FILE_TOOLS);
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unconfigured_runtime_tools_edit_one_shared_workspace_across_families() {
    tokio::time::timeout(DEADLINE, journey())
        .await
        .expect("default tools journey deadline");
}

async fn journey() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let evidence = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../output/library-runtime");
    std::fs::create_dir_all(&evidence).unwrap();
    let trace = evidence.join(format!("default-tools-{}.json", std::process::id()));
    println!("TRANSCRIPT {}", trace.display());
    let workspace = std::env::temp_dir().join(format!(
        "nanocode-default-tools-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace).unwrap();
    let transcript: Transcript = Arc::default();
    let address = serve(Arc::clone(&transcript), trace).await;
    let openai = OpenAi::builder("synthetic-key")
        .transport(ResponsesTransport::Https)
        .store(false)
        .api_base_url(format!("http://{address}"))
        .build()
        .unwrap();
    let claude = ClaudeClient::new(
        reqwest::Client::new(),
        format!("http://{address}/v1/messages"),
        "synthetic-key",
    );

    // No tools configured: both families get their canonical coding tools,
    // rooted at the one runtime workspace, behind Code Mode by default.
    let nano = Nanocodex::builder()
        .openai(openai.clone())
        .claude(claude.clone())
        .workspace(&workspace)
        .build()
        .unwrap();
    let (codex_root, _events) = nano.spawn().build().await.unwrap();
    let (claude_root, _events) = nano
        .spawn()
        .harness(HarnessFamily::Claude)
        .build()
        .await
        .unwrap();
    edits(&codex_root, &transcript, &workspace, "codex-root.txt").await;
    edits(&claude_root, &transcript, &workspace, "claude-root.txt").await;
    println!(
        "ROOTS Codex exec_command+apply_patch and Claude Code Mode Read+Edit edited files in {}",
        workspace.display()
    );

    // Children routed across families and a same-family native child bind
    // the same workspace and default catalogs.
    let (claude_child, _events) = codex_root
        .spawn_with(SpawnOptions::new().harness(HarnessFamily::Claude))
        .await
        .unwrap();
    assert_eq!(claude_child.harness_family(), HarnessFamily::Claude);
    edits(&claude_child, &transcript, &workspace, "claude-child.txt").await;
    let (codex_child, _events) = claude_root
        .spawn_with(SpawnOptions::new().harness(HarnessFamily::Codex))
        .await
        .unwrap();
    assert_eq!(codex_child.harness_family(), HarnessFamily::Codex);
    edits(&codex_child, &transcript, &workspace, "codex-child.txt").await;
    let (native_child, _events) = claude_root.spawn().await.unwrap();
    assert_eq!(native_child.harness_family(), HarnessFamily::Claude);
    edits(
        &native_child,
        &transcript,
        &workspace,
        "claude-native-child.txt",
    )
    .await;
    println!(
        "CHILDREN Codex->Claude, Claude->Codex and Claude->Claude children edited the same workspace"
    );
    nano.shutdown().await.unwrap();

    // Code Mode preserves canonical Claude file tools. A
    // configure hook cannot move an agent out of the runtime workspace.
    let configured = Nanocodex::builder()
        .openai(CodexHarness::new(openai).configure(|builder| {
            Ok(builder.workspace(std::env::temp_dir().join("nanocode-elsewhere")))
        }))
        .claude(
            ClaudeHarness::new(claude.clone())
                .configure(|builder| Ok(builder.workspace("/nanocode-elsewhere"))),
        )
        .workspace(&workspace)
        .build()
        .unwrap();
    let (codex_root, _events) = configured.spawn().build().await.unwrap();
    edits(&codex_root, &transcript, &workspace, "codex-configured.txt").await;
    let (claude_root, _events) = configured
        .spawn()
        .harness(HarnessFamily::Claude)
        .build()
        .await
        .unwrap();
    edits(
        &claude_root,
        &transcript,
        &workspace,
        "claude-configured-root.txt",
    )
    .await;
    let (native_child, _events) = claude_root.spawn().await.unwrap();
    edits(
        &native_child,
        &transcript,
        &workspace,
        "claude-configured-child.txt",
    )
    .await;
    configured.shutdown().await.unwrap();
    println!(
        "CODE_MODE Claude root and native child called Read+Edit through exec; a configure hook kept the runtime workspace"
    );

    // An explicit empty selection still replaces the default catalog.
    let empty = Nanocodex::builder()
        .claude(
            ClaudeHarness::new(claude.clone())
                .tools(Tools::builder().without_defaults().build().unwrap()),
        )
        .workspace(&workspace)
        .build()
        .unwrap();
    let (bare, _events) = empty.spawn().build().await.unwrap();
    std::fs::write(workspace.join("untouched.txt"), "alpha\n").unwrap();
    bare.prompt("[claude-files] untouched.txt")
        .await
        .unwrap()
        .result()
        .await
        .unwrap();
    let request = requests(&transcript, "messages", "untouched.txt").remove(0);
    assert_eq!(tool_names(&request["tools"]), ["exec", "wait"], "{request}");
    empty.shutdown().await.unwrap();
    println!("EXPLICIT_EMPTY an empty Claude selection declares no file tools");

    // A workspace that is not a directory fails before any provider request.
    let before = transcript.lock().unwrap().len();
    let missing = Nanocodex::builder()
        .claude(claude)
        .workspace(workspace.join("missing"))
        .build()
        .unwrap();
    let error = missing
        .spawn()
        .build()
        .await
        .err()
        .expect("missing workspace");
    assert!(
        matches!(&error, NanocodexError::InvalidRequest(_)),
        "{error}"
    );
    assert_eq!(before, transcript.lock().unwrap().len());
    println!("MISSING_WORKSPACE rejected before any provider request: {error}");
    std::fs::remove_dir_all(&workspace).unwrap();
}
