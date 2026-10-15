pub use nanocode_code_mode::{code_mode, code_mode_description, code_mode_spec};
pub use nanocode_home::SessionEnvironment;
pub use nanocode_media::image;
pub use nanocode_tools::{
    Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult, tool,
};

pub mod contract {
    pub use nanocode_tools::*;
}

pub mod embedded;
pub mod remote;
#[path = "runtime/selection.rs"]
mod selection;
pub use selection::{ToolExposure, ToolSource, Tools, ToolsBuildError, ToolsBuilder};
#[cfg(not(target_family = "wasm"))]
pub mod attachment;
#[cfg(not(target_family = "wasm"))]
pub mod mcp;
#[cfg(not(target_family = "wasm"))]
mod prepared;
#[cfg(not(target_family = "wasm"))]
pub mod runtime;
#[cfg(not(target_family = "wasm"))]
pub use nanocode_process as shell;
#[cfg(not(target_family = "wasm"))]
pub mod workspace_runtime;
#[cfg(not(target_family = "wasm"))]
pub(crate) use nanocode_tools::ToolOutputContent;
#[cfg(not(target_family = "wasm"))]
pub(crate) use runtime::DynamicToolProvider;
#[cfg(not(target_family = "wasm"))]
pub use shell::{ForegroundOutput, ambient_sensitive_environment, run_bash};
#[cfg(not(target_family = "wasm"))]
pub use workspace_runtime::WorkspaceTools;
mod code_mode_order;

#[cfg(target_family = "wasm")]
pub mod runtime {
    pub use crate::embedded::{
        EmbeddedToolRuntime as ToolRuntime, EmbeddedToolRuntimeControl as ToolRuntimeControl,
        OwnedToolContext,
    };
    pub use crate::{ToolExposure, ToolSource, Tools, ToolsBuildError, ToolsBuilder};
}

pub mod __private {
    pub use nanocode_tools::__private::*;
    pub fn model_contract(
        runtime: &crate::runtime::ToolRuntime,
        session_id: &str,
    ) -> (Vec<crate::ToolDefinition>, Vec<(String, String)>) {
        runtime.model_contract(session_id)
    }
}
