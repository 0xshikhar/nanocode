use super::{
    RemoteTool, RemoteTools,
    process_routes::{ProcessRoutes, SessionMapping},
};
use crate::{Tool, ToolContext, ToolDefinition, ToolInput, ToolResult};
use async_trait::async_trait;
use serde_json::Value;
use std::{
    path::{Component, Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RemoteMode {
    Alongside,
    Replace,
}

#[derive(Clone)]
pub(crate) struct Route {
    pub tools: RemoteTools,
    pub workspace: PathBuf,
    pub mode: RemoteMode,
}

pub(crate) struct RoutedTool {
    pub local: Arc<dyn Tool>,
    pub routes: Arc<Vec<Route>>,
    pub pool: Option<super::RunnerPool>,
    pub sessions: Arc<ProcessRoutes>,
    pub local_runtime_id: Arc<str>,
}

pub(crate) struct RemoteOnlyTool {
    pub definition: ToolDefinition,
    pub providers: Vec<Arc<dyn crate::DynamicToolProvider>>,
}
#[async_trait]
impl Tool for RemoteOnlyTool {
    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }
    fn supports_parallel_tool_calls(&self) -> bool {
        self.providers
            .iter()
            .find(|provider| provider.contains(self.definition.name()))
            .is_some_and(|provider| provider.supports_parallel_tool_calls(self.definition.name()))
    }
    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult {
        let ToolInput::Function(input) = input else {
            return Err("dynamic provider requires object arguments".into());
        };
        let value: Value = serde_json::from_str(input.get())?;
        for provider in &self.providers {
            if let Some(output) = provider
                .execute(self.definition.name(), value.clone(), context)
                .await
            {
                return Ok(output);
            }
        }
        Err(format!(
            "local workspace does not provide `{}`; select an attached runner workdir",
            self.definition.name()
        )
        .into())
    }
}

fn cua(name: &str) -> bool {
    matches!(name, "mcp__cua_repl__js" | "mcp__cua_repl__reset")
}

fn normalize(path: &Path) -> Result<PathBuf, crate::contract::ToolError> {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                if matches!(result.components().next_back(), Some(Component::Normal(_))) {
                    result.pop();
                } else if path.is_absolute() {
                    return Err("workdir traverses above its root".into());
                } else {
                    result.push("..");
                }
            }
            Component::CurDir => {}
            component => result.push(component.as_os_str()),
        }
    }
    Ok(result)
}

impl Route {
    pub(crate) fn namespace(&self) -> &str {
        self.tools
            .iter()
            .next()
            .expect("validated remote catalog")
            .namespace()
    }
    pub(crate) fn remote(&self, name: &str) -> Option<RemoteTool> {
        self.tools
            .iter()
            .find(|tool| tool.remote_name() == name)
            .cloned()
    }
    pub(crate) fn mount(&self) -> PathBuf {
        PathBuf::from(format!(
            "/{}",
            self.tools
                .iter()
                .next()
                .expect("validated remote catalog")
                .namespace()
        ))
    }
}

impl RoutedTool {
    fn snapshot(&self) -> Vec<Route> {
        let mut routes = self.routes.as_ref().clone();
        if let Some(pool) = &self.pool {
            routes.extend(pool.routes());
        }
        routes
    }

    async fn check_mount(
        &self,
        path: &Path,
        routes: &[Route],
        session_id: &str,
    ) -> Result<(), crate::contract::ToolError> {
        if routes.iter().any(|route| path.starts_with(route.mount())) {
            return Ok(());
        }
        if let Some(name) = self.sessions.known_mount(session_id, path).await? {
            return Err(format!(
                "runner mount `/{name}` is unavailable; attach that runner before using it"
            )
            .into());
        }
        Ok(())
    }
}

#[async_trait]
impl Tool for RoutedTool {
    fn definition(&self) -> ToolDefinition {
        let mut definition = self.local.definition();
        let guidance = if self.pool.is_some() {
            " Workspace routing: omitted workdir uses the configured default host. An attached runner is selected by /<runner-name>/...; paths below that root map to its workspace. Runner availability and mounts are announced in conversation messages. write_stdin uses the returned logical session_id. apply_patch uses absolute paths within a single host.".to_owned()
        } else {
            format!(
                " Workspace routing: default cwd executes {}. Remote mounts: {}. Paths under a mount map to its workspace. write_stdin routes by the returned session_id. apply_patch routes by absolute file paths; do not mix machines in one patch.",
                if self
                    .routes
                    .iter()
                    .any(|route| route.mode == RemoteMode::Replace)
                {
                    "on the replacement runner"
                } else {
                    "locally"
                },
                self.routes
                    .iter()
                    .map(|route| format!(
                        "{} -> {}",
                        route.mount().display(),
                        route.workspace.display()
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        match &mut definition {
            ToolDefinition::Function { description, .. }
            | ToolDefinition::Custom { description, .. } => {
                *description = format!("{description}{guidance}").into();
            }
            _ => {}
        }
        if let ToolDefinition::Function { parameters, .. } = &mut definition {
            let mut schema = parameters.as_value().clone();
            if let Some(object) = schema.as_object_mut() {
                if !matches!(
                    definition_name(&self.local).as_str(),
                    "exec_command" | "write_stdin" | "view_image"
                ) {
                    if let Some(properties) = object
                        .entry("properties")
                        .or_insert_with(|| serde_json::json!({}))
                        .as_object_mut()
                    {
                        properties.insert("workdir".into(), serde_json::json!({"type":"string","description":"Logical workspace root selecting the local host or a remote runner."}));
                    }
                }
            }
            if cua(definition_name(&self.local).as_str()) {
                if let Some(object) = schema.as_object_mut() {
                    let required = object
                        .entry("required")
                        .or_insert_with(|| serde_json::json!([]));
                    if let Some(required) = required.as_array_mut() {
                        if !required.iter().any(|field| field == "workdir") {
                            required.push("workdir".into());
                        }
                    }
                    if definition_name(&self.local) == "mcp__cua_repl__js" {
                        object.insert("required".into(), serde_json::json!(["workdir"]));
                    }
                }
            }
            *parameters = schema.into();
        }
        definition
    }
    fn supports_parallel_tool_calls(&self) -> bool {
        self.pool.is_none()
            && self.local.supports_parallel_tool_calls()
            && self
                .routes
                .iter()
                .filter_map(|route| route.remote(self.local.definition().name()))
                .all(|remote| remote.supports_parallel_tool_calls())
    }
    async fn end_turn(
        &self,
        session_id: &str,
        turn_id: &str,
        hook: &str,
    ) -> Result<(), crate::contract::ToolError> {
        self.local.end_turn(session_id, turn_id, hook).await?;
        let routes = self.snapshot();
        for remote in routes
            .iter()
            .filter_map(|route| route.remote(self.local.definition().name()))
        {
            remote.end_turn(session_id, turn_id, hook).await?;
        }
        Ok(())
    }
    async fn execute(&self, input: ToolInput, context: ToolContext<'_>) -> ToolResult {
        let routes = self.snapshot();
        if let Some(pool) = &self.pool {
            self.sessions
                .remember_mounts(context.session_id(), &pool.reserved_mounts())
                .await?;
            if let Some(name) = pool.replacement() {
                if !routes.iter().any(|route| route.namespace() == name) {
                    return Err(format!("replacement runner `{name}` is unavailable").into());
                }
            }
        }
        let definition = self.local.definition();
        let name = definition.name();
        let mut selected = routes
            .iter()
            .position(|route| route.mode == RemoteMode::Replace);
        let input = match input {
            ToolInput::Function(raw) => {
                let mut value: Value = serde_json::from_str(raw.get())?;
                if cua(name) {
                    let workdir = value
                        .get("workdir")
                        .and_then(Value::as_str)
                        .ok_or("CUA requires an explicit workdir selecting an attached desktop")?;
                    let path = normalize(Path::new(workdir))?;
                    selected = routes
                        .iter()
                        .position(|route| path.starts_with(route.mount()));
                    let index = selected.ok_or_else(|| {
                        format!("selected root `{workdir}` has no attached desktop")
                    })?;
                    let remote = routes[index].remote(name).ok_or_else(|| {
                        format!("selected root `{workdir}` has no desktop providing `{name}`")
                    })?;
                    if name == "mcp__cua_repl__js"
                        && value.as_object().is_some_and(|object| object.len() == 1)
                    {
                        return Ok(crate::ToolOutput::json(&serde_json::json!({"workdir":workdir, "tools":routes[index].tools.iter().filter(|tool| cua(tool.remote_name())).map(|tool| tool.native_definition()).collect::<Vec<_>>() })).with_structured_result(serde_json::json!({"workdir":workdir,"definition":remote.native_definition()})));
                    }
                }
                if name == "write_stdin" {
                    if let Some(session) = value.get("session_id").and_then(Value::as_i64) {
                        let stored = self
                            .sessions
                            .get(context.session_id(), i32::try_from(session)?)
                            .await?;
                        selected = if let Some(namespace) = &stored.namespace {
                            let index = routes
                                .iter()
                                .position(|route| route.namespace() == &**namespace)
                                .ok_or("process runner is no longer attached")?;
                            let current = routes[index]
                                .remote(name)
                                .and_then(|tool| tool.runtime_id());
                            if current != stored.runtime_id {
                                return Err("process session belongs to a restarted runner".into());
                            }
                            Some(index)
                        } else {
                            if stored.runtime_id.as_deref() != Some(&*self.local_runtime_id) {
                                return Err(
                                    "local process session belongs to a previous SDK runtime"
                                        .into(),
                                );
                            }
                            None
                        };
                        value["session_id"] = stored.original.into();
                    }
                } else {
                    let key = if value.get("workdir").is_some() {
                        "workdir"
                    } else if value.get("cwd").is_some() {
                        "cwd"
                    } else if value.get("path").is_some() {
                        "path"
                    } else if value.get("file_path").is_some() {
                        "file_path"
                    } else {
                        "workdir"
                    };
                    if let Some(path) = value.get(key).and_then(Value::as_str) {
                        let path = normalize(Path::new(path))?;
                        self.check_mount(&path, &routes, context.session_id())
                            .await?;
                        for (index, route) in routes.iter().enumerate() {
                            if let Ok(relative) = path.strip_prefix(route.mount()) {
                                selected = Some(index);
                                value[key] = route
                                    .workspace
                                    .join(relative)
                                    .to_string_lossy()
                                    .into_owned()
                                    .into();
                                break;
                            }
                        }
                    }
                }
                if let Some(index) = selected {
                    let route = &routes[index];
                    for key in ["path", "file_path"] {
                        if let Some(path) = value.get(key).and_then(Value::as_str) {
                            let path = normalize(Path::new(path))?;
                            if let Ok(relative) = path.strip_prefix(route.mount()) {
                                value[key] = route
                                    .workspace
                                    .join(relative)
                                    .to_string_lossy()
                                    .into_owned()
                                    .into();
                            }
                        }
                    }
                }
                if !matches!(name, "exec_command" | "write_stdin" | "view_image") {
                    if let Some(object) = value.as_object_mut() {
                        object.remove("workdir");
                    }
                }
                ToolInput::Function(serde_json::value::to_raw_value(&value)?)
            }
            ToolInput::Freeform(mut patch) => {
                let mut machine = None;
                for line in patch.lines() {
                    for prefix in [
                        "*** Add File: ",
                        "*** Update File: ",
                        "*** Delete File: ",
                        "*** Move to: ",
                    ] {
                        if let Some(path) = line.strip_prefix(prefix) {
                            let path = normalize(Path::new(path))?;
                            self.check_mount(&path, &routes, context.session_id())
                                .await?;
                            let route = routes
                                .iter()
                                .position(|route| path.starts_with(route.mount()))
                                .or(selected);
                            if machine.is_some_and(|previous| previous != route) {
                                return Err(
                                    "a patch cannot span local and remote workspaces".into()
                                );
                            }
                            machine = Some(route);
                        }
                    }
                }
                selected = machine.unwrap_or(selected);
                if let Some(index) = selected {
                    let route = &routes[index];
                    patch = patch
                        .lines()
                        .map(|line| {
                            for prefix in [
                                "*** Add File: ",
                                "*** Update File: ",
                                "*** Delete File: ",
                                "*** Move to: ",
                            ] {
                                if let Some(path) = line.strip_prefix(prefix) {
                                    if let Ok(relative) =
                                        normalize(Path::new(path)).and_then(|path| {
                                            Ok(path.strip_prefix(route.mount())?.to_path_buf())
                                        })
                                    {
                                        return format!(
                                            "{prefix}{}",
                                            route.workspace.join(relative).display()
                                        );
                                    }
                                }
                            }
                            line.to_owned()
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                }
                ToolInput::Freeform(patch)
            }
        };
        let (mut output, runtime_id) = if let Some(index) = selected {
            routes[index]
                .remote(name)
                .ok_or_else(|| format!("remote runner does not provide {name}"))?
                .execute_with_runtime(input, context)
                .await?
        } else {
            (self.local.execute(input, context).await?, None)
        };
        if matches!(name, "exec_command" | "write_stdin") {
            {
                let mut result = output.structured_result();
                if let Some(original) = result.get("session_id").and_then(Value::as_i64) {
                    let mapping = SessionMapping {
                        original,
                        namespace: selected.map(|index| routes[index].namespace().into()),
                        runtime_id: if selected.is_some() {
                            runtime_id.clone()
                        } else {
                            Some(self.local_runtime_id.to_string().into())
                        },
                    };
                    let id = self.sessions.insert(context.session_id(), mapping).await?;
                    result["session_id"] = id.into();
                    output = output.with_structured_result(result.clone());
                    output.output = crate::contract::ToolOutputBody::Text(result.to_string());
                }
            }
        }
        Ok(output)
    }
}

fn definition_name(tool: &Arc<dyn Tool>) -> String {
    tool.definition().name().to_owned()
}

pub(crate) struct RunnerProvider {
    pub pool: super::RunnerPool,
    pub routes: Arc<Vec<Route>>,
    pub sessions: Arc<ProcessRoutes>,
    pub local_runtime_id: Arc<str>,
    pub providers: Vec<Arc<dyn crate::DynamicToolProvider>>,
}

#[async_trait]
impl crate::DynamicToolProvider for RunnerProvider {
    fn start(&self) {}
    fn direct_tools(&self) -> Vec<Arc<dyn Tool>> {
        Vec::new()
    }
    fn model_catalog_visible(&self) -> bool {
        false
    }
    fn available_definitions(&self) -> Vec<ToolDefinition> {
        let mut seen = std::collections::HashSet::new();
        self.providers
            .iter()
            .flat_map(|provider| provider.available_definitions())
            .chain(
                self.pool
                    .routes()
                    .iter()
                    .flat_map(|route| route.tools.iter().map(|tool| tool.native_definition()))
                    .collect::<Vec<_>>(),
            )
            .filter(|definition| seen.insert(definition.name().to_owned()))
            .map(|definition| {
                RoutedTool {
                    local: Arc::new(RemoteOnlyTool {
                        definition,
                        providers: self.providers.clone(),
                    }),
                    routes: self.routes.clone(),
                    pool: Some(self.pool.clone()),
                    sessions: self.sessions.clone(),
                    local_runtime_id: self.local_runtime_id.clone(),
                }
                .definition()
            })
            .collect()
    }
    async fn execute(
        &self,
        name: &str,
        input: Value,
        context: ToolContext<'_>,
    ) -> Option<crate::ToolOutput> {
        let definition = self
            .available_definitions()
            .into_iter()
            .find(|definition| definition.name() == name)?;
        let routed = RoutedTool {
            local: Arc::new(RemoteOnlyTool {
                definition,
                providers: self.providers.clone(),
            }),
            routes: self.routes.clone(),
            pool: Some(self.pool.clone()),
            sessions: self.sessions.clone(),
            local_runtime_id: self.local_runtime_id.clone(),
        };
        Some(match serde_json::value::to_raw_value(&input) {
            Ok(raw) => match routed.execute(ToolInput::Function(raw), context).await {
                Ok(output) => output,
                Err(error) => crate::ToolOutput::error(error.to_string()),
            },
            Err(error) => crate::ToolOutput::error(error.to_string()),
        })
    }
}
