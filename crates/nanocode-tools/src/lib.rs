//! Shared invocation traits and macro support for Codex and Claude tools.
use async_trait::async_trait;
/// Defines a typed tool implementing the shared [`Tool`] contract.
pub use nanocode_tools_macros::tool;
use nanocode_types::responses::ResponseItem;
pub use nanocode_types::tools::*;
use serde::de::DeserializeOwned;
use serde_json::value::RawValue;
mod schema;
pub use schema::schema_for;
#[doc(hidden)]
pub mod __private {
    pub use crate::{
        Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult, schema_for,
    };
    pub use async_trait::async_trait;
    pub use schemars;
    pub use serde;
}

/// Read-only context for one tool invocation.
#[derive(Clone, Copy)]
pub struct ToolContext<'a> {
    model: &'a str,
    session_id: &'a str,
    call_id: &'a str,
    turn_id: Option<&'a str>,
    history: &'a [ResponseItem],
    output_token_budget: usize,
    host_context: Option<&'a str>,
    instruction_revision: Option<u64>,
}

impl<'a> ToolContext<'a> {
    /// Creates the complete read-only context for one tool invocation.
    #[must_use]
    pub const fn new(
        model: &'a str,
        session_id: &'a str,
        call_id: &'a str,
        history: &'a [ResponseItem],
        output_token_budget: usize,
    ) -> Self {
        Self {
            model,
            session_id,
            call_id,
            turn_id: None,
            history,
            output_token_budget,
            host_context: None,
            instruction_revision: None,
        }
    }

    /// Captures the runtime instruction revision at the model call boundary.
    #[must_use]
    pub const fn with_instruction_revision(mut self, revision: Option<u64>) -> Self {
        self.instruction_revision = revision;
        self
    }

    /// Returns the captured revision, never a live runtime lookup.
    #[must_use]
    pub const fn instruction_revision(self) -> Option<u64> {
        self.instruction_revision
    }

    /// Attaches embedding-owned invocation context without exposing it to tool
    /// schemas or serialized model-visible values.
    #[doc(hidden)]
    #[must_use]
    pub const fn with_host_context(mut self, host_context: Option<&'a str>) -> Self {
        self.host_context = host_context;
        self
    }

    /// Returns embedding-owned invocation context, when one was supplied.
    #[doc(hidden)]
    #[must_use]
    pub const fn host_context(self) -> Option<&'a str> {
        self.host_context
    }

    /// Returns the fixed model contract for this invocation.
    #[must_use]
    pub const fn model(self) -> &'a str {
        self.model
    }

    /// Returns the stable client-owned session identity.
    #[must_use]
    pub const fn session_id(self) -> &'a str {
        self.session_id
    }

    /// Returns the provider tool-call identity.
    #[must_use]
    pub const fn call_id(self) -> &'a str {
        self.call_id
    }

    /// Attaches the host's stable logical-turn identity. This is shared by all
    /// tool calls in a turn and must not be replaced with a tool-call identity.
    #[must_use]
    pub const fn with_turn_id(mut self, turn_id: Option<&'a str>) -> Self {
        self.turn_id = turn_id;
        self
    }

    /// Returns the logical-turn identity, if supplied by the execution host.
    #[must_use]
    pub const fn turn_id(self) -> Option<&'a str> {
        self.turn_id
    }

    /// Returns committed authoritative history visible at this call boundary.
    #[must_use]
    pub const fn history(self) -> &'a [ResponseItem] {
        self.history
    }

    /// Returns the requested model-visible tool-output budget, or `usize::MAX`
    /// when no budget was requested.
    #[must_use]
    pub const fn output_token_budget(self) -> usize {
        self.output_token_budget
    }
}

/// Canonical input presented to function and freeform tools.
pub enum ToolInput {
    /// Validated raw JSON arguments from a function call.
    Function(Box<RawValue>),
    /// Complete freeform custom-tool input.
    Freeform(String),
}

impl ToolInput {
    /// Borrows raw JSON function arguments without materializing a value tree.
    ///
    /// # Errors
    ///
    /// Returns an error for freeform input.
    pub fn function_json(&self) -> Result<&RawValue, ToolInputError> {
        match self {
            Self::Function(input) => Ok(input),
            Self::Freeform(_) => Err(ToolInputError::ExpectedFunction),
        }
    }

    /// Decodes JSON function arguments into a caller-selected type.
    ///
    /// # Errors
    ///
    /// Returns an error for freeform input or invalid JSON arguments.
    pub fn decode_json<T: DeserializeOwned>(&self) -> Result<T, ToolInputError> {
        serde_json::from_str(self.function_json()?.get()).map_err(ToolInputError::Decode)
    }

    /// Extracts freeform source text.
    ///
    /// # Errors
    ///
    /// Returns an error for JSON function arguments.
    pub fn into_freeform(self) -> Result<String, ToolInputError> {
        match self {
            Self::Freeform(input) => Ok(input),
            Self::Function(_) => Err(ToolInputError::ExpectedFreeform),
        }
    }
}

/// Invalid access or decoding of typed tool input.
#[derive(Debug, thiserror::Error)]
pub enum ToolInputError {
    /// A function tool received freeform input.
    #[error("expected JSON function arguments")]
    ExpectedFunction,
    /// A custom tool received function arguments.
    #[error("expected freeform tool input")]
    ExpectedFreeform,
    /// Function arguments did not decode into the requested type.
    #[error("failed to parse function arguments: {0}")]
    Decode(#[source] serde_json::Error),
}

/// A caller-defined model-visible tool.
///
/// ```
/// use async_trait::async_trait;
/// use nanocode_types::responses::JsonSchema;
/// use nanocode_tools::{Tool, ToolContext, ToolDefinition, ToolInput, ToolOutput, ToolResult};
/// use serde_json::json;
///
/// struct DeploymentRegion;
///
/// #[async_trait]
/// impl Tool for DeploymentRegion {
///     fn definition(&self) -> ToolDefinition {
///         ToolDefinition::function(
///             "deployment_region",
///             "Return the production deployment region.",
///             JsonSchema::from(json!({
///                 "type": "object",
///                 "properties": {},
///                 "additionalProperties": false
///             })),
///         )
///     }
///
///     async fn execute(
///         &self,
///         input: ToolInput,
///         _context: ToolContext<'_>,
///     ) -> ToolResult {
///         let _: serde_json::Value = input.decode_json()?;
///         Ok(ToolOutput::text("us-west-2"))
///     }
/// }
/// ```
#[async_trait]
pub trait Tool: Send + Sync + 'static {
    /// Returns the complete model-visible definition and registry name.
    fn definition(&self) -> ToolDefinition;

    /// Returns whether this handler is safe to execute alongside sibling tool calls.
    ///
    /// Execution is serial by default. Opt in only when the handler's state and
    /// effects are safe to overlap with other parallel-capable tools.
    fn supports_parallel_tool_calls(&self) -> bool {
        false
    }

    /// Trusted runtime notification after a turn settles, including cancellation.
    /// This is not a model-callable tool and must not start a new resource session.
    async fn end_turn(
        &self,
        _session_id: &str,
        _turn_id: &str,
        _hook_event_name: &str,
    ) -> Result<(), ToolError> {
        Ok(())
    }

    /// Executes one invocation.
    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult;
}

/// Backend-independent registration of shared tool implementations.
pub trait ToolCollection: Sized {
    /// Error returned when a tool cannot be registered.
    type Error;

    /// Adds a shared tool while retaining the collection's execution policy.
    fn add_shared_tool(self, tool: std::sync::Arc<dyn Tool>) -> Result<Self, Self::Error>;
}

/// Invalid declarative tool selection.
#[derive(Debug, thiserror::Error)]
pub enum ToolsBuildError {
    /// A host capability could not be bound to its owning session.
    #[error("host tool initialization failed: {0}")]
    HostInitialization(String),

    /// A custom definition has an empty registry name.
    #[error("tool name must not be empty")]
    EmptyName,

    /// The model-visible working-directory override is empty.
    #[error("working directory override must not be empty")]
    EmptyWorkingDirectory,

    /// The model-visible shell override is empty.
    #[error("default shell override must not be empty")]
    EmptyDefaultShell,

    /// Two custom tools use the same definition name.
    #[error("tool name `{0}` is registered more than once")]
    DuplicateName(Box<str>),

    /// A singleton tool source was added more than once.
    #[error("tool source `{0}` is configured more than once")]
    DuplicateSource(&'static str),

    /// A custom tool collides with an enabled built-in tool.
    #[error("tool name `{0}` conflicts with an enabled built-in tool")]
    BuiltInName(Box<str>),

    /// A custom tool collides with a host-owned routing tool.
    #[error("tool name `{0}` is reserved by the Code Mode host")]
    ReservedName(Box<str>),

    /// A model-visible name does not match the Responses tool-name grammar.
    #[error(
        "invalid public tool name `{0}`; expected 1-128 ASCII bytes beginning with an alphanumeric and containing only alphanumerics, `.`, `_`, `:`, or `-`"
    )]
    InvalidPublicName(Box<str>),

    /// Two Code Mode tools map to the same JavaScript identifier.
    #[error("tool names `{first}` and `{second}` both normalize to Code Mode name `{normalized}`")]
    NormalizedNameCollision {
        /// First public tool name using the normalized identifier.
        first: Box<str>,
        /// Later public tool name using the normalized identifier.
        second: Box<str>,
        /// Conflicting JavaScript identifier.
        normalized: Box<str>,
    },
}
