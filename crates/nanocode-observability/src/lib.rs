//! Tracing formatting and OpenTelemetry setup for Nanocodex applications.
#![deny(missing_docs, rustdoc::broken_intra_doc_links)]

extern crate self as nanocode_observability;

#[cfg(feature = "cli")]
mod cli;

use std::{fs::OpenOptions, io, path::PathBuf};

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource, runtime,
    trace::{
        SdkTracerProvider,
        span_processor_with_async_runtime::BatchSpanProcessor as TokioBatchSpanProcessor,
    },
};
use opentelemetry_semantic_conventions::{SCHEMA_URL, attribute::SERVICE_VERSION};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_subscriber::{
    EnvFilter, Layer, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt,
};

#[cfg(feature = "cli")]
pub use cli::ObservabilityOutputArgs;

const DEPLOYMENT_ENVIRONMENT_NAME: &str = "deployment.environment.name";
const LOCAL_LOG_BUFFERED_LINES_LIMIT: usize = 4_096;

/// Human-readable or structured local tracing output.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogFormat {
    /// Multiline human-readable records.
    Pretty,
    /// Dense human-readable records.
    #[default]
    Compact,
    /// Structured JSON records.
    Json,
}

/// Destination for the local formatting layer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LogOutput {
    /// Write local records to standard error.
    #[default]
    Stderr,
    /// Append local records to a file, creating its parent directories.
    File(PathBuf),
}

/// Builder for local formatting and optional OTLP/HTTP trace export.
#[derive(Clone, Debug)]
pub struct ObservabilityBuilder {
    filter: String,
    otel_filter: String,
    format: LogFormat,
    output: LogOutput,
    service_name: String,
    service_version: String,
    environment: Option<String>,
    otlp_endpoint: Option<String>,
}

/// Keeps asynchronous formatting and OTLP providers alive and flushes them on drop.
pub struct ObservabilityGuard {
    tracer_provider: Option<SdkTracerProvider>,
    _writer: WorkerGuard,
}

/// Failure to configure, install, or flush observability.
#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    /// A local or OpenTelemetry filter is invalid.
    #[error("invalid tracing filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    /// The configured local output could not be opened.
    #[error("failed to open tracing output: {0}")]
    Output(#[from] io::Error),
    /// The OTLP exporter could not be constructed.
    #[error("failed to configure OTLP exporter: {0}")]
    Otlp(#[from] opentelemetry_otlp::ExporterBuildError),
    /// The OpenTelemetry SDK could not flush or shut down.
    #[error("failed to flush or shut down the OpenTelemetry exporter: {0}")]
    OTelSdk(#[from] opentelemetry_sdk::error::OTelSdkError),
    /// Another process-global tracing subscriber is already installed.
    #[error("a global tracing subscriber is already installed")]
    Subscriber,
}

impl ObservabilityBuilder {
    /// Starts a builder with the service identity attached to exported spans.
    #[must_use]
    pub fn new(service_name: impl Into<String>, service_version: impl Into<String>) -> Self {
        let filter = "warn,nanocode=info,nanocode_backend_codex=info,nanocode_backend_claude=info,nanocode_agent=info,nanocode_code_mode=info".to_owned();
        Self {
            otel_filter: filter.clone(),
            filter,
            format: LogFormat::Compact,
            output: LogOutput::Stderr,
            service_name: service_name.into(),
            service_version: service_version.into(),
            environment: None,
            otlp_endpoint: None,
        }
    }

    /// Sets the filter applied to local formatted records.
    #[must_use]
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    /// Sets the independent filter applied only to exported OpenTelemetry spans.
    #[must_use]
    pub fn otel_filter(mut self, filter: impl Into<String>) -> Self {
        self.otel_filter = filter.into();
        self
    }

    /// Selects the local record format.
    #[must_use]
    pub const fn format(mut self, format: LogFormat) -> Self {
        self.format = format;
        self
    }

    /// Selects the local record destination.
    #[must_use]
    pub fn output(mut self, output: LogOutput) -> Self {
        self.output = output;
        self
    }

    /// Sets the deployment environment attached to exported spans.
    #[must_use]
    pub fn environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    /// Sets the OTLP/HTTP collector base endpoint.
    ///
    /// The standard `/v1/traces` signal path is appended unless it is already
    /// present, matching `OTEL_EXPORTER_OTLP_ENDPOINT` semantics.
    #[must_use]
    pub fn otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        let endpoint = endpoint.into();
        self.otlp_endpoint = Some(trace_endpoint(&endpoint));
        self
    }

    /// Installs the process-global subscriber.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid filter, unusable output file, invalid
    /// exporter configuration, or an already-installed global subscriber.
    pub fn install(self) -> Result<ObservabilityGuard, ObservabilityError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            drop(rustls::crypto::ring::default_provider().install_default());
        }
        let (writer, writer_guard) = NonBlockingBuilder::default()
            .buffered_lines_limit(LOCAL_LOG_BUFFERED_LINES_LIMIT)
            .lossy(false)
            .finish(self.writer()?);
        let filter = EnvFilter::try_new(self.filter.as_str())?;
        let otel_filter = EnvFilter::try_new(self.otel_filter.as_str())?;
        let fmt_layer = match self.format {
            LogFormat::Pretty => tracing_subscriber::fmt::layer()
                .pretty()
                .with_writer(writer)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_filter(filter)
                .boxed(),
            LogFormat::Compact => tracing_subscriber::fmt::layer()
                .compact()
                .with_writer(writer)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_filter(filter)
                .boxed(),
            LogFormat::Json => tracing_subscriber::fmt::layer()
                .json()
                .with_span_list(true)
                .with_current_span(false)
                .with_writer(writer)
                .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                .with_filter(filter)
                .boxed(),
        };
        let tracer_provider = self.tracer_provider()?;
        let tracing_layer = tracer_provider.as_ref().map(|provider| {
            tracing_opentelemetry::layer()
                .with_tracer(provider.tracer(self.service_name))
                .with_filter(otel_filter)
        });
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(tracing_layer)
            .try_init()
            .map_err(|_| ObservabilityError::Subscriber)?;
        tracing::callsite::rebuild_interest_cache();
        Ok(ObservabilityGuard {
            tracer_provider,
            _writer: writer_guard,
        })
    }

    fn writer(&self) -> Result<Box<dyn io::Write + Send>, io::Error> {
        match &self.output {
            LogOutput::Stderr => Ok(Box::new(io::stderr())),
            LogOutput::File(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Ok(Box::new(
                    OpenOptions::new().create(true).append(true).open(path)?,
                ))
            }
        }
    }

    fn tracer_provider(&self) -> Result<Option<SdkTracerProvider>, ObservabilityError> {
        let Some(endpoint) = self.otlp_endpoint.as_deref() else {
            return Ok(None);
        };
        let resource = self.resource();
        if current_tokio_runtime_is_multi_thread() {
            let exporter = SpanExporter::builder()
                .with_http()
                .with_endpoint(endpoint)
                .with_protocol(Protocol::HttpBinary)
                .with_http_client(reqwest::Client::new())
                .build()?;
            let processor = TokioBatchSpanProcessor::builder(exporter, runtime::Tokio).build();
            return Ok(Some(
                SdkTracerProvider::builder()
                    .with_resource(resource)
                    .with_span_processor(processor)
                    .build(),
            ));
        }

        // The async processor's synchronous flush waits for work scheduled on
        // Tokio. Keep the SDK's dedicated-thread processor when no runtime is
        // active or when a current-thread runtime could deadlock that flush.
        let exporter = SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .with_protocol(Protocol::HttpBinary)
            .with_http_client(reqwest::blocking::Client::new())
            .build()?;
        let processor = opentelemetry_sdk::trace::BatchSpanProcessor::builder(exporter).build();
        Ok(Some(
            SdkTracerProvider::builder()
                .with_resource(resource)
                .with_span_processor(processor)
                .build(),
        ))
    }

    fn resource(&self) -> Resource {
        Resource::builder()
            .with_service_name(self.service_name.clone())
            .with_schema_url(
                [
                    opentelemetry::KeyValue::new(SERVICE_VERSION, self.service_version.clone()),
                    opentelemetry::KeyValue::new(
                        DEPLOYMENT_ENVIRONMENT_NAME,
                        self.environment
                            .clone()
                            .unwrap_or_else(|| "development".to_owned()),
                    ),
                ],
                SCHEMA_URL,
            )
            .build()
    }
}

fn current_tokio_runtime_is_multi_thread() -> bool {
    tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
}

fn trace_endpoint(endpoint: &str) -> String {
    let endpoint = endpoint.trim_end_matches('/');
    if endpoint.ends_with("/v1/traces") {
        endpoint.to_owned()
    } else {
        format!("{endpoint}/v1/traces")
    }
}

impl ObservabilityGuard {
    /// Flushes pending spans and shuts down the exporter. Later calls are no-ops.
    ///
    /// # Errors
    ///
    /// Returns an error when the exporter cannot flush or shut down cleanly.
    pub fn shutdown(&mut self) -> Result<(), ObservabilityError> {
        if let Some(provider) = self.tracer_provider.take() {
            provider.force_flush()?;
            provider.shutdown()?;
        }
        Ok(())
    }
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        drop(self.shutdown());
    }
}
