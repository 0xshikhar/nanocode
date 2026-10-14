use std::{ffi::OsString, path::PathBuf, sync::Arc};

use crate::{
    SessionEnvironment, StandardTool, Tool, ToolContext, ToolInput, ToolOutput,
    apply_patch::ApplyPatchHandler,
    shell::{ExecCommandHandler, ShellSessions, WriteStdinHandler},
    view_image::ViewImageHandler,
};

pub struct WorkspaceToolRuntime {
    apply_patch: ApplyPatchHandler,
    exec_command: ExecCommandHandler,
    view_image: ViewImageHandler,
    write_stdin: WriteStdinHandler,
    sessions: Arc<ShellSessions>,
}

#[derive(Clone, Debug)]
pub struct WorkspaceTools {
    pub(crate) root: PathBuf,
}

impl WorkspaceTools {
    #[must_use]
    pub fn new(workspace: impl Into<PathBuf>) -> Self {
        Self {
            root: workspace.into(),
        }
    }

    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }
}

impl WorkspaceToolRuntime {
    #[must_use]
    pub fn new(workspace: PathBuf, session: &SessionEnvironment) -> Self {
        Self::with_optional_view_image_wire_limit(workspace, None, Arc::new(session.os_variables()))
    }

    #[cfg(feature = "attachment")]
    pub(crate) fn unbound(workspace: PathBuf) -> Self {
        Self::with_optional_view_image_wire_limit(workspace, None, Arc::default())
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
            apply_patch: ApplyPatchHandler::new(workspace.clone()),
            exec_command: ExecCommandHandler::new(workspace.clone(), Arc::clone(&sessions)),
            view_image: max_wire_bytes.map_or_else(
                || ViewImageHandler::new(workspace.clone()),
                |max_wire_bytes| {
                    ViewImageHandler::with_wire_limit(workspace.clone(), max_wire_bytes)
                },
            ),
            write_stdin: WriteStdinHandler::new(Arc::clone(&sessions)),
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

    pub(crate) fn supports_parallel_tool_calls(&self, name: &str) -> bool {
        self.handler(name)
            .is_some_and(Tool::supports_parallel_tool_calls)
    }

    fn handler(&self, name: &str) -> Option<&dyn Tool> {
        match name {
            name if name == StandardTool::ApplyPatch.name() => Some(&self.apply_patch),
            name if name == StandardTool::ExecCommand.name() => Some(&self.exec_command),
            name if name == StandardTool::ViewImage.name() => Some(&self.view_image),
            name if name == StandardTool::WriteStdin.name() => Some(&self.write_stdin),
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
