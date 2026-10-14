use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::AttachmentMachine;

pub(crate) const JSON_SAFE_INTEGER_MAX: u64 = 9_007_199_254_740_991;

pub(crate) const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(20);

#[derive(Debug, Default, Clone, Serialize)]
pub(crate) struct ReceiptTiming {
    pub(crate) scheduler_ms: f64,
    pub(crate) execution_gate_ms: f64,
    pub(crate) execution_ms: f64,
    pub(crate) result_encode_ms: f64,
    pub(crate) result_queue_ms: f64,
    pub(crate) host_elapsed_ms: f64,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DiagnosticStage {
    Received,
    ExecutionStarted,
    ExecutionFinished,
    ResultPrepared,
}

impl DiagnosticStage {
    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::Received => "received",
            Self::ExecutionStarted => "execution_started",
            Self::ExecutionFinished => "execution_finished",
            Self::ResultPrepared => "result_prepared",
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ExecutorFrame<'a> {
    Catalog {
        capabilities: [&'static str; 1],
        runtime_id: &'a str,
        command_recovery: bool,
        turn_lifecycle: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        diagnostics: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        connection_id: Option<&'a str>,
        tools: &'a Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        machines: Option<&'a [AttachmentMachine]>,
        #[serde(skip_serializing_if = "Option::is_none")]
        attachment_id: Option<&'a str>,
    },
    Diagnostic {
        call_id: &'a str,
        stage: DiagnosticStage,
        elapsed_ms: f64,
    },
    Result {
        call_id: &'a str,
        outcome: &'a Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        timing: Option<&'a ReceiptTiming>,
    },
    Status {
        call_id: &'a str,
        state: &'static str,
    },
    Drain {},
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RemoteFrame {
    Ready {},
    Call {
        session_id: String,
        #[serde(default, deserialize_with = "deserialize_turn_id")]
        turn_id: Option<String>,
        call_id: String,
        model: String,
        name: String,
        input: Value,
        output_token_budget: u64,
        output_byte_budget: u64,
        deadline_at: u64,
    },
    TurnEnded {
        session_id: String,
        turn_id: String,
        hook_event_name: String,
    },
    Cancel {
        call_id: String,
    },
    Ack {
        call_id: String,
    },
    Recover {
        call_ids: Vec<String>,
    },
    Draining {},
}

impl RemoteFrame {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Ready {} => "ready",
            Self::Call { .. } => "call",
            Self::Cancel { .. } => "cancel",
            Self::TurnEnded { .. } => "turn_ended",
            Self::Ack { .. } => "ack",
            Self::Recover { .. } => "recover",
            Self::Draining {} => "draining",
        }
    }

    pub(crate) fn parse(text: &str) -> Result<Self, &'static str> {
        let frame: Self = serde_json::from_str(text).map_err(|_| "invalid attachment frame")?;
        frame.validate()?;
        Ok(frame)
    }

    fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Ready {} | Self::Draining {} => Ok(()),
            Self::Call {
                session_id,
                turn_id,
                call_id,
                model: _,
                name,
                input,
                output_token_budget,
                output_byte_budget,
                deadline_at,
            } => {
                if !valid_identifier(session_id)
                    || turn_id
                        .as_ref()
                        .is_some_and(|turn| turn.is_empty() || turn.len() > 256)
                    || !valid_identifier(call_id)
                    || !valid_tool_name(name)
                    || !(input.is_object() || input.is_string())
                    || *output_token_budget > JSON_SAFE_INTEGER_MAX
                    || !positive(*output_byte_budget)
                    || !positive(*deadline_at)
                {
                    return Err("invalid call");
                }
                Ok(())
            }
            Self::TurnEnded {
                session_id,
                turn_id,
                hook_event_name,
            } => {
                if valid_identifier(session_id)
                    && !turn_id.is_empty()
                    && turn_id.len() <= 256
                    && matches!(
                        hook_event_name.as_str(),
                        "Stop" | "Interrupt" | "SubagentStop"
                    )
                {
                    Ok(())
                } else {
                    Err("invalid turn identity")
                }
            }
            Self::Cancel { call_id } | Self::Ack { call_id } => {
                if valid_identifier(call_id) {
                    Ok(())
                } else {
                    Err("invalid call identity")
                }
            }
            Self::Recover { call_ids } => {
                if call_ids.is_empty()
                    || call_ids.len() > 100
                    || !call_ids.iter().all(|id| valid_identifier(id))
                {
                    Err("invalid recovery batch")
                } else {
                    Ok(())
                }
            }
        }
    }
}

fn deserialize_turn_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    String::deserialize(deserializer).map(Some)
}

const fn positive(value: u64) -> bool {
    value > 0 && value <= JSON_SAFE_INTEGER_MAX
}

fn valid_identifier(value: &str) -> bool {
    value.len() <= 128
        && value
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

fn valid_tool_name(value: &str) -> bool {
    valid_identifier(value) && !matches!(value, "exec" | "tool_search" | "wait")
}
