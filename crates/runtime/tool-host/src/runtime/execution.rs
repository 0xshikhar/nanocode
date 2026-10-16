use super::*;

pub struct ToolRuntime {
    inner: Runtime,
}

enum Runtime {
    Native(NativeToolRuntime),
    Host(crate::embedded::EmbeddedToolRuntime),
}

#[derive(Clone)]
pub struct ToolRuntimeControl {
    inner: RuntimeControl,
}

#[derive(Clone)]
enum RuntimeControl {
    Native(NativeToolRuntimeControl),
    Host(crate::embedded::EmbeddedToolRuntimeControl),
}

impl ToolRuntime {
    pub fn new(workspace: impl Into<PathBuf>, builtin_tools: Vec<Arc<dyn Tool>>) -> Self {
        Self {
            inner: Runtime::Native(NativeToolRuntime::new(workspace, builtin_tools)),
        }
    }

    #[must_use]
    pub fn new_with_tools(
        workspace: impl Into<PathBuf>,
        builtin_tools: Vec<Arc<dyn Tool>>,
        tools: &Tools,
    ) -> Self {
        let inner = if tools.embedded_host.is_some() {
            Runtime::Host(crate::embedded::EmbeddedToolRuntime::new_with_tools(
                workspace,
                builtin_tools,
                tools,
            ))
        } else {
            Runtime::Native(NativeToolRuntime::new_with_tools(
                workspace,
                builtin_tools,
                tools,
            ))
        };
        Self { inner }
    }

    #[must_use]
    pub fn control(&self) -> ToolRuntimeControl {
        ToolRuntimeControl {
            inner: match &self.inner {
                Runtime::Native(runtime) => RuntimeControl::Native(runtime.control()),
                Runtime::Host(runtime) => RuntimeControl::Host(runtime.control()),
            },
        }
    }

    pub fn default_shell_name(&self) -> &str {
        match &self.inner {
            Runtime::Native(runtime) => runtime.default_shell_name(),
            Runtime::Host(runtime) => runtime.default_shell_name(),
        }
    }

    pub fn working_directory(&self) -> &str {
        match &self.inner {
            Runtime::Native(runtime) => runtime.working_directory(),
            Runtime::Host(runtime) => runtime.working_directory(),
        }
    }

    pub fn is_code_only(&self) -> bool {
        match &self.inner {
            Runtime::Native(runtime) => runtime.is_code_only(),
            Runtime::Host(runtime) => runtime.is_code_only(),
        }
    }

    pub fn model_specs(&self, session_id: &str) -> Vec<ToolDefinition> {
        match &self.inner {
            Runtime::Native(runtime) => runtime.model_specs(session_id),
            Runtime::Host(runtime) => runtime.model_specs(session_id),
        }
    }

    pub(crate) fn model_contract(
        &self,
        session_id: &str,
    ) -> (Vec<ToolDefinition>, Vec<(String, String)>) {
        match &self.inner {
            Runtime::Native(runtime) => runtime.model_contract(session_id),
            Runtime::Host(runtime) => runtime.model_contract(session_id),
        }
    }

    #[doc(hidden)]
    pub fn registered_code_mode_definitions(&self) -> Vec<ToolDefinition> {
        match &self.inner {
            Runtime::Native(runtime) => runtime.registry.registered_code_mode_definitions(),
            Runtime::Host(runtime) => runtime.registered_code_mode_definitions(),
        }
    }

    pub fn supports_parallel_tool_calls(&self, name: &str) -> bool {
        match &self.inner {
            Runtime::Native(runtime) => runtime.supports_parallel_tool_calls(name),
            Runtime::Host(runtime) => runtime.supports_parallel_tool_calls(name),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        match &self.inner {
            Runtime::Native(runtime) => runtime.contains(name),
            Runtime::Host(runtime) => runtime.contains(name),
        }
    }

    pub fn execute_code<'call>(
        &'call self,
        source: &'call str,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => runtime.execute_code(source, context).await,
                Runtime::Host(runtime) => runtime.execute_code(source, context).await,
            }
        })
    }

    pub fn execute_code_with_updates<'call>(
        &'call self,
        source: &'call str,
        context: ToolContext<'call>,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => {
                    runtime
                        .execute_code_with_updates(source, context, observer)
                        .await
                }
                Runtime::Host(runtime) => {
                    runtime
                        .execute_code_with_updates(source, context, observer)
                        .await
                }
            }
        })
    }

    pub fn execute_code_owned<'call>(
        &'call self,
        source: &'call str,
        context: OwnedToolContext,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => runtime.execute_code_owned(source, context).await,
                Runtime::Host(runtime) => runtime.execute_code_owned(source, context).await,
            }
        })
    }

    pub fn execute_code_owned_with_updates<'call>(
        &'call self,
        source: &'call str,
        context: OwnedToolContext,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => {
                    runtime
                        .execute_code_owned_with_updates(source, context, observer)
                        .await
                }
                Runtime::Host(runtime) => {
                    runtime
                        .execute_code_owned_with_updates(source, context, observer)
                        .await
                }
            }
        })
    }

    pub fn wait_for_code<'call>(
        &'call self,
        input: &'call str,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => runtime.wait_for_code(input, context).await,
                Runtime::Host(runtime) => runtime.wait_for_code(input, context).await,
            }
        })
    }

    pub fn wait_for_code_with_updates<'call>(
        &'call self,
        input: &'call str,
        context: ToolContext<'call>,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => {
                    runtime
                        .wait_for_code_with_updates(input, context, observer)
                        .await
                }
                Runtime::Host(runtime) => {
                    runtime
                        .wait_for_code_with_updates(input, context, observer)
                        .await
                }
            }
        })
    }

    pub fn execute_tool<'call>(
        &'call self,
        name: &'call str,
        input: ToolInput,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<ToolOutput, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            match &self.inner {
                Runtime::Native(runtime) => runtime.execute_tool(name, input, context).await,
                Runtime::Host(runtime) => runtime.execute_tool(name, input, context).await,
            }
        })
    }
}

impl ToolRuntimeControl {
    pub fn settle_preempt(&self) {
        match &self.inner {
            RuntimeControl::Native(control) => control.settle_preempt(),
            RuntimeControl::Host(control) => control.settle_preempt(),
        }
    }

    pub fn preempt_turn<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.preempt_turn().await,
                RuntimeControl::Host(control) => control.preempt_turn().await,
            }
        })
    }

    pub fn begin_turn(&self) {
        match &self.inner {
            RuntimeControl::Native(control) => control.begin_turn(),
            RuntimeControl::Host(control) => control.begin_turn(),
        }
    }

    pub fn detach_turn_with_updates<'call>(
        &'call self,
        observer: &'call mut (dyn FnMut(&str) -> Box<dyn CodeModeObserver> + Send),
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.detach_turn_with_updates(observer).await,
                RuntimeControl::Host(control) => control.detach_turn_with_updates(observer).await,
            }
        })
    }

    pub fn cancel_turn<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.cancel_turn().await,
                RuntimeControl::Host(control) => control.cancel_turn().await,
            }
        })
    }

    pub fn cancel_turn_with_updates<'call>(
        &'call self,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.cancel_turn_with_updates(observer).await,
                RuntimeControl::Host(control) => control.cancel_turn_with_updates(observer).await,
            }
        })
    }

    pub fn cancel<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.cancel().await,
                RuntimeControl::Host(control) => control.cancel().await,
            }
        })
    }

    pub fn cancel_with_updates<'call>(
        &'call self,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            match &self.inner {
                RuntimeControl::Native(control) => control.cancel_with_updates(observer).await,
                RuntimeControl::Host(control) => control.cancel_with_updates(observer).await,
            }
        })
    }
}

struct NativeToolRuntime {
    pub(super) registry: Arc<ToolRegistry>,
    exposure: Option<ToolExposure>,
    deferred_tools_guidance_enabled: bool,
    code_mode: code_mode::CodeModeRuntime,
    workspace: Option<Arc<dyn crate::workspace_runtime::WorkspaceExecutor>>,
    current_turn: Arc<AtomicU64>,
    default_shell_name: Arc<str>,
    working_directory: Arc<str>,
}

#[doc(hidden)]
#[derive(Clone)]
struct NativeToolRuntimeControl {
    code_mode: code_mode::CodeModeControl,
    workspace: Option<Arc<dyn crate::workspace_runtime::WorkspaceExecutor>>,
    current_turn: Arc<AtomicU64>,
}

impl NativeToolRuntime {
    pub fn new(workspace: impl Into<PathBuf>, builtin_tools: Vec<Arc<dyn Tool>>) -> Self {
        Self::new_inner(workspace.into(), builtin_tools, None)
    }

    pub fn new_with_tools(
        workspace: impl Into<PathBuf>,
        builtin_tools: Vec<Arc<dyn Tool>>,
        tools: &Tools,
    ) -> Self {
        let source = tools
            .workspace_source()
            .filter(|_| tools.workspace_enabled());
        let root = source.map_or_else(|| workspace.into(), |source| source.root().to_owned());
        let executor =
            source.map(|source| source.create(tools.process_environment().as_ref().clone(), None));
        Self::new_inner(root, builtin_tools, executor).with_tools(tools)
    }

    fn new_inner(
        workspace: PathBuf,
        builtin_tools: Vec<Arc<dyn Tool>>,
        executor: Option<Arc<dyn crate::workspace_runtime::WorkspaceExecutor>>,
    ) -> Self {
        let current_turn = Arc::new(AtomicU64::new(0));
        let default_shell_name = Arc::from(nanocode_home::shell::default_shell_name());
        let working_directory = Arc::from(workspace.to_string_lossy().into_owned());
        let mut handlers = executor
            .as_ref()
            .map_or_else(Vec::new, |workspace| workspace.tools());
        handlers.extend(builtin_tools);
        Self {
            registry: Arc::new(ToolRegistry::from_ordered(handlers)),
            exposure: None,
            deferred_tools_guidance_enabled: false,
            code_mode: code_mode::CodeModeRuntime::new_with_turn(Arc::clone(&current_turn)),
            workspace: executor,
            current_turn,
            default_shell_name,
            working_directory,
        }
    }

    fn with_tools(mut self, tools: &Tools) -> Self {
        tools.start_providers();
        let registry = Arc::make_mut(&mut self.registry);
        for policy in tools.code_mode_policies() {
            if !registry
                .policies
                .iter()
                .any(|existing| Arc::ptr_eq(existing, &policy))
            {
                registry.policies.push(policy);
            }
        }
        if self.exposure.is_none() {
            let exposure = tools.exposure();
            self.exposure = Some(exposure);
            registry.set_all_exposures(exposure);
        }
        self.deferred_tools_guidance_enabled |= tools.deferred_tools_guidance_enabled;
        registry.extend(tools.registered.iter().map(|tool| {
            (
                Arc::clone(&tool.handler),
                tool.exposure.unwrap_or_else(|| tools.exposure()),
            )
        }));
        registry.extend(
            tools
                .provider_direct
                .iter()
                .cloned()
                .map(|tool| (tool, tools.exposure())),
        );
        registry.providers.extend(tools.providers.iter().cloned());
        registry.route_workspaces(
            &tools.remote_runners,
            tools.process_route_store.clone(),
            tools.runner_pool.as_ref(),
        );
        if let Some(working_directory) = &tools.working_directory {
            self.working_directory = Arc::clone(working_directory);
        }
        if let Some(default_shell) = &tools.default_shell {
            self.default_shell_name = Arc::clone(default_shell);
        }
        self
    }

    #[must_use]
    pub fn default_shell_name(&self) -> &str {
        &self.default_shell_name
    }

    #[must_use]
    pub fn working_directory(&self) -> &str {
        &self.working_directory
    }

    #[doc(hidden)]
    #[must_use]
    pub fn control(&self) -> NativeToolRuntimeControl {
        NativeToolRuntimeControl {
            code_mode: self.code_mode.control(),
            workspace: self.workspace.clone(),
            current_turn: Arc::clone(&self.current_turn),
        }
    }

    #[must_use]
    pub fn is_code_only(&self) -> bool {
        self.exposure.unwrap_or_default() == ToolExposure::CodeModeOnly
    }

    #[must_use]
    pub fn model_specs(&self, _session_id: &str) -> Vec<ToolDefinition> {
        let mut nested = self.registry.registered_code_mode_definitions();
        let provider_summaries = self.registry.code_mode_tool_summaries();
        let mut direct = self
            .registry
            .direct_definitions()
            .filter(|definition| !matches!(definition, ToolDefinition::ToolSearch { .. }))
            .cloned()
            .collect::<Vec<_>>();
        let code_mode_names = nested
            .iter()
            .map(|definition| definition.name().to_owned())
            .collect::<HashSet<_>>();
        direct = group_direct_code_mode_definitions(direct, &code_mode_names);
        crate::code_mode_order::sort_definitions(&mut nested);
        let mut native = vec![
            code_mode::exec_spec(
                &nested,
                &provider_summaries,
                self.deferred_tools_guidance_enabled,
                self.exposure.unwrap_or_default() == ToolExposure::CodeModeOnly,
            ),
            code_mode::wait_spec(),
        ];
        if self.is_code_only() {
            return native;
        }
        native.append(&mut direct);
        native.extend(
            self.registry
                .definitions()
                .iter()
                .filter(|definition| matches!(definition, ToolDefinition::ToolSearch { .. }))
                .cloned(),
        );
        native
    }

    pub(crate) fn model_contract(
        &self,
        session_id: &str,
    ) -> (Vec<ToolDefinition>, Vec<(String, String)>) {
        (
            self.model_specs(session_id),
            self.registry.code_mode_tool_names(),
        )
    }

    #[must_use]
    pub fn supports_parallel_tool_calls(&self, name: &str) -> bool {
        self.registry.supports_parallel_tool_calls(name)
    }

    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.registry.contains(name)
    }

    pub fn execute_code<'call>(
        &'call self,
        source: &'call str,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            Ok(self
                .code_mode
                .execute(
                    source,
                    self.registry.clone(),
                    OwnedToolContext::from_context(context),
                )
                .await)
        })
    }

    #[doc(hidden)]
    pub fn execute_code_with_updates<'call>(
        &'call self,
        source: &'call str,
        context: ToolContext<'call>,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            Ok(self
                .code_mode
                .execute_with_updates(
                    source,
                    self.registry.clone(),
                    OwnedToolContext::from_context(context),
                    observer,
                )
                .await)
        })
    }

    #[doc(hidden)]
    pub fn execute_code_owned<'call>(
        &'call self,
        source: &'call str,
        context: OwnedToolContext,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            Ok(self
                .code_mode
                .execute(source, self.registry.clone(), context)
                .await)
        })
    }

    #[doc(hidden)]
    pub fn execute_code_owned_with_updates<'call>(
        &'call self,
        source: &'call str,
        context: OwnedToolContext,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move {
            Ok(self
                .code_mode
                .execute_with_updates(source, self.registry.clone(), context, observer)
                .await)
        })
    }

    pub fn wait_for_code<'call>(
        &'call self,
        input: &'call str,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move { Ok(self.code_mode.wait(input, context).await) })
    }

    #[doc(hidden)]
    pub fn wait_for_code_with_updates<'call>(
        &'call self,
        input: &'call str,
        _context: ToolContext<'call>,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<CodeModeExecution, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move { Ok(self.code_mode.wait_with_updates(input, observer).await) })
    }

    pub fn execute_tool<'call>(
        &'call self,
        name: &'call str,
        input: ToolInput,
        context: ToolContext<'call>,
    ) -> futures_util::future::BoxFuture<
        'call,
        Result<ToolOutput, crate::embedded::CodeModeHostError>,
    > {
        Box::pin(async move { Ok(self.registry.execute_direct(name, input, context).await) })
    }
}

fn group_direct_code_mode_definitions(
    definitions: Vec<ToolDefinition>,
    code_mode_names: &HashSet<String>,
) -> Vec<ToolDefinition> {
    let mut grouped = Vec::<ToolDefinition>::new();
    for definition in definitions {
        let canonical_name = definition.name().to_owned();
        let mut definition = if code_mode_names.contains(&canonical_name) {
            crate::code_mode_description::augment_definition_for_code_mode(definition)
        } else {
            definition
        };
        let Some((namespace, name)) = canonical_name.rsplit_once("__") else {
            grouped.push(definition);
            continue;
        };
        if namespace.is_empty() || name.is_empty() {
            grouped.push(definition);
            continue;
        }
        let ToolDefinition::Function {
            name: direct_name, ..
        } = &mut definition
        else {
            grouped.push(definition);
            continue;
        };
        *direct_name = name.into();
        if let Some(ToolDefinition::Namespace { tools, .. }) = grouped.iter_mut().find(
            |group| matches!(group, ToolDefinition::Namespace { name, .. } if &**name == namespace),
        ) {
            tools.push(definition);
        } else {
            grouped.push(ToolDefinition::namespace(
                namespace,
                format!("Tools in the {namespace} namespace."),
                [definition],
            ));
        }
    }
    grouped
}

impl NativeToolRuntimeControl {
    pub fn settle_preempt(&self) {
        self.code_mode.settle_preempt();
    }

    pub fn preempt_turn<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            self.code_mode.preempt_turn();
        })
    }

    #[doc(hidden)]
    pub fn begin_turn(&self) {
        let _ = self
            .current_turn
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |turn| {
                Some(turn.saturating_add(1))
            });
    }

    pub fn detach_turn_with_updates<'call>(
        &'call self,
        observer: &'call mut (dyn FnMut(&str) -> Box<dyn CodeModeObserver> + Send),
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            let turn_id = self.current_turn.load(Ordering::Acquire);
            self.code_mode.detach_turn(turn_id, observer).await;
        })
    }

    #[doc(hidden)]
    pub fn cancel_turn<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            let turn_id = self.current_turn.load(Ordering::Acquire);
            self.code_mode.terminate_turn(turn_id, None).await;
        })
    }

    pub fn cancel_turn_with_updates<'call>(
        &'call self,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            let turn_id = self.current_turn.load(Ordering::Acquire);

            self.code_mode.terminate_turn(turn_id, Some(observer)).await;
        })
    }

    #[doc(hidden)]
    pub fn cancel<'call>(&'call self) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            let _code_mode_quiescence = self.code_mode.terminate_all(None).await;
            if let Some(workspace) = &self.workspace {
                workspace.cancel().await;
            }
        })
    }

    pub fn cancel_with_updates<'call>(
        &'call self,
        observer: &'call mut dyn CodeModeObserver,
    ) -> futures_util::future::BoxFuture<'call, ()> {
        Box::pin(async move {
            let _code_mode_quiescence = self.code_mode.terminate_all(Some(observer)).await;
            if let Some(workspace) = &self.workspace {
                workspace.cancel().await;
            }
        })
    }
}

pub(super) fn record_tool_content(span: &tracing::Span, kind: &'static str, content: &str) {
    span.in_scope(|| {
        info!(
            target: "nanocode_code_mode",
            content_kind = kind,
            content,
            "tool content"
        );
    });
}

pub(super) fn panicked_tool_output(
    span: &tracing::Span,
    payload: Box<dyn Any + Send>,
) -> ToolOutput {
    let message = panic_payload(payload);
    record_tool_content(span, "tool.panic", &message);
    ToolOutput::error("aborted")
}

fn panic_payload(payload: Box<dyn Any + Send>) -> String {
    match payload.downcast::<String>() {
        Ok(message) => *message,
        Err(payload) => payload.downcast::<&'static str>().map_or_else(
            |_| "non-string panic payload".to_owned(),
            |message| (*message).to_owned(),
        ),
    }
}

pub(super) fn tool_execution_span(
    name: &str,
    context: ToolContext<'_>,
    arguments_bytes: usize,
    arguments_kind: &'static str,
    arguments_count: usize,
    argument_keys: &str,
) -> tracing::Span {
    info_span!(
        target: "nanocode_code_mode",
        "tool.execute",
        otel.kind = "internal",
        otel.status_code = tracing::field::Empty,
        tool.name = name,
        session.id = context.session_id(),
        tool.call_id = context.call_id(),
        tool.arguments.bytes = arguments_bytes,
        tool.arguments.kind = arguments_kind,
        tool.arguments.count = arguments_count,
        tool.arguments.keys = argument_keys,
        process.exit.code = tracing::field::Empty,
        process.running = tracing::field::Empty,
        process.wall_time_ms = tracing::field::Empty,
        shell.session.id = tracing::field::Empty,
        tool.output.bytes = tracing::field::Empty,
        tool.output.original_tokens = tracing::field::Empty,
        status = tracing::field::Empty,
        duration_ns = tracing::field::Empty,
    )
}

pub(super) fn finish_tool_execution_span(
    span: &tracing::Span,
    started_at: std::time::Instant,
    execution: &ToolOutput,
    output_content: Option<&str>,
) {
    if let Some(output_content) = output_content {
        record_tool_content(span, "tool.output", output_content);
        span.record("tool.output.bytes", output_content.len());
    }
    span.record(
        "status",
        if execution.success {
            "completed"
        } else {
            "failed"
        },
    );
    span.record(
        "otel.status_code",
        if execution.success { "OK" } else { "ERROR" },
    );
    span.record(
        "duration_ns",
        u64::try_from(started_at.elapsed().as_nanos()).unwrap_or(u64::MAX),
    );
    if let Some(process) = execution.process_trace() {
        if let Some(exit_code) = process.exit_code {
            span.record("process.exit.code", exit_code);
        }
        span.record("process.running", process.session_id.is_some());
        span.record("process.wall_time_ms", process.wall_time_seconds * 1_000.0);
        if let Some(session_id) = process.session_id {
            span.record("shell.session.id", session_id);
        }
        span.record("tool.output.bytes", process.output_bytes);
        if let Some(original_token_count) = process.original_token_count {
            span.record("tool.output.original_tokens", original_token_count);
        }
    }
}
