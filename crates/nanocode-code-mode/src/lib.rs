#![cfg_attr(
    all(not(feature = "native"), feature = "workspace-runtime"),
    doc = include_str!("../WORKSPACE_RUNTIME.md")
)]
#![cfg_attr(
    not(any(feature = "native", feature = "workspace-runtime")),
    doc = "Dependency-light model-visible tool contracts for Nanocodex."
)]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(target_family = "wasm", allow(clippy::module_name_repetitions))]

#[cfg(feature = "workspace-runtime")]
#[doc(hidden)]
pub mod apply_patch;
#[cfg(all(not(target_family = "wasm"), feature = "attachment"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub mod attachment;
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub mod code_mode;
#[cfg(feature = "code-mode")]
#[path = "code_mode/description.rs"]
mod code_mode_description;
#[cfg(feature = "code-mode")]
mod code_mode_order;
#[cfg(feature = "code-mode")]
#[path = "code_mode/spec.rs"]
mod code_mode_spec;
#[cfg(feature = "code-mode")]
pub mod embedded;
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
pub mod extensions;
#[cfg(feature = "image-processing")]
pub use nanocode_media::image;
#[cfg(all(not(target_family = "wasm"), feature = "mcp"))]
#[cfg_attr(docsrs, doc(cfg(all(not(target_family = "wasm"), feature = "mcp"))))]
pub mod mcp;
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
mod plan;
#[cfg(all(not(target_family = "wasm"), feature = "attachment"))]
mod prepared;
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub mod runtime;
#[cfg(any(
    feature = "code-mode",
    all(not(target_family = "wasm"), feature = "attachment")
))]
#[path = "runtime/selection.rs"]
mod selection;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
mod shell;
#[cfg(feature = "workspace-runtime")]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub mod standard;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
mod view_image;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub mod workspace_runtime;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
pub use workspace_runtime::WorkspaceTools;

pub mod contract {
    #[cfg(all(
        not(target_family = "wasm"),
        any(feature = "attachment", feature = "workspace-runtime")
    ))]
    #[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
    pub use async_trait::async_trait;
    pub use nanocode_tools::{
        DEFAULT_TOOL_OUTPUT_TOKENS, Tool, ToolContext, ToolDefinition, ToolError, ToolInput,
        ToolInputError, ToolOutput, ToolOutputBody, ToolOutputContent, ToolOutputWire,
        ToolProcessTraceWire, ToolResult,
    };
}

#[cfg(all(target_family = "wasm", feature = "code-mode"))]

pub mod code_mode {
    pub use crate::embedded::{
        CodeModeCell, CodeModeExecution, CodeModeNotification, CodeModeObserver, CodeModeUpdate,
        NestedToolCall,
    };
}

#[cfg(all(target_family = "wasm", feature = "code-mode"))]

pub mod runtime {
    pub use crate::{
        embedded::{
            EmbeddedToolMode, EmbeddedToolRuntime as ToolRuntime,
            EmbeddedToolRuntimeControl as ToolRuntimeControl, OwnedToolContext,
        },
        selection::{ToolExposure, ToolSource, Tools, ToolsBuildError, ToolsBuilder},
    };
}

#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
pub(crate) use contract::ToolOutputBody;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
pub(crate) use contract::ToolOutputContent;
pub use contract::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};
pub use nanocode_home::SessionEnvironment;
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
#[cfg_attr(docsrs, doc(cfg(not(target_family = "wasm"))))]
pub use nanocode_tools_macros::tool;
#[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
pub(crate) use nanocode_types::ImageDetail;
#[cfg(all(not(target_family = "wasm"), feature = "mcp"))]
pub(crate) use runtime::DynamicToolProvider;
#[cfg(any(
    feature = "code-mode",
    all(not(target_family = "wasm"), feature = "attachment")
))]
pub use selection::ToolExposure;
#[cfg(any(
    feature = "code-mode",
    all(not(target_family = "wasm"), feature = "attachment")
))]
pub use selection::Tools;
#[cfg(any(
    feature = "code-mode",
    all(not(target_family = "wasm"), feature = "attachment")
))]
pub use selection::{ToolSource, ToolsBuildError, ToolsBuilder};
#[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
#[cfg_attr(
    docsrs,
    doc(cfg(all(not(target_family = "wasm"), feature = "code-mode")))
)]
pub use shell::{ForegroundOutput, ambient_sensitive_environment, run_bash};
#[cfg(feature = "workspace-runtime")]
pub(crate) use standard::StandardTool;

#[doc(hidden)]
pub mod __private {
    #[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
    pub use async_trait::async_trait;
    #[cfg(all(not(target_family = "wasm"), feature = "workspace-runtime"))]
    pub use schemars;
    #[cfg(not(target_family = "wasm"))]
    pub use serde;

    #[cfg(all(not(target_family = "wasm"), feature = "code-mode"))]
    pub use crate::runtime::schema_for;
    #[cfg(not(target_family = "wasm"))]
    pub use crate::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};

    #[cfg(feature = "code-mode")]
    pub fn model_contract(
        runtime: &crate::runtime::ToolRuntime,
        session_id: &str,
    ) -> (Vec<nanocode_tools::ToolDefinition>, Vec<(String, String)>) {
        runtime.model_contract(session_id)
    }
}
