use nanocode_agent::{AgentHandle, NanocodexError, Result};
use nanocode_code_mode::{Tool, Tools};
use nanocode_tools::ToolCollection as _;
use std::{path::PathBuf, sync::Arc};

pub fn install_default_tools(
    workspace: PathBuf,
    handle: &AgentHandle,
    selected: Option<Tools>,
    additive: Vec<Arc<dyn Tool>>,
) -> Result<crate::ClaudeTools> {
    install_default_tools_with_process_routes(workspace, handle, selected, additive, None)
}

pub fn install_default_tools_with_process_routes(
    workspace: PathBuf,
    handle: &AgentHandle,
    selected: Option<Tools>,
    additive: Vec<Arc<dyn Tool>>,
    process_routes: Option<Arc<dyn nanocode_code_mode::attachment::ProcessRouteStore>>,
) -> Result<crate::ClaudeTools> {
    install_default_tools_with_runner_pool(
        workspace,
        handle,
        selected,
        additive,
        process_routes,
        None,
    )
}

pub fn install_default_tools_with_runner_pool(
    workspace: PathBuf,
    handle: &AgentHandle,
    selected: Option<Tools>,
    additive: Vec<Arc<dyn Tool>>,
    process_routes: Option<Arc<dyn nanocode_code_mode::attachment::ProcessRouteStore>>,
    runner_pool: Option<nanocode_code_mode::attachment::RunnerPool>,
) -> Result<crate::ClaudeTools> {
    let jobs = Arc::new(crate::claude_bash::Jobs::default());
    let selected = match selected {
        Some(tools) => tools,
        None => {
            let files = crate::tools::ClaudeWorkspaceFiles::new(&workspace).map_err(|error| {
                NanocodexError::InvalidRequest(format!(
                    "Claude file tools require the runtime workspace {}: {error}",
                    workspace.display()
                ))
            })?;
            crate::claude_files::selection(
                &Arc::new(files),
                workspace.clone(),
                handle.session_environment(),
                jobs.clone(),
            )
            .map_err(|error| NanocodexError::InvalidRequest(error.to_string()))?
        }
    };
    let selected = match process_routes {
        Some(store) => selected.with_default_process_route_store(store),
        None => selected,
    };
    let selected = match runner_pool {
        Some(pool) => selected.with_runner_pool(pool),
        None => selected,
    };
    let mut selected = selected
        .into_builder()
        .exposure(nanocode_code_mode::ToolExposure::CodeModeOnly)
        .build()
        .map_err(|error| NanocodexError::InvalidRequest(error.to_string()))?;
    for tool in additive {
        selected = selected.add_shared_tool(tool)?;
    }
    Ok(
        crate::claude_tools::install(&selected, workspace, handle)?.adapter_shutdown(move || {
            let jobs = jobs.clone();
            async move {
                jobs.shutdown().await;
            }
        }),
    )
}
