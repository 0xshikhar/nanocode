use nanocode_agent::Result;
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
pub(super) type Cell = Option<Arc<tokio::sync::OnceCell<crate::ComputerTools>>>;

#[cfg(not(target_family = "wasm"))]
pub(super) type Computer = crate::ComputerTools;

#[cfg(not(target_family = "wasm"))]
pub(super) async fn connect(cell: Cell) -> Result<Option<Computer>> {
    let Some(cell) = cell else {
        return Ok(None);
    };
    cell.get_or_try_init(|| async {
        crate::ComputerTools::connect(crate::ComputerConfig::installed()?).await
    })
    .await
    .map(|computer| Some(computer.clone()))
    .map_err(|error: nanocode_tools::ToolError| {
        nanocode_agent::NanocodexError::InvalidRequest(error.to_string())
    })
}

#[cfg(not(target_family = "wasm"))]
pub(super) fn shared(computer: Option<&Computer>) -> Vec<Arc<dyn nanocode_tools::Tool>> {
    computer
        .map(|computer| {
            computer
                .tools()
                .map(|tool| Arc::new(tool) as Arc<dyn nanocode_tools::Tool>)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(not(target_family = "wasm"))]
pub(super) fn expose(
    mut builder: nanocode_code_mode::ToolsBuilder,
    computer: Option<&Computer>,
) -> nanocode_code_mode::ToolsBuilder {
    if let Some(computer) = computer {
        for tool in computer.tools() {
            builder = builder.tool(tool);
        }
    }
    builder
}

#[cfg(target_family = "wasm")]
pub(super) type Cell = Option<Arc<Computer>>;

#[cfg(target_family = "wasm")]
pub(super) enum Computer {}

#[cfg(target_family = "wasm")]
pub(super) async fn connect(cell: Cell) -> Result<Option<Computer>> {
    Ok(cell.map(|computer| match *computer {}))
}

#[cfg(target_family = "wasm")]
pub(super) fn shared(computer: Option<&Computer>) -> Vec<Arc<dyn nanocode_tools::Tool>> {
    computer.map_or_else(Vec::new, |computer| match *computer {})
}

#[cfg(target_family = "wasm")]
pub(super) fn expose(
    builder: nanocode_code_mode::ToolsBuilder,
    computer: Option<&Computer>,
) -> nanocode_code_mode::ToolsBuilder {
    computer.map_or(builder, |computer| match *computer {})
}
