use super::{
    AgentEvents, DurableAgentExt, HashMap, JavaScriptDurabilityStore, JavaScriptSpawnRouter,
    JsFuture, JsValue, Mutex, RustNanocodex, SessionCheckpoint, WasmHarnessFactory, WasmNanocodex,
    WasmSubagents, WasmSubagentsConfig, host_cancel_code_turn, js_agent_error, js_error,
};
use nanocode_backend_claude::{
    Claude, ClaudeAuthFuture, ClaudeAuthProvider, ClaudeAuthUnavailable, ClaudeClient,
    ClaudeNestedToolUpdate, ClaudeToolInvocation, ClaudeToolProgress, ClaudeToolReply, ClaudeTools,
    ServerToolDefinition, ToolDefinition, ToolResultContent,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_namespace = ["globalThis", "nanocodexHost"], js_name = claudeAuth)]
    fn host_claude_auth(auth_host_id: u32) -> Result<js_sys::Promise, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["globalThis", "nanocodexHost"], js_name = executeClaudeTool)]
    fn host_execute_claude_tool(
        host_definition_id: u32,
        name: &str,
        input: &str,
        session_id: &str,
        call_id: &str,
        model: &str,
        turn_id: &str,
        local_definitions: &str,
        execute_local_tool: &JsValue,
    ) -> Result<js_sys::Promise, JsValue>;

    #[wasm_bindgen(catch, js_namespace = ["globalThis", "nanocodexHost"], js_name = nextClaudeCodeUpdate)]
    fn host_next_claude_code_update(
        host_definition_id: u32,
        session_id: &str,
        call_id: &str,
    ) -> Result<js_sys::Promise, JsValue>;
}

async fn observe_live_updates(
    host_definition_id: u32,
    session_id: &str,
    call_id: &str,
    progress: &ClaudeToolProgress,
) {
    #[derive(Deserialize)]
    #[serde(tag = "type", rename_all = "snake_case")]
    enum Update {
        NestedCallStarted {
            call_id: String,
            name: String,
            input: Value,
        },
        NestedCallCompleted {
            call: Value,
        },
        #[serde(other)]
        Other,
    }
    loop {
        let Ok(next) = host_next_claude_code_update(host_definition_id, session_id, call_id) else {
            return;
        };
        let Ok(value) = JsFuture::from(next).await else {
            return;
        };
        let Some(encoded) = value.as_string() else {
            return;
        };
        match serde_json::from_str::<Update>(&encoded) {
            Ok(Update::NestedCallStarted {
                call_id,
                name,
                input,
            }) => progress.update(ClaudeNestedToolUpdate::Started {
                call_id,
                name,
                input,
            }),
            Ok(Update::NestedCallCompleted { call }) => {
                progress.update(ClaudeNestedToolUpdate::Completed(call));
            }
            Ok(Update::Other) | Err(_) => {}
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct ClaudeConfig {
    model: String,
    tool_mode: Option<String>,
    session_id: Option<String>,
    api_key: Option<String>,
    auth_host_id: Option<u32>,
    endpoint: Option<String>,
    #[serde(default)]
    subscription_compatibility: bool,
    subscription_identity: Option<nanocode_backend_claude::SubscriptionIdentity>,
    host_definition_id: Option<u32>,
    #[serde(default)]
    tools: Vec<ToolDefinition>,
    #[serde(default)]
    server_tools: Vec<ServerToolDefinition>,
    max_tokens: Option<u32>,
    thinking: Option<super::Thinking>,
    #[serde(default)]
    adaptive_thinking: bool,
    #[serde(default)]
    keep_thinking: bool,

    fast_mode: Option<bool>,
    #[serde(default)]
    cache: CachePolicy,
    auto_compact: Option<bool>,
    auto_compact_window_tokens: Option<u64>,
    context_window_tokens: Option<u64>,
    instructions: Option<String>,
    system_blocks: Option<Vec<Value>>,
    workspace: Option<String>,
    #[serde(default)]
    parallel_tools: bool,

    #[serde(default)]
    parallel_safe_tools: Vec<String>,
    #[serde(default)]
    client_tool_search: bool,
    durability_host_id: Option<String>,
    durability_id: Option<String>,
    terminal_receipt_retention: Option<usize>,
    resume: Option<SessionCheckpoint>,
    subagents: Option<WasmSubagentsConfig>,
    #[serde(default)]
    subagent_routing: bool,
    codex_harness: Option<Value>,
    #[serde(default)]
    inherit: Option<super::WasmInherit>,
}

#[derive(Default, Deserialize, Serialize)]
enum CachePolicy {
    #[serde(rename = "off")]
    Off,
    #[default]
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "1h")]
    OneHour,
}

impl ClaudeConfig {
    fn is_direct(&self) -> bool {
        self.tool_mode.as_deref() == Some("direct")
    }

    fn validate(&self) -> Result<(), &'static str> {
        if !matches!(
            self.tool_mode.as_deref(),
            None | Some("code-only" | "direct")
        ) {
            return Err("unsupported Claude toolMode");
        }
        if !self.server_tools.is_empty() || self.client_tool_search {
            return Err("Claude does not expose provider or client search tools directly");
        }
        if !self.is_direct()
            && (self.tools.len() != 2
                || !self.tools.iter().any(|tool| tool.name == "exec")
                || !self.tools.iter().any(|tool| tool.name == "wait"))
        {
            return Err("Claude Code Mode must expose only exec and wait");
        }
        if self.model.trim().is_empty() {
            return Err("Claude model must not be empty");
        }
        if self
            .session_id
            .as_ref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err("sessionId must not be empty");
        }
        if let (Some(session_id), Some(durability_id)) = (&self.session_id, &self.durability_id)
            && session_id != durability_id
        {
            return Err("durable Claude sessionId must equal durabilityId");
        }
        match (&self.api_key, self.auth_host_id) {
            (Some(key), None) if !key.trim().is_empty() => {}
            (None, Some(_)) => {}
            _ => return Err("supply exactly one nonempty apiKey or authHostId"),
        }
        if !self.tools.is_empty() && self.host_definition_id.is_none() {
            return Err("explicit Claude tools require hostDefinitionId");
        }
        if self.instructions.is_some() && self.system_blocks.is_some() {
            return Err("instructions and systemBlocks are mutually exclusive");
        }
        if self.auto_compact == Some(false) {
            return Err("disabling Claude automatic compaction is unsupported by this backend");
        }
        if self.max_tokens == Some(0)
            || self.context_window_tokens == Some(0)
            || self.auto_compact_window_tokens == Some(0)
        {
            return Err("Claude token limits must be positive");
        }
        match (&self.durability_host_id, &self.durability_id) {
            (None, None) => {
                if self.terminal_receipt_retention.is_some() {
                    return Err("terminalReceiptRetention requires durability");
                }
            }
            (Some(host), Some(id)) if !host.trim().is_empty() && !id.trim().is_empty() => {}
            _ => {
                return Err(
                    "durabilityHostId and durabilityId must be nonempty and supplied together",
                );
            }
        }
        if self
            .terminal_receipt_retention
            .is_some_and(|limit| limit > 4_096)
        {
            return Err("terminalReceiptRetention must be from 0 through 4096");
        }
        if self.endpoint.as_ref().is_some_and(|endpoint| {
            reqwest::Url::parse(endpoint).map_or(true, |url| {
                !matches!(url.scheme(), "http" | "https")
                    || url.host_str().is_none()
                    || !url.username().is_empty()
                    || url.password().is_some()
                    || url.fragment().is_some()
            })
        }) {
            return Err(
                "endpoint must be an explicit HTTP(S) Messages URL without userinfo or fragment",
            );
        }
        Ok(())
    }
}

struct JavaScriptClaudeAuth {
    auth_host_id: u32,
}

impl ClaudeAuthProvider for JavaScriptClaudeAuth {
    fn headers(
        &self,
    ) -> ClaudeAuthFuture<'_, Result<reqwest::header::HeaderMap, ClaudeAuthUnavailable>> {
        Box::pin(async move {
            let promise = host_claude_auth(self.auth_host_id).map_err(|_| {
                ClaudeAuthUnavailable::new("the JavaScript Claude auth callback could not be called")
            })?;
            let result = JsFuture::from(promise).await.map_err(|error| {
                ClaudeAuthUnavailable::new(match error.as_string() {
                    Some(reason) => format!("the JavaScript Claude auth callback rejected: {reason}"),
                    None => "the JavaScript Claude auth callback rejected".to_owned(),
                })
            })?;
            let encoded = result.as_string().ok_or_else(|| {
                ClaudeAuthUnavailable::new("the JavaScript Claude auth callback returned a non-string value")
            })?;
            let headers: BTreeMap<String, String> =
                serde_json::from_str(&encoded).map_err(|_| {
                    ClaudeAuthUnavailable::new("the JavaScript Claude auth headers are not a JSON string map")
                })?;
            if headers.is_empty() {
                return Err(ClaudeAuthUnavailable::new(
                    "the JavaScript Claude auth callback returned no headers",
                ));
            }
            let mut output = reqwest::header::HeaderMap::new();
            for (name, value) in headers {
                let name = reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| {
                    ClaudeAuthUnavailable::new(format!("invalid Claude auth header name {name:?}"))
                })?;
                let mut value = reqwest::header::HeaderValue::from_str(&value).map_err(|_| {
                    ClaudeAuthUnavailable::new(format!("invalid value for Claude auth header {name}"))
                })?;
                value.set_sensitive(true);
                output.insert(name, value);
            }
            Ok(output)
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct HostToolReply {
    content: ToolResultContent,
    #[serde(default)]
    is_error: bool,
    metadata: Option<Value>,
    structured_result: Option<Value>,
}

async fn execute_tool(
    host_definition_id: u32,
    name: &str,
    input: Value,
    invocation: ClaudeToolInvocation,
    local_tools: Option<ClaudeTools>,
) -> Result<ClaudeToolReply, String> {
    struct PendingTool<'a> {
        session_id: &'a str,
        cancel: Option<js_sys::Function>,
        settled: bool,
    }
    impl Drop for PendingTool<'_> {
        fn drop(&mut self) {
            if !self.settled {
                if let Some(cancel) = &self.cancel {
                    let _ = cancel.call0(&JsValue::UNDEFINED);
                } else {
                    host_cancel_code_turn(self.session_id);
                }
            }
        }
    }
    let local_tools = local_tools.unwrap_or_default();
    let definitions = serde_json::to_string(
        &local_tools
            .definitions()
            .iter()
            .map(|definition| {
                serde_json::json!({ "type": "function", "name": definition.name,
            "description": definition.description, "parameters": definition.input_schema })
            })
            .collect::<Vec<_>>(),
    )
    .map_err(|error| error.to_string())?;
    let original = invocation.clone();

    let callback = Closure::wrap(
        Box::new(move |name: String, input: String, call_id: String| {
            let tools = local_tools.clone();
            let mut invocation = original.clone();
            invocation.call_id = call_id;
            invocation.progress = None;
            let (abort, registration) = futures_util::future::AbortHandle::new_pair();
            let future = futures_util::future::Abortable::new(
                async move {
                    let input = serde_json::from_str(&input)
                        .map_err(|_| js_error("Claude nested tool input is invalid"))?;
                    let reply = tools
                        .execute(&name, input, invocation)
                        .await
                        .map_err(js_error)?;
                    let output = match reply.content {
                        ToolResultContent::Text(text) => text,
                        ToolResultContent::Blocks(blocks) => {
                            serde_json::to_string(&blocks).map_err(js_error)?
                        }
                    };
                    Ok(JsValue::from_str(
                        &serde_json::json!({ "output": output, "success": !reply.is_error,
                "structured_result": reply.structured_result, "metadata": reply.metadata })
                        .to_string(),
                    ))
                },
                registration,
            );
            let promise = wasm_bindgen_futures::future_to_promise(async move {
                future
                    .await
                    .map_err(|_| js_error("Claude nested tool execution cancelled"))?
            });
            let cancel =
                Closure::wrap(Box::new(move || abort.abort()) as Box<dyn FnMut()>).into_js_value();
            let _ = js_sys::Reflect::set(promise.as_ref(), &JsValue::from_str("cancel"), &cancel);
            promise
        }) as Box<dyn FnMut(String, String, String) -> js_sys::Promise>,
    )
    .into_js_value();
    let promise = host_execute_claude_tool(
        host_definition_id,
        name,
        &input.to_string(),
        &invocation.session_id,
        &invocation.call_id,
        &invocation.model,
        &invocation.turn_id,
        &definitions,
        &callback,
    )
    .map_err(|_| "Claude tool host rejected invocation".to_owned())?;
    let mut pending = PendingTool {
        session_id: &invocation.session_id,
        cancel: js_sys::Reflect::get(promise.as_ref(), &JsValue::from_str("cancel"))
            .ok()
            .and_then(|value| value.dyn_into().ok()),
        settled: false,
    };
    if let Some(progress) = &invocation.progress {
        observe_live_updates(
            host_definition_id,
            &invocation.session_id,
            &invocation.call_id,
            progress,
        )
        .await;
    }
    let response = JsFuture::from(promise).await;
    pending.settled = true;
    let response = response.map_err(|error| {
        if js_sys::Reflect::get(&error, &JsValue::from_str("code"))
            .ok()
            .and_then(|value| value.as_string())
            .as_deref()
            == Some("host_interrupted")
        {
            ClaudeTools::HOST_INTERRUPTED.to_owned()
        } else {
            "Claude tool host invocation failed".to_owned()
        }
    })?;
    let response = response
        .as_string()
        .ok_or_else(|| "Claude tool host must return a JSON string".to_owned())?;
    let reply: HostToolReply = serde_json::from_str(&response)
        .map_err(|_| "Claude tool host returned an invalid Claude reply".to_owned())?;
    Ok(ClaudeToolReply {
        content: reply.content,
        is_error: reply.is_error,
        metadata: reply.metadata,
        structured_result: reply.structured_result,
    })
}

#[wasm_bindgen(js_class = Nanocodex)]
impl WasmNanocodex {
    #[wasm_bindgen(js_name = createClaude)]
    pub async fn create_claude(config_json: &str) -> Result<Self, JsValue> {
        Self::create_claude_with_codex_auth(config_json, None).await
    }

    #[wasm_bindgen(js_name = createClaudeWithChatGpt)]
    pub async fn create_claude_with_chat_gpt(
        config_json: &str,
        subscription: &super::WasmChatGptSubscription,
    ) -> Result<Self, JsValue> {
        let auth = subscription.inner.authorization().await.map_err(js_error)?;
        Self::create_claude_with_codex_auth(config_json, Some(auth)).await
    }

    async fn create_claude_with_codex_auth(
        config_json: &str,
        codex_auth: Option<nanocode::oai::auth::OpenAiAuth>,
    ) -> Result<Self, JsValue> {
        let config: ClaudeConfig = serde_json::from_str(config_json)
            .map_err(|_| js_error("invalid Claude configuration"))?;
        let (factory, subagents) = if let Some(settings) = &config.subagents {
            let host = config
                .host_definition_id
                .ok_or_else(|| js_error("subagents require hostDefinitionId"))?;
            let (registry, control, updates) =
                nanocode_subagents::channel(settings.max_concurrency);
            if config.subagent_routing {
                registry.set_spawn_router(Arc::new(JavaScriptSpawnRouter {
                    host_definition_id: host,
                }));
            }
            let parents = Arc::new(Mutex::new(HashMap::new()));
            let codex = config
                .codex_harness
                .clone()
                .map(|recipe| {
                    let auth = if let Some(auth) = codex_auth.clone() {
                        auth
                    } else {
                        let key = recipe
                            .get("api_key")
                            .and_then(Value::as_str)
                            .ok_or_else(|| js_error("Codex harness requires explicit transport"))?;
                        nanocode::oai::auth::OpenAiAuth::api_key(key.to_owned())
                    };
                    Ok::<_, JsValue>((recipe, auth))
                })
                .transpose()?;
            let factory = Arc::new(WasmHarnessFactory {
                registry: registry.clone(),
                parents: parents.clone(),
                hosts: Arc::new(Mutex::new(HashMap::new())),
                codex,
                claude: Some(serde_json::to_value(&config).map_err(js_error)?),
                durability_host_id: (config.session_id == config.durability_id)
                    .then(|| config.durability_host_id.clone())
                    .flatten(),
            });
            let subagents = WasmSubagents::new(
                host,
                registry,
                control,
                updates,
                parents,
                factory.hosts.clone(),
                settings.continuation != Some(false),
            );
            (Some(factory), Some(subagents))
        } else {
            (None, None)
        };
        let (inner, events) = build_claude(config, factory, None, None, None).await?;
        Ok(Self::from_parts(inner, events, subagents))
    }
}

pub(super) async fn build_claude(
    config: ClaudeConfig,
    factory: Option<Arc<WasmHarnessFactory>>,
    mut checkpoint: Option<SessionCheckpoint>,
    host_context: Option<Arc<str>>,
    lineage: Option<nanocode::Lineage>,
) -> Result<(RustNanocodex, AgentEvents), JsValue> {
    config.validate().map_err(js_error)?;
    let inherit = config.inherit.clone();
    let record = super::durability_record(
        config.durability_id.as_deref(),
        config.session_id.as_deref(),
        nanocode::ModelSelection::new(nanocode::HarnessFamily::Claude, config.model.clone()),
        config.workspace.as_deref(),
        checkpoint.as_ref().or(config.resume.as_ref()),
        lineage.or_else(|| inherit.as_ref().map(super::WasmInherit::lineage)),
    )
    .map(|record| match &inherit {
        Some(inherit) => inherit.record(record),
        None => record,
    });
    let mut seed = inherit
        .as_ref()
        .map(super::WasmInherit::transcript)
        .unwrap_or_default();
    let direct = config.is_direct();
    let endpoint = config.endpoint.unwrap_or_else(|| {
        if config.subscription_compatibility {
            nanocode_backend_claude::ANTHROPIC_SUBSCRIPTION_MESSAGES_URL.to_owned()
        } else {
            nanocode_backend_claude::ANTHROPIC_MESSAGES_URL.to_owned()
        }
    });
    let http = reqwest::Client::new();
    let mut client = match (config.api_key, config.auth_host_id) {
        (Some(key), None) => ClaudeClient::new(http, endpoint, key),
        (None, Some(auth_host_id)) => ClaudeClient::with_auth_provider(
            http,
            endpoint,
            Arc::new(JavaScriptClaudeAuth { auth_host_id }),
        ),
        _ => return Err(js_error("invalid explicit Claude authentication")),
    };
    if config.subscription_compatibility {
        client = client.subscription_compatibility();
        if let Some(identity) = config.subscription_identity {
            identity
                .validate()
                .map_err(|_| js_error("invalid subscription identity"))?;
            client = client.with_subscription_identity(identity);
        }
    } else if config.subscription_identity.is_some() {
        return Err(js_error(
            "subscription identity requires subscription compatibility",
        ));
    }
    let builder_model = config.model.clone();
    let mut builder = RustNanocodex::builder(Claude::new(client, config.model))
        .parallel_tools(config.parallel_tools)
        .parallel_safe_tools(config.parallel_safe_tools);

    if let Some(resume) = config.resume {
        builder = builder.resume(resume).map_err(js_agent_error)?;
    }
    if let Some(session_id) = config.session_id {
        builder = builder.session_id(session_id);
    }
    if let Some(tokens) = config.max_tokens {
        builder = builder.max_tokens(tokens);
    }
    if let Some(effort) = config.thinking {
        if builder_model.parse::<nanocode::ModelSelection>().is_ok() {
            builder = builder.thinking(effort).map_err(js_error)?;
        } else if effort != super::Thinking::None {
            let native = serde_json::from_value(serde_json::to_value(effort).map_err(js_error)?)
                .map_err(js_error)?;
            builder = builder.effort(native);
        }
    }
    if config.adaptive_thinking {
        builder = builder.adaptive_thinking();
    }
    if config.keep_thinking {
        builder = builder.keep_thinking();
    }
    if let Some(enabled) = config.fast_mode {
        builder = builder.fast_mode(enabled);
    }
    builder = match config.cache {
        CachePolicy::Off => builder.automatic_cache(false),
        CachePolicy::FiveMinutes => builder.automatic_cache(true),
        CachePolicy::OneHour => builder.cache_one_hour(),
    };
    if let Some(tokens) = config.context_window_tokens {
        builder = builder.context_window_tokens(tokens);
    }
    if let Some(tokens) = config.auto_compact_window_tokens {
        builder = builder.auto_compact_window_tokens(tokens);
    }
    if let Some(instructions) = config.instructions {
        builder = builder.system(instructions);
    }
    if let Some(blocks) = config.system_blocks {
        builder = builder.system_blocks(blocks);
    }
    if let Some(workspace) = config.workspace {
        builder = builder.workspace(workspace);
    }

    builder = builder.code_only(!direct);
    let code_definitions = if factory.is_some() && !direct {
        config.tools.clone()
    } else {
        Vec::new()
    };
    for definition in config
        .tools
        .into_iter()
        .filter(|_| direct || factory.is_none())
    {
        let host_id = config
            .host_definition_id
            .ok_or_else(|| js_error("explicit Claude tools require hostDefinitionId"))?;
        let name = definition.name.clone();
        builder = builder.tool_with_context(definition, move |input, invocation| {
            let name = name.clone();
            async move { execute_tool(host_id, &name, input, invocation, None).await }
        });
    }
    if let (Some(route_id), Some(state_id)) = (config.durability_host_id, config.durability_id) {
        let store = JavaScriptDurabilityStore { route_id };
        let durable = if let Some(limit) = config.terminal_receipt_retention {
            nanocode::agent::durability::DurableSession::open_with_terminal_receipt_limit(
                store, state_id, limit,
            )
            .await
        } else {
            nanocode::agent::durability::DurableSession::open(store, state_id).await
        }
        .map_err(js_error)?;
        if durable
            .latest_checkpoint()
            .await
            .map_err(js_error)?
            .is_some()
        {
            checkpoint = None;
        }
        if let Some(checkpoint) = checkpoint.take() {
            builder = builder.resume(checkpoint).map_err(js_agent_error)?;
        }
        if let Some(record) = record {
            let described = durable.describe(record).await.map_err(js_error)?;
            seed = super::inherited_seed(&durable, described).await?;
        }
        builder = builder.durability(durable).await.map_err(js_error)?;
    }
    if factory.is_none() {
        builder = builder.tools_factory(|agent| {
            let preempt_session = agent.session_id().to_owned();
            Ok(ClaudeTools::new().code_turn_preempt(move || {
                super::host_preempt_code_turn(&preempt_session);
                async {}
            }))
        });
    }
    if let Some(factory) = factory {
        let host = config
            .host_definition_id
            .ok_or_else(|| js_error("subagents require hostDefinitionId"))?;
        let registry = factory.registry.clone();
        let parents = factory.parents.clone();
        builder = builder
            .spawn_factory(factory.clone())
            .tools_factory(move |agent| {
                let agent = agent.with_spawn_factory(factory.clone());
                factory
                    .hosts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(agent.session_id().to_owned(), host);
                parents
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(agent.session_id().to_owned(), agent.clone());
                let preempt_session = agent.session_id().to_owned();
                let code_session = preempt_session.clone();
                let native = nanocode_subagents::install_claude_tools(
                    ClaudeTools::new(),
                    agent,
                    registry.clone(),
                )?;
                let native = native.code_turn_preempt(move || {
                    super::host_preempt_code_turn(&preempt_session);
                    async {}
                });
                if direct {
                    return Ok(native);
                }
                let mut tools = ClaudeTools::new().code_turn_preempt(move || {
                    super::host_preempt_code_turn(&code_session);
                    async {}
                });
                for definition in &code_definitions {
                    let name = definition.name.clone();
                    let native = native.clone();
                    tools =
                        tools.tool_with_context(definition.clone(), move |input, invocation| {
                            let name = name.clone();
                            let native = native.clone();
                            async move {
                                execute_tool(host, &name, input, invocation, Some(native)).await
                            }
                        });
                }
                Ok(tools)
            });
    }
    builder = builder.host_context(host_context);
    if let Some(checkpoint) = checkpoint {
        builder = builder.resume(checkpoint).map_err(js_agent_error)?;
    }
    let (agent, events) = builder.build().map_err(js_error)?;
    Ok((agent.with_transcript(seed).map_err(js_agent_error)?, events))
}
