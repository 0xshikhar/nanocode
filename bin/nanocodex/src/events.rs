use std::{collections::HashMap, time::Duration};

use nanocode::agent::events::{AgentEvent, AgentEventKind};
use nanocode_tui::transcript::{CellKind, NoticeLevel, ToolBody, ToolCell, ToolStatus, Transcript};
use serde_json::Value;

#[derive(Default)]
pub struct EventMapper {
    assistant: HashMap<String, usize>,
    reasoning: HashMap<u64, usize>,
    tools: HashMap<String, usize>,
    usage: Usage,
    last: Usage,
    session: Usage,
    turn_start: usize,
    reported: bool,
}

#[derive(Clone, Copy, Default)]
pub struct Usage {
    pub input: u64,
    pub cached: u64,
    pub output: u64,
    pub tool_calls: u64,
    pub turns: u64,
}

fn completion_agent(text: &str) -> Option<&str> {
    text.rsplit_once("<subagent_completion agent_id=\"")
        .and_then(|(_, rest)| rest.split_once('"'))
        .map(|(agent, _)| agent)
}

pub enum Accepted {
    Prompt { request_id: Option<String> },
    Steer { id: Option<String>, text: String },
}

pub struct Mapped {
    pub accepted: Option<Accepted>,
}

impl EventMapper {
    pub fn reset(&mut self) {
        self.assistant.clear();
        self.reasoning.clear();
        self.tools.clear();
    }

    pub const fn begin_turn(&mut self) {
        self.reported = false;
    }

    pub const fn reported(&self) -> bool {
        self.reported
    }

    pub const fn session_usage(&self) -> Usage {
        self.session
    }

    pub const fn last_usage(&self) -> Usage {
        self.last
    }

    pub const fn turn_start(&self) -> usize {
        self.turn_start
    }

    pub const fn mark_turn_start(&mut self, index: usize) {
        self.turn_start = index;
    }

    pub fn apply(&mut self, transcript: &mut Transcript, event: &AgentEvent) -> Mapped {
        let mut mapped = Mapped { accepted: None };
        let Ok(payload) = serde_json::from_str::<Value>(event.payload.get()) else {
            transcript.push(CellKind::Notice(
                NoticeLevel::Error,
                format!("undecodable {:?} event", event.kind),
            ));
            return mapped;
        };
        match event.kind {
            AgentEventKind::InputAccepted => {
                let text = prompt_text(&payload["input"]);
                let shown = text.as_str();
                match completion_agent(shown) {
                    Some(agent) => transcript.push(CellKind::Notice(
                        NoticeLevel::Info,
                        format!("subagent #{agent} completed · continuing"),
                    )),
                    None => transcript.push(CellKind::User(shown.to_owned())),
                };
                self.reasoning.clear();
                mapped.accepted = Some(match payload["kind"].as_str() {
                    Some("steer") => Accepted::Steer {
                        id: payload["item_id"]
                            .as_str()
                            .and_then(|item| item.rsplit_once(":steer:"))
                            .map(|(_, id)| id.to_owned()),
                        text,
                    },
                    _ => Accepted::Prompt {
                        request_id: payload["request_id"].as_str().map(str::to_owned),
                    },
                });
            }
            AgentEventKind::RunStarted => {
                self.reset();
                self.usage = Usage::default();
                self.reported = false;
                self.turn_start = transcript.len();
            }
            AgentEventKind::ModelCallCompleted => {
                let usage = &payload["usage"];
                self.usage.input += usage["input_tokens"].as_u64().unwrap_or_default();
                self.usage.output += usage["output_tokens"].as_u64().unwrap_or_default();
                self.usage.cached += usage["input_tokens_details"]["cached_tokens"]
                    .as_u64()
                    .unwrap_or_default();
            }
            AgentEventKind::AssistantDelta | AgentEventKind::AssistantMessage => {
                let key = payload["item_id"].as_str().map_or_else(
                    || format!("call-{}", payload["model_call_index"]),
                    str::to_owned,
                );
                let text = payload["text"].as_str().unwrap_or_default();
                let final_message = event.kind == AgentEventKind::AssistantMessage;
                match self.assistant.get(&key).copied() {
                    Some(index) => {
                        transcript.update(index, |cell| {
                            if let CellKind::Assistant(existing) = cell {
                                if final_message {
                                    text.clone_into(existing);
                                } else {
                                    existing.push_str(text);
                                }
                            }
                        });
                    }
                    None if !text.is_empty() => {
                        let index = transcript.push(CellKind::Assistant(text.to_owned()));
                        self.assistant.insert(key, index);
                        self.reasoning.clear();
                    }
                    None => {}
                }
            }
            AgentEventKind::ReasoningSummaryDelta => {
                let call = payload["model_call_index"].as_u64().unwrap_or_default();
                let text = payload["text"].as_str().unwrap_or_default();
                match self.reasoning.get(&call).copied() {
                    Some(index) if transcript.last().is_some_and(|(last, _)| last == index) => {
                        transcript.update(index, |cell| {
                            if let CellKind::Reasoning(existing) = cell {
                                existing.push_str(text);
                            }
                        });
                    }
                    _ if !text.trim().is_empty() => {
                        let index = transcript.push(CellKind::Reasoning(text.to_owned()));
                        self.reasoning.insert(call, index);
                    }
                    _ => {}
                }
            }
            AgentEventKind::ToolCall => self.tool_call(transcript, &payload),
            AgentEventKind::ToolResult => self.tool_result(transcript, &payload),
            AgentEventKind::RunError => {
                let message = payload["message"]
                    .as_str()
                    .unwrap_or("run error")
                    .to_owned();
                if !transcript.has_notice_since(self.turn_start, NoticeLevel::Error, &message) {
                    transcript.push(CellKind::Notice(NoticeLevel::Error, message));
                }
            }
            AgentEventKind::ModelCallFailed => {
                transcript.push(CellKind::Notice(
                    NoticeLevel::Warning,
                    format!(
                        "model call failed: {}",
                        payload["error"].as_str().unwrap_or_default()
                    ),
                ));
            }
            AgentEventKind::ModelAttemptRetrying => {
                let attempt = payload["attempt"].as_u64().unwrap_or_default();
                let error = payload["error"].as_str().unwrap_or_default();
                transcript.push(CellKind::Notice(
                    NoticeLevel::Warning,
                    format!("retrying model request (attempt {attempt}) {error}")
                        .trim()
                        .to_owned(),
                ));
            }
            AgentEventKind::ModelCompactionStarted => {
                transcript.push(CellKind::Notice(
                    NoticeLevel::Info,
                    "compacting context…".into(),
                ));
            }
            AgentEventKind::ModelCompactionFailed => {
                transcript.push(CellKind::Notice(
                    NoticeLevel::Warning,
                    format!(
                        "compaction failed: {}",
                        payload["error"].as_str().unwrap_or_default()
                    ),
                ));
            }
            AgentEventKind::RunCompleted | AgentEventKind::RunFailed => {
                let status = payload["status"].as_str().unwrap_or("failed");
                let tool_status = match status {
                    "completed" => ToolStatus::Completed,
                    "cancelled" => ToolStatus::Cancelled,
                    _ => ToolStatus::Failed,
                };
                transcript.finish_running_tools(tool_status);
                transcript.push(CellKind::Summary(summary(&payload, status, self.usage)));
                self.last = self.usage;
                self.session.input += self.usage.input;
                self.session.cached += self.usage.cached;
                self.session.output += self.usage.output;
                self.session.tool_calls += self.usage.tool_calls;
                self.session.turns += 1;
                self.reported = true;
                self.reset();
            }
            _ => {}
        }
        mapped
    }

    fn tool_call(&mut self, transcript: &mut Transcript, payload: &Value) {
        self.usage.tool_calls += 1;
        let call_id = payload["call_id"].as_str().unwrap_or_default().to_owned();
        let name = payload["tool"].as_str().unwrap_or("tool").to_owned();
        let parent = payload["parent_call_id"]
            .as_str()
            .map(str::to_owned)
            .or_else(|| {
                call_id
                    .rsplit_once('/')
                    .map(|(parent, _)| parent.to_owned())
            });
        let depth = parent
            .as_ref()
            .filter(|parent| self.tools.contains_key(*parent))
            .map_or(0, |_| 1);
        let arguments = match &payload["arguments"] {
            Value::String(text) => {
                serde_json::from_str(text).unwrap_or_else(|_| Value::String(text.clone()))
            }
            other => other.clone(),
        };
        let mut cell = ToolCell::new(call_id.clone(), name.clone());
        cell.depth = depth;
        describe_tool(&mut cell, &name, &arguments);
        self.reasoning.clear();
        let index = transcript.push(CellKind::Tool(cell));
        self.tools.insert(call_id, index);
    }

    fn tool_result(&mut self, transcript: &mut Transcript, payload: &Value) {
        let call_id = payload["call_id"].as_str().unwrap_or_default();
        let Some(index) = self
            .tools
            .get(call_id)
            .copied()
            .or_else(|| transcript.find_tool(call_id))
        else {
            let name = payload["tool"].as_str().unwrap_or("tool").to_owned();
            let display = if name == "exec" {
                "code".to_owned()
            } else {
                name
            };
            let cell = ToolCell::new(call_id.to_owned(), display);
            let index = transcript.push(CellKind::Tool(cell));
            self.tools.insert(call_id.to_owned(), index);
            self.tool_result(transcript, payload);
            return;
        };
        let status = match payload["status"].as_str() {
            Some("completed") => ToolStatus::Completed,
            Some("cancelled") => ToolStatus::Cancelled,
            _ => ToolStatus::Failed,
        };
        let duration = payload["duration_ns"].as_u64().map(Duration::from_nanos);
        let output = output_text(payload);
        transcript.update(index, |cell| {
            if let CellKind::Tool(tool) = cell {
                tool.status = status;
                tool.duration = duration;
                if tool.output_body == ToolBody::Text && looks_like_diff(&output) {
                    tool.output_body = ToolBody::Diff;
                }
                tool.output = output;
            }
        });
    }
}

pub fn describe_tool(cell: &mut ToolCell, name: &str, arguments: &Value) {
    let field = |key: &str| {
        arguments
            .get(key)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    match name {
        "exec" | "code_mode" | "js" => {
            cell.name = "code".into();
            cell.input = match arguments {
                Value::String(code) => code.clone(),
                other => field("code").unwrap_or_else(|| other.to_string()),
            };
            cell.input_body = ToolBody::Code;
        }
        "wait" => {
            cell.title = field("cell_id")
                .map(|id| format!("cell {id}"))
                .unwrap_or_default();
        }
        "exec_command" | "Bash" | "shell" | "run_command" => {
            cell.name = "run".into();
            cell.title = field("cmd")
                .or_else(|| field("command"))
                .unwrap_or_default();
        }
        "apply_patch" => {
            cell.name = "patch".into();
            cell.input =
                field("input")
                    .or_else(|| field("patch"))
                    .unwrap_or_else(|| match arguments {
                        Value::String(patch) => patch.clone(),
                        other => other.to_string(),
                    });
            cell.input_body = ToolBody::Diff;
        }
        "Edit" | "MultiEdit" | "Write" | "Read" | "NotebookEdit" | "view_image" => {
            cell.title = field("file_path")
                .or_else(|| field("path"))
                .unwrap_or_default();
        }
        "Glob" | "Grep" => {
            cell.title = field("pattern").unwrap_or_default();
            if let Some(path) = field("path") {
                cell.title = format!("{} in {path}", cell.title);
            }
        }
        "WebSearch" | "web_search" => cell.title = field("query").unwrap_or_default(),
        "WebFetch" => cell.title = field("url").unwrap_or_default(),
        "spawn_agent" => {
            cell.name = "spawn agent".into();
            cell.title = field("role").unwrap_or_default();
            cell.input = field("task").unwrap_or_default();
        }
        "send_agent_message" => {
            cell.name = "message agent".into();
            cell.title = arguments
                .get("agent_id")
                .map(Value::to_string)
                .unwrap_or_default();
            cell.input = field("message").unwrap_or_default();
        }
        "wait_agent" => cell.name = "wait for agents".into(),
        _ => {
            cell.title = match arguments {
                Value::Object(map) if map.is_empty() => String::new(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
        }
    }
}

fn prompt_text(input: &Value) -> String {
    match input {
        Value::String(text) => text.clone(),
        other => {
            let mut texts = Vec::new();
            collect_text(other, &mut texts);
            if texts.is_empty() {
                other.to_string()
            } else {
                texts.join("\n")
            }
        }
    }
}

fn collect_text(value: &Value, texts: &mut Vec<String>) {
    match value {
        Value::String(text) => texts.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_text(item, texts)),
        Value::Object(map) => {
            if let Some(Value::String(text)) = map.get("text") {
                texts.push(text.clone());
            } else if map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.contains("image"))
            {
                let source = ["path", "image_url", "file_id"]
                    .into_iter()
                    .find_map(|key| map.get(key).and_then(Value::as_str))
                    .unwrap_or("attached");
                let name = source.rsplit('/').next().unwrap_or(source);
                texts.push(format!("[image: {name}]"));
            } else {
                map.values().for_each(|item| collect_text(item, texts));
            }
        }
        _ => {}
    }
}

fn output_text(payload: &Value) -> String {
    if let Some(output) = payload["structured_result"]
        .get("output")
        .and_then(Value::as_str)
    {
        return output.to_owned();
    }
    let mut texts = Vec::new();
    collect_text(&payload["result"], &mut texts);
    let text = texts.concat();
    humanize(strip_code_header(&text))
}

pub fn humanize(text: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(text.trim()) else {
        return text.to_owned();
    };
    match &value {
        Value::Object(map) if map.contains_key("stdout") || map.contains_key("stderr") => {
            let mut parts = Vec::new();
            for key in ["stdout", "stderr", "output"] {
                if let Some(text) = map
                    .get(key)
                    .and_then(Value::as_str)
                    .filter(|text| !text.is_empty())
                {
                    parts.push(text.trim_end().to_owned());
                }
            }
            if let Some(code) = map
                .get("exit_code")
                .and_then(Value::as_i64)
                .filter(|code| *code != 0)
            {
                parts.push(format!("exit code {code}"));
            }
            parts.join("\n")
        }
        Value::Object(map)
            if map.contains_key("agents")
                || map.contains_key("agent_id")
                || map.contains_key("spawned") =>
        {
            let mut lines = Vec::new();
            agent_lines(&value, &mut lines);
            if lines.is_empty() {
                text.to_owned()
            } else {
                lines.join("\n")
            }
        }
        Value::Object(map) => map
            .iter()
            .map(|(key, value)| match value {
                Value::String(text) => format!("{key}: {text}"),
                other => format!("{key}: {other}"),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => text.to_owned(),
    }
}

fn agent_lines(value: &Value, lines: &mut Vec<String>) {
    match value {
        Value::Object(map) if map.contains_key("agent_id") => {
            let id = map
                .get("agent_id")
                .map(Value::to_string)
                .unwrap_or_default();
            let role = map.get("role").and_then(Value::as_str).unwrap_or("agent");
            let state = map["status"]["state"].as_str().unwrap_or("unknown");
            let mut line = format!("#{id} {role} · {state}");
            match &map["status"]["output"] {
                Value::Null => {}
                Value::String(output) => {
                    line.push_str(&format!(" · {}", output.lines().next().unwrap_or_default()));
                }
                other => line.push_str(&format!(" · {other}")),
            }
            if let Some(error) = map["status"]["error"].as_str() {
                line.push_str(&format!(" · {error}"));
            }
            lines.push(line);
        }
        Value::Object(map) => map.values().for_each(|value| agent_lines(value, lines)),
        Value::Array(items) => items.iter().for_each(|item| agent_lines(item, lines)),
        _ => {}
    }
}

pub fn clean_output(text: &str) -> String {
    humanize(strip_code_header(text))
}

fn strip_code_header(text: &str) -> &str {
    let Some(rest) = text
        .strip_prefix("Script completed\n")
        .or_else(|| text.strip_prefix("Script failed\n"))
    else {
        return text;
    };
    let rest = rest
        .strip_prefix("Wall time")
        .and_then(|rest| rest.split_once('\n'))
        .map_or(rest, |(_, rest)| rest);
    rest.strip_prefix("Output:\n").unwrap_or(rest)
}

pub fn looks_like_diff(text: &str) -> bool {
    text.starts_with("diff --git") || text.starts_with("--- ") && text.contains("\n+++ ")
}

fn summary(payload: &Value, status: &str, observed: Usage) -> String {
    let mut parts = Vec::new();
    if status != "completed" {
        parts.push(status.to_owned());
    }
    if let Some(duration) = payload["duration_ns"].as_u64() {
        parts.push(nanocode_tui::format::format_duration(duration));
    }
    let tools = payload["tool_calls"]
        .as_u64()
        .unwrap_or(observed.tool_calls);
    if tools > 0 {
        parts.push(format!(
            "{tools} tool call{}",
            if tools == 1 { "" } else { "s" }
        ));
    }
    let usage = &payload["usage"];
    let (input, output, cached) = match (
        usage["input_tokens"].as_u64(),
        usage["output_tokens"].as_u64(),
    ) {
        (Some(input), Some(output)) => (
            input,
            output,
            usage["cached_input_tokens"].as_u64().unwrap_or_default(),
        ),
        _ => (observed.input, observed.output, observed.cached),
    };
    if input > 0 || output > 0 {
        let mut usage_text = format!("{} in · {} out", tokens(input), tokens(output));
        if cached > 0 {
            usage_text.push_str(&format!(" · {} cached", tokens(cached)));
        }
        parts.push(usage_text);
    }
    if let Some(cost) = payload["cost_usd"].as_f64() {
        parts.push(format!("{}{cost:.4}", '$'));
    }
    parts.join(" · ")
}

fn tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}k", count as f64 / 1_000.0)
    } else {
        count.to_string()
    }
}
