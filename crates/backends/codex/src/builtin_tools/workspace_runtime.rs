use std::{ffi::OsString, path::PathBuf, sync::Arc};

use nanocode_code_mode::embedded::HostFuture;
use nanocode_home::SessionEnvironment;
use nanocode_tool_runtime::shell::{MAX_EXEC_YIELD_MS, MAX_POLL_YIELD_MS, ShellSessions};
use nanocode_tool_runtime::workspace_runtime::{WorkspaceExecutor, WorkspaceToolFactory};

use super::{
    StandardTool, Tool, ToolContext, ToolInput, ToolOutput,
    apply_patch::ApplyPatchHandler,
    shell::{ExecCommandHandler, WriteStdinHandler},
    view_image::ViewImageHandler,
};

pub struct WorkspaceToolRuntime {
    apply_patch: Arc<ApplyPatchHandler>,
    exec_command: Arc<ExecCommandHandler>,
    view_image: Arc<ViewImageHandler>,
    write_stdin: Arc<WriteStdinHandler>,
    sessions: Arc<ShellSessions>,
}

#[derive(Clone)]
pub struct WorkspaceTools {
    pub(crate) source: nanocode_tool_runtime::WorkspaceTools,
}

impl std::fmt::Debug for WorkspaceTools {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkspaceTools")
            .field("root", &self.root())
            .finish()
    }
}

impl WorkspaceTools {
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            source: nanocode_tool_runtime::WorkspaceTools::with_factory(
                workspace,
                CodexWorkspaceFactory,
            ),
        }
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        self.source.root()
    }
}

impl nanocode_tool_runtime::ToolSource for WorkspaceTools {
    fn install(
        self,
        builder: nanocode_tool_runtime::ToolsBuilder,
    ) -> nanocode_tool_runtime::ToolsBuilder {
        builder.add(self.source)
    }
}

struct CodexWorkspaceFactory;

impl WorkspaceToolFactory for CodexWorkspaceFactory {
    fn create(
        &self,
        root: PathBuf,
        environment: Vec<(OsString, OsString)>,
        max_wire_bytes: Option<u64>,
    ) -> Arc<dyn WorkspaceExecutor> {
        Arc::new(WorkspaceToolRuntime::with_optional_view_image_wire_limit(
            root,
            max_wire_bytes,
            Arc::new(environment),
        ))
    }
}

impl WorkspaceExecutor for WorkspaceToolRuntime {
    fn tools(&self) -> Vec<Arc<dyn Tool>> {
        vec![
            self.apply_patch.clone(),
            self.exec_command.clone(),
            self.view_image.clone(),
            self.write_stdin.clone(),
        ]
    }

    fn timeout_ms(&self, name: &str) -> Option<u64> {
        match name {
            name if name == StandardTool::ExecCommand.name() => Some(MAX_EXEC_YIELD_MS + 10_000),
            name if name == StandardTool::WriteStdin.name() => Some(MAX_POLL_YIELD_MS + 10_000),
            _ => None,
        }
    }

    fn cancel<'a>(&'a self) -> HostFuture<'a, ()> {
        Box::pin(self.sessions.terminate_all())
    }
}

impl WorkspaceToolRuntime {
    #[must_use]
    pub fn new(workspace: PathBuf, session: &SessionEnvironment) -> Self {
        Self::with_optional_view_image_wire_limit(workspace, None, Arc::new(session.os_variables()))
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_view_image_wire_limit(workspace: PathBuf, max_wire_bytes: u64) -> Self {
        Self::with_optional_view_image_wire_limit(
            workspace,
            Some(max_wire_bytes),
            Arc::<Vec<(OsString, OsString)>>::default(),
        )
    }

    #[doc(hidden)]
    #[must_use]
    pub fn with_environment_and_view_image_wire_limit(
        workspace: PathBuf,
        max_wire_bytes: u64,
        environment: Vec<(OsString, OsString)>,
    ) -> Self {
        Self::with_optional_view_image_wire_limit(
            workspace,
            Some(max_wire_bytes),
            Arc::new(environment),
        )
    }

    fn with_optional_view_image_wire_limit(
        workspace: PathBuf,
        max_wire_bytes: Option<u64>,
        environment: Arc<Vec<(OsString, OsString)>>,
    ) -> Self {
        let sessions = Arc::new(ShellSessions::with_environment(environment));
        Self {
            apply_patch: Arc::new(ApplyPatchHandler::new(workspace.clone())),
            exec_command: Arc::new(ExecCommandHandler::new(
                workspace.clone(),
                Arc::clone(&sessions),
            )),
            view_image: Arc::new(max_wire_bytes.map_or_else(
                || ViewImageHandler::new(workspace.clone()),
                |max_wire_bytes| {
                    ViewImageHandler::with_wire_limit(workspace.clone(), max_wire_bytes)
                },
            )),
            write_stdin: Arc::new(WriteStdinHandler::new(Arc::clone(&sessions))),
            sessions,
        }
    }

    pub async fn execute_tool(
        &self,
        name: &str,
        input: ToolInput,
        context: ToolContext<'_>,
    ) -> ToolOutput {
        let Some(handler) = self.handler(name) else {
            return ToolOutput::error(format!("unknown workspace tool `{name}`"));
        };
        let result = handler.execute(input, context).await;
        result.unwrap_or_else(|error| ToolOutput::error(error.to_string()))
    }

    fn handler(&self, name: &str) -> Option<&dyn Tool> {
        match name {
            name if name == StandardTool::ApplyPatch.name() => Some(self.apply_patch.as_ref()),
            name if name == StandardTool::ExecCommand.name() => Some(self.exec_command.as_ref()),
            name if name == StandardTool::ViewImage.name() => Some(self.view_image.as_ref()),
            name if name == StandardTool::WriteStdin.name() => Some(self.write_stdin.as_ref()),
            _ => None,
        }
    }

    #[must_use]
    pub fn control(&self) -> WorkspaceToolRuntimeControl {
        WorkspaceToolRuntimeControl {
            sessions: Arc::clone(&self.sessions),
        }
    }
}

pub struct WorkspaceToolRuntimeControl {
    sessions: Arc<ShellSessions>,
}

impl WorkspaceToolRuntimeControl {
    pub async fn cancel(&self) {
        self.sessions.terminate_all().await;
    }
}
