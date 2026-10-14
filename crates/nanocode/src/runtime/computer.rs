use nanocode_agent::Result;
use std::sync::Arc;

#[cfg(not(target_family = "wasm"))]
pub(super) struct Cua {
    tools: tokio::sync::OnceCell<Option<Computer>>,
    unavailable: std::sync::OnceLock<String>,
}

#[cfg(not(target_family = "wasm"))]
pub(super) type Cell = Option<Arc<Cua>>;

#[cfg(not(target_family = "wasm"))]
pub(super) type Computer = crate::ComputerTools;

#[cfg(not(target_family = "wasm"))]
pub(super) fn cell(explicit: Option<Computer>) -> Arc<Cua> {
    Arc::new(Cua {
        tools: match explicit {
            Some(computer) => tokio::sync::OnceCell::new_with(Some(Some(computer))),
            None => tokio::sync::OnceCell::new(),
        },
        unavailable: std::sync::OnceLock::new(),
    })
}

#[cfg(not(target_family = "wasm"))]
pub(super) async fn connect(cell: Cell) -> Result<Option<Computer>> {
    let Some(cua) = cell else {
        return Ok(None);
    };
    Ok(cua
        .tools
        .get_or_init(|| async {
            let connected = match crate::ComputerConfig::installed() {
                Ok(config) => crate::ComputerTools::connect(config).await,
                Err(error) => Err(error),
            };
            connected
                .map_err(|error| {
                    let _ = cua.unavailable.set(error.to_string());
                })
                .ok()
        })
        .await
        .clone())
}

#[cfg(not(target_family = "wasm"))]
pub(super) fn unavailable(cell: &Cell) -> Option<String> {
    cell.as_ref()?.unavailable.get().cloned()
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
pub(super) fn unavailable(cell: &Cell) -> Option<String> {
    cell.as_ref().map(|computer| match **computer {})
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
