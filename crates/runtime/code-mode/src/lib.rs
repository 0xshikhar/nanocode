#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(target_family = "wasm", allow(clippy::module_name_repetitions))]

#[doc(hidden)]
pub mod apply_patch;
#[cfg(not(target_family = "wasm"))]
pub mod attachment;
#[cfg(not(target_family = "wasm"))]
pub mod code_mode;

#[path = "code_mode/description.rs"]
mod code_mode_description;

mod code_mode_order;

#[path = "code_mode/spec.rs"]
mod code_mode_spec;

pub mod embedded;
#[cfg(not(target_family = "wasm"))]
pub mod extensions;

pub use nanocode_media::image;
#[cfg(not(target_family = "wasm"))]
pub mod mcp;
#[cfg(not(target_family = "wasm"))]
mod plan;
#[cfg(not(target_family = "wasm"))]
mod prepared;
#[cfg(not(target_family = "wasm"))]
pub mod runtime;

#[path = "runtime/selection.rs"]
mod selection;
#[cfg(not(target_family = "wasm"))]
mod shell;

pub mod standard;
#[cfg(not(target_family = "wasm"))]
mod view_image;
#[cfg(not(target_family = "wasm"))]
pub mod workspace_runtime;
#[cfg(not(target_family = "wasm"))]
pub use workspace_runtime::WorkspaceTools;

pub mod contract {
    #[cfg(not(target_family = "wasm"))]
    pub use async_trait::async_trait;
    pub use nanocode_tools::{
        DEFAULT_TOOL_OUTPUT_TOKENS, Tool, ToolContext, ToolDefinition, ToolError, ToolInput,
        ToolInputError, ToolOutput, ToolOutputBody, ToolOutputContent, ToolOutputWire,
        ToolProcessTraceWire, ToolResult,
    };
}

#[cfg(target_family = "wasm")]

pub mod code_mode {
    pub use crate::embedded::{
        CodeModeCell, CodeModeExecution, CodeModeNotification, CodeModeObserver, CodeModeUpdate,
        NestedToolCall,
    };
}

#[cfg(target_family = "wasm")]

pub mod runtime {
    pub use crate::{
        embedded::{
            EmbeddedToolMode, EmbeddedToolRuntime as ToolRuntime,
            EmbeddedToolRuntimeControl as ToolRuntimeControl, OwnedToolContext,
        },
        selection::{ToolExposure, ToolSource, Tools, ToolsBuildError, ToolsBuilder},
    };
}

#[cfg(not(target_family = "wasm"))]
pub(crate) use contract::ToolOutputBody;
#[cfg(not(target_family = "wasm"))]
pub(crate) use contract::ToolOutputContent;
pub use contract::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};
pub use nanocode_home::SessionEnvironment;
#[cfg(not(target_family = "wasm"))]
pub use nanocode_tools_macros::tool;
#[cfg(not(target_family = "wasm"))]
pub(crate) use nanocode_types::ImageDetail;
#[cfg(not(target_family = "wasm"))]
pub(crate) use runtime::DynamicToolProvider;

pub use selection::ToolExposure;

pub use selection::Tools;

pub use selection::{ToolSource, ToolsBuildError, ToolsBuilder};
#[cfg(not(target_family = "wasm"))]
pub use shell::{ForegroundOutput, ambient_sensitive_environment, run_bash};

pub(crate) use standard::StandardTool;

#[doc(hidden)]
pub mod __private {
    #[cfg(not(target_family = "wasm"))]
    pub use async_trait::async_trait;
    #[cfg(not(target_family = "wasm"))]
    pub use schemars;
    #[cfg(not(target_family = "wasm"))]
    pub use serde;

    #[cfg(not(target_family = "wasm"))]
    pub use crate::runtime::schema_for;
    #[cfg(not(target_family = "wasm"))]
    pub use crate::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};

    pub fn model_contract(
        runtime: &crate::runtime::ToolRuntime,
        session_id: &str,
    ) -> (Vec<nanocode_tools::ToolDefinition>, Vec<(String, String)>) {
        runtime.model_contract(session_id)
    }
}
