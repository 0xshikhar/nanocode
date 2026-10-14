use super::{Runner, RunnerPool, RunnerServerConfig};
use crate::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

pub(crate) struct RunnerManagement {
    pub pool: RunnerPool,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    List {},
    Connect {
        url: String,
        token: String,
        name: Option<String>,
        mode: Option<Mode>,
        call_timeout_ms: Option<u64>,
    },
    Listen {
        address: String,
        token: Option<String>,
        mode: Option<Mode>,
        call_timeout_ms: Option<u64>,
    },
    Disconnect {
        name: String,
    },
    StopListening {},
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Mode {
    Alongside,
    Replace,
}

#[async_trait]
impl Tool for RunnerManagement {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::function(
            "runners",
            "Manage workspace runners shared by all agents. action=list shows status and /name mounts. action=connect requires url and token; name selects the mount, mode defaults to alongside and replace must be explicitly requested. action=listen requires address, accepts multiple authenticated runners, and returns an endpoint and credential to give the runner; call it only when the user requests a listener. action=disconnect requires name. action=stop_listening stops all listeners. Attachments are announced in conversation messages; tool definitions stay unchanged. Credentials are excluded from list/connect outputs. call_timeout_ms is an optional caller deadline policy, in positive whole milliseconds.",
            json!({
                "type":"object",
                "properties":{
                    "action":{"type":"string","enum":["list","connect","listen","disconnect","stop_listening"]},
                    "url":{"type":"string"},
                    "token":{"type":"string"},
                    "name":{"type":"string"},
                    "address":{"type":"string"},
                    "mode":{"type":"string","enum":["alongside","replace"]},
                    "call_timeout_ms":{"type":"integer","minimum":1}
                },
                "required":["action"],
                "additionalProperties":false
            }),
        )
    }

    async fn execute(&self, input: ToolInput, _context: ToolContext<'_>) -> ToolResult {
        let request: Request = input.decode_json()?;
        let result = match request {
            Request::List {} => {
                json!({"runners":self.pool.list().into_iter().map(info).collect::<Vec<_>>(),"listeners":self.pool.servers().iter().map(|server| json!({"url":server.endpoint(),"token":"[REDACTED]"})).collect::<Vec<_>>() })
            }
            Request::Connect {
                url,
                token,
                name,
                mode,
                call_timeout_ms,
            } => {
                let mut config = Runner::connect(url).token(token);
                if let Some(name) = name {
                    config = config.name(name);
                }
                if matches!(mode, Some(Mode::Replace)) {
                    config = config.replace();
                }
                if let Some(ms) = call_timeout_ms {
                    config = config.call_timeout(Duration::from_millis(ms));
                }
                info(self.pool.connect(config).await?.info())
            }
            Request::Listen {
                address,
                token,
                mode,
                call_timeout_ms,
            } => {
                let mut config = RunnerServerConfig::listen(address);
                if let Some(token) = token {
                    config = config.token(token);
                }
                if matches!(mode, Some(Mode::Replace)) {
                    config = config.replace();
                }
                if let Some(ms) = call_timeout_ms {
                    config = config.call_timeout(Duration::from_millis(ms));
                }
                let server = self.pool.listen(config).await?;
                json!({"url":server.endpoint(), "token":server.token(), "instruction":"Give this endpoint and credential to the runner's connect command."})
            }
            Request::Disconnect { name } => {
                self.pool.disconnect(&name).await?;
                json!({"name":name,"status":"disconnected"})
            }
            Request::StopListening {} => {
                for server in self.pool.servers() {
                    server.stop().await?;
                }
                json!({"status":"stopped_listening"})
            }
        };
        Ok(ToolOutput::json(&result).with_structured_result(result))
    }
}

fn info(info: super::RunnerInfo) -> Value {
    json!({"name":info.name,"mount":format!("/{}",info.name),"url":info.endpoint,"mode":match info.mode {super::RemoteMode::Alongside=>"alongside",super::RemoteMode::Replace=>"replace"},"status":match info.status {super::RunnerStatus::Connecting=>"connecting".to_owned(),super::RunnerStatus::Ready=>"ready".to_owned(),super::RunnerStatus::Disconnected=>"disconnected".to_owned(),super::RunnerStatus::Failed(error)=>format!("failed: {error}")},"tool_count":info.tool_count})
}
