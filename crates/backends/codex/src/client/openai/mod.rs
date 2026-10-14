use std::{num::NonZeroU32, sync::Arc};

use ::tower::Layer;

mod config;
mod platform;

use crate::client::{
    DefaultResponsesService, Model, OpenAiAuth, OpenAiAuthError, OpenAiAuthMode, ReasoningMode,
    ResponsesHistory, ResponsesRetryPolicy, ResponsesTransport, Thinking, pricing::ServiceTier,
    responses::StrictJsonSchema, session::SessionBuilder,
};

#[doc(hidden)]
pub use config::ModelConfig;
pub use config::ModelConfig as ResponsesServiceConfig;

#[derive(Clone)]
pub struct OpenAi<F = StandardServiceFactory> {
    config: ModelConfig,
    factory: F,
}

impl OpenAi<StandardServiceFactory> {
    pub fn new(auth: impl Into<OpenAiAuth>) -> Result<Self, OpenAiError> {
        Self::builder(auth).build()
    }

    #[must_use]
    pub fn builder(auth: impl Into<OpenAiAuth>) -> OpenAiBuilder<StandardServiceFactory> {
        let auth = auth.into();
        let mode = auth.mode();
        let mut config = ModelConfig {
            auth,
            ..ModelConfig::default()
        };
        apply_mode_defaults(&mut config, mode);
        OpenAiBuilder {
            config,
            factory: StandardServiceFactory::default(),
        }
    }
}

impl<F> OpenAi<F> {
    #[must_use]
    pub const fn config(&self) -> &ModelConfig {
        &self.config
    }
}

impl<F> OpenAi<F>
where
    F: ResponsesServiceFactory,
{
    #[must_use]
    pub const fn auth_mode(&self) -> OpenAiAuthMode {
        self.config.auth.mode()
    }

    #[must_use]
    pub fn instructions(&self, instructions: impl Into<Arc<str>>) -> SessionBuilder<F> {
        SessionBuilder::new(self.clone(), instructions.into())
    }

    pub(crate) fn make_service(&self) -> F::Service {
        self.factory.make(Arc::new(self.config.clone()))
    }

    pub(crate) fn into_parts(self) -> (ModelConfig, F) {
        (self.config, self.factory)
    }
}

#[derive(Clone)]
pub struct OpenAiBuilder<F = StandardServiceFactory> {
    config: ModelConfig,
    factory: F,
}

impl<F> OpenAiBuilder<F> {
    #[must_use]
    pub const fn model(mut self, model: Model) -> Self {
        self.config.model = model;

        if matches!(model, Model::Glm53 | Model::Kimi | Model::Mimo) {
            self.config.responses_transport = ResponsesTransport::Https;
            self.config.responses_history = ResponsesHistory::FullReplay;
            self.config.store_responses = false;
            self.config.websocket_warmup = false;
        }
        if !self.config.thinking_explicit {
            self.config.thinking = model.default_thinking();
        }
        if self.config.context_window_tokens > model.max_context_window_tokens() {
            self.config.context_window_tokens = model.max_context_window_tokens();
        }
        self
    }

    #[must_use]
    pub fn model_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.config.model_id_prefix = Some(Arc::from(prefix.trim()));
        self
    }

    #[must_use]
    pub const fn transport(mut self, transport: ResponsesTransport) -> Self {
        self.config.responses_transport = transport;
        if matches!(transport, ResponsesTransport::Https) && !self.config.store_responses {
            self.config.responses_history = ResponsesHistory::FullReplay;
        }
        self
    }

    #[must_use]
    pub const fn websocket_warmup(mut self, enabled: bool) -> Self {
        self.config.websocket_warmup = enabled;
        self
    }

    #[must_use]
    pub const fn raw_api_events(mut self, enabled: bool) -> Self {
        self.config.raw_api_events = enabled;
        self
    }

    #[must_use]
    pub const fn history(mut self, history: ResponsesHistory) -> Self {
        self.config.responses_history = history;
        self
    }

    #[must_use]
    pub const fn store(mut self, store: bool) -> Self {
        self.config.store_responses = store;
        if matches!(self.config.responses_transport, ResponsesTransport::Https) {
            self.config.responses_history = if store {
                ResponsesHistory::Incremental
            } else {
                ResponsesHistory::FullReplay
            };
        }
        self
    }

    #[must_use]
    pub fn strict_json_schema(mut self, schema: StrictJsonSchema) -> Self {
        self.config.strict_json_schema = Some(schema);
        self
    }

    #[must_use]
    pub const fn thinking(mut self, thinking: Thinking) -> Self {
        self.config.thinking = thinking;
        self.config.thinking_explicit = true;
        self
    }

    #[must_use]
    pub const fn reasoning_mode(mut self, reasoning_mode: ReasoningMode) -> Self {
        self.config.reasoning_mode = reasoning_mode;
        self
    }

    #[must_use]
    pub const fn fast_mode(self, enabled: bool) -> Self {
        self.service_tier(ServiceTier::from_fast_mode(enabled))
    }

    #[must_use]
    pub const fn service_tier(mut self, service_tier: ServiceTier) -> Self {
        self.config.service_tier = service_tier;
        self
    }

    #[must_use]
    pub const fn context_window_tokens(mut self, tokens: u64) -> Self {
        let maximum = self.config.model.max_context_window_tokens();
        self.config.context_window_tokens = if tokens > maximum { maximum } else { tokens };
        self
    }

    #[must_use]
    pub fn websocket_url(mut self, url: impl Into<String>) -> Self {
        self.config.websocket_url = url.into();
        self
    }

    #[must_use]
    pub fn api_base_url(mut self, url: impl Into<String>) -> Self {
        self.config.api_base_url = url.into();
        self
    }

    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn chat_gateway(mut self, gateway: crate::client::transport::ChatGateway) -> Self {
        self.config.api_base_url = gateway.default_api_base_url().to_owned();
        self.config.chat_gateway = Some(Arc::new(gateway));
        self.config.responses_transport = ResponsesTransport::Https;
        self.config.responses_history = ResponsesHistory::FullReplay;
        self.config.store_responses = false;
        self.config.websocket_warmup = false;
        self
    }

    #[cfg(any(target_family = "wasm", docsrs))]
    #[must_use]
    pub fn host_transport(
        mut self,
        transport: impl crate::client::transport::host::HostTransport,
    ) -> Self {
        self.config.host_transport = Some(Arc::new(transport));
        self
    }

    #[must_use]
    pub fn layer<L>(self, layer: L) -> OpenAiBuilder<LayeredServiceFactory<F, L>> {
        OpenAiBuilder {
            config: self.config,
            factory: LayeredServiceFactory {
                inner: self.factory,
                layer,
            },
        }
    }

    #[must_use]
    pub fn service<M>(self, make: M) -> OpenAiBuilder<CallerServiceFactory<M>> {
        OpenAiBuilder {
            config: self.config,
            factory: CallerServiceFactory {
                make: Arc::new(make),
            },
        }
    }
}

impl OpenAiBuilder<StandardServiceFactory> {
    #[must_use]
    pub const fn max_attempts(mut self, max_attempts: NonZeroU32) -> Self {
        self.factory.max_attempts = max_attempts;
        self
    }

    #[cfg(not(target_family = "wasm"))]
    #[must_use]
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.factory.platform.set_http_client(client);
        self
    }
}

impl<F> OpenAiBuilder<F>
where
    F: ResponsesServiceFactory,
{
    pub fn build(self) -> Result<OpenAi<F>, OpenAiError> {
        #[cfg(not(target_family = "wasm"))]
        crate::client::transport::install_default_rustls_crypto_provider();
        validate(&self.config)?;
        self.factory.validate_config(&self.config)?;
        Ok(OpenAi {
            config: self.config,
            factory: self.factory,
        })
    }
}

#[derive(Clone)]
pub struct StandardServiceFactory {
    max_attempts: NonZeroU32,
    platform: platform::FactoryPlatform,
}

impl Default for StandardServiceFactory {
    fn default() -> Self {
        #[cfg(not(target_family = "wasm"))]
        crate::client::transport::install_default_rustls_crypto_provider();
        Self {
            max_attempts: ResponsesRetryPolicy::DEFAULT_MAX_ATTEMPTS,
            platform: platform::FactoryPlatform::new(),
        }
    }
}

pub struct CallerServiceFactory<M> {
    make: Arc<M>,
}

impl<M> Clone for CallerServiceFactory<M> {
    fn clone(&self) -> Self {
        Self {
            make: Arc::clone(&self.make),
        }
    }
}

#[derive(Clone)]
pub struct LayeredServiceFactory<F, L> {
    inner: F,
    layer: L,
}

pub trait ResponsesServiceFactory: Clone {
    type Service;

    fn validate_config(&self, _config: &ResponsesServiceConfig) -> Result<(), OpenAiError> {
        Ok(())
    }

    fn make(&self, config: Arc<ResponsesServiceConfig>) -> Self::Service;
}

impl ResponsesServiceFactory for StandardServiceFactory {
    type Service = DefaultResponsesService;

    fn validate_config(&self, config: &ModelConfig) -> Result<(), OpenAiError> {
        self.platform.validate_config(config)
    }

    fn make(&self, config: Arc<ModelConfig>) -> Self::Service {
        self.platform.make(config, self.max_attempts)
    }
}

impl<M, S> ResponsesServiceFactory for CallerServiceFactory<M>
where
    M: Fn() -> S,
{
    type Service = S;

    fn make(&self, _config: Arc<ModelConfig>) -> Self::Service {
        (self.make)()
    }
}

impl<F, L> ResponsesServiceFactory for LayeredServiceFactory<F, L>
where
    F: ResponsesServiceFactory,
    L: Layer<F::Service> + Clone,
{
    type Service = L::Service;

    fn validate_config(&self, config: &ModelConfig) -> Result<(), OpenAiError> {
        self.inner.validate_config(config)
    }

    fn make(&self, config: Arc<ModelConfig>) -> Self::Service {
        self.layer.layer(self.inner.make(config))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenAiError {
    #[error(transparent)]
    Authorization(#[from] OpenAiAuthError),

    #[error("invalid OpenAI client configuration: {detail}")]
    InvalidConfiguration { detail: &'static str },
}

fn apply_mode_defaults(config: &mut ModelConfig, mode: OpenAiAuthMode) {
    config.store_responses = false;
    config.responses_history = ResponsesHistory::Incremental;
    mode.default_websocket_url()
        .clone_into(&mut config.websocket_url);
    mode.default_api_base_url()
        .clone_into(&mut config.api_base_url);
}

fn validate(config: &ModelConfig) -> Result<(), OpenAiError> {
    config.auth.validate()?;
    if !config.model.supports_thinking(config.thinking) {
        return Err(OpenAiError::InvalidConfiguration {
            detail: (if config.model == Model::Glm53 {
                "GLM-5.3 requires low, medium, or high reasoning effort"
            } else if config.model == Model::Sol {
                "GPT-6.1 Sol requires low, medium, high, xhigh, or max reasoning effort"
            } else {
                "GPT-6 Astra requires low, medium, high, xhigh, or max reasoning effort"
            }),
        });
    }
    if !config.model.supports_reasoning_mode(config.reasoning_mode) {
        return Err(OpenAiError::InvalidConfiguration {
            detail: (if config.model == Model::Glm53 {
                "GLM-5.3 does not support pro reasoning mode"
            } else {
                "the selected gateway model does not support pro reasoning mode"
            }),
        });
    }
    if config.context_window_tokens == 0 {
        return Err(OpenAiError::InvalidConfiguration {
            detail: "the model context window must be greater than zero",
        });
    }
    if config.websocket_url.trim().is_empty() {
        return Err(OpenAiError::InvalidConfiguration {
            detail: "the Responses WebSocket URL must not be empty",
        });
    }
    if config.api_base_url.trim().is_empty() {
        return Err(OpenAiError::InvalidConfiguration {
            detail: "the OpenAI API base URL must not be empty",
        });
    }
    if let Some(prefix) = config.model_id_prefix.as_deref() {
        if prefix.is_empty()
            || prefix.starts_with('/')
            || prefix.ends_with('/')
            || prefix.split('/').any(str::is_empty)
            || prefix
                .chars()
                .any(|character| character.is_whitespace() || character.is_control())
        {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "the model ID prefix must contain non-empty path segments without whitespace",
            });
        }
        if config.auth.mode() != OpenAiAuthMode::ApiKey {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "model ID prefixes require API-key authentication",
            });
        }
        if !matches!(config.responses_transport, ResponsesTransport::Https) {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "model ID prefixes require the HTTPS Responses transport",
            });
        }
    }
    #[cfg(not(target_family = "wasm"))]
    if let Some(gateway) = config.chat_gateway.as_deref() {
        if !gateway.api_base_url_is_allowed(&config.api_base_url) {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "the gateway endpoint is fixed; only a loopback test base URL may replace it",
            });
        }
        if !gateway.stall_timeout_is_valid() {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "the gateway stall timeout must be greater than zero",
            });
        }
        if !gateway.wire_model_is_valid() {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "the gateway model ID must be an OpenRouter vendor/model slug",
            });
        }
        if config.auth.mode() != OpenAiAuthMode::ApiKey {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "Chat Completions gateways require API-key authentication",
            });
        }
        if config.model_id_prefix.is_some() {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "Chat Completions gateways choose their own model IDs; remove the model ID prefix",
            });
        }
        if !matches!(config.responses_transport, ResponsesTransport::Https)
            || config.store_responses
            || !matches!(config.responses_history, ResponsesHistory::FullReplay)
        {
            return Err(OpenAiError::InvalidConfiguration {
                detail: "Chat Completions gateways require stateless HTTPS with full history replay",
            });
        }
    }
    if config.auth.mode() == OpenAiAuthMode::ChatGpt && config.store_responses {
        return Err(OpenAiError::InvalidConfiguration {
            detail: "ChatGPT subscription authentication does not support store: true",
        });
    }
    if matches!(config.responses_transport, ResponsesTransport::Https)
        && !config.store_responses
        && matches!(config.responses_history, ResponsesHistory::Incremental)
    {
        return Err(OpenAiError::InvalidConfiguration {
            detail: "HTTPS with store: false requires full client-history replay",
        });
    }
    Ok(())
}
