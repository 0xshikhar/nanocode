#![deny(rustdoc::broken_intra_doc_links)]

extern crate self as nanocode_observability;

mod cli;

use std::{fs::OpenOptions, io, path::PathBuf, time::Duration};

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{MetricExporter, Protocol, SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{
    Resource,
    metrics::{PeriodicReader, SdkMeterProvider},
    trace::{BatchSpanProcessor, SdkTracerProvider},
};
use opentelemetry_semantic_conventions::{SCHEMA_URL, attribute::SERVICE_VERSION};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_opentelemetry::MetricsLayer;
use tracing_subscriber::{
    EnvFilter, Layer, fmt::format::FmtSpan, layer::SubscriberExt, util::SubscriberInitExt,
};

pub use cli::ObservabilityOutputArgs;

pub const DEFAULT_FILTER: &str = "warn,nanocode=info,nanocode_backend_codex=info,nanocode_backend_claude=info,nanocodex_claude=info,nanocode_agent=info,nanocode_code_mode=info,nanocode_subagents=info,nanocode_durability=info,nanocode_store_jsonl=info,nanocodex=info";
pub const METRICS_TARGET: &str = "nc_metrics";

const DEPLOYMENT_ENVIRONMENT_NAME: &str = "deployment.environment.name";
const LOCAL_LOG_BUFFERED_LINES_LIMIT: usize = 4_096;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LogFormat {
    Pretty,

    #[default]
    Compact,

    Json,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum LogOutput {
    #[default]
    Stderr,

    File(PathBuf),

    Disabled,
}

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
    shutdown_timeout: Option<Duration>,
}

pub struct ObservabilityGuard {
    tracer_provider: Option<SdkTracerProvider>,
    meter_provider: Option<SdkMeterProvider>,
    shutdown_timeout: Option<Duration>,
    _writer: Option<WorkerGuard>,
}

#[derive(Debug, thiserror::Error)]
pub enum ObservabilityError {
    #[error("invalid tracing filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),

    #[error("failed to open tracing output: {0}")]
    Output(#[from] io::Error),

    #[error("failed to configure OTLP exporter: {0}")]
    Otlp(#[from] opentelemetry_otlp::ExporterBuildError),

    #[error("failed to flush or shut down the OpenTelemetry exporter: {0}")]
    OTelSdk(#[from] opentelemetry_sdk::error::OTelSdkError),

    #[error("a global tracing subscriber is already installed")]
    Subscriber,
}

impl ObservabilityBuilder {
    #[must_use]
    pub fn new(service_name: impl Into<String>, service_version: impl Into<String>) -> Self {
        Self {
            otel_filter: format!("{DEFAULT_FILTER},{METRICS_TARGET}=info"),
            filter: DEFAULT_FILTER.to_owned(),
            format: LogFormat::Compact,
            output: LogOutput::Stderr,
            service_name: service_name.into(),
            service_version: service_version.into(),
            environment: None,
            otlp_endpoint: None,
            shutdown_timeout: None,
        }
    }

    #[must_use]
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    #[must_use]
    pub fn otel_filter(mut self, filter: impl Into<String>) -> Self {
        self.otel_filter = filter.into();
        self
    }

    #[must_use]
    pub const fn format(mut self, format: LogFormat) -> Self {
        self.format = format;
        self
    }

    #[must_use]
    pub fn output(mut self, output: LogOutput) -> Self {
        self.output = output;
        self
    }

    #[must_use]
    pub fn environment(mut self, environment: impl Into<String>) -> Self {
        self.environment = Some(environment.into());
        self
    }

    #[must_use]
    pub fn otlp_endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.otlp_endpoint = Some(endpoint.into().trim_end_matches('/').to_owned());
        self
    }

    #[must_use]
    pub const fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = Some(timeout);
        self
    }

    pub fn install(self) -> Result<ObservabilityGuard, ObservabilityError> {
        if rustls::crypto::CryptoProvider::get_default().is_none() {
            drop(rustls::crypto::ring::default_provider().install_default());
        }
        let (fmt_layer, writer_guard) = match self.writer()? {
            Some(output) => {
                let (writer, guard) = NonBlockingBuilder::default()
                    .buffered_lines_limit(LOCAL_LOG_BUFFERED_LINES_LIMIT)
                    .lossy(false)
                    .finish(output);
                let filter = EnvFilter::try_new(self.filter.as_str())?;
                let layer = match self.format {
                    LogFormat::Pretty => tracing_subscriber::fmt::layer()
                        .pretty()
                        .with_writer(writer)
                        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE)
                        .with_filter(filter)
                        .boxed(),
                    LogFormat::Compact => tracing_subscriber::fmt::layer()
                        .compact()
                        .with_ansi(false)
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
                (Some(layer), Some(guard))
            }
            None => (None, None),
        };
        let (tracer_provider, meter_provider) = match self.otlp_endpoint.as_deref() {
            Some(endpoint) => {
                let resource = self.resource();
                (
                    Some(tracer_provider(endpoint, resource.clone())?),
                    Some(meter_provider(endpoint, resource)?),
                )
            }
            None => (None, None),
        };
        let tracing_layer = match &tracer_provider {
            Some(provider) => Some(
                tracing_opentelemetry::layer()
                    .with_tracer(provider.tracer(self.service_name.clone()))
                    .with_filter(EnvFilter::try_new(self.otel_filter.as_str())?),
            ),
            None => None,
        };
        let metrics_layer = match &meter_provider {
            Some(provider) => Some(
                MetricsLayer::new(provider.clone())
                    .with_filter(EnvFilter::try_new(self.otel_filter.as_str())?),
            ),
            None => None,
        };
        tracing_subscriber::registry()
            .with(fmt_layer)
            .with(tracing_layer)
            .with(metrics_layer)
            .try_init()
            .map_err(|_| ObservabilityError::Subscriber)?;
        tracing::callsite::rebuild_interest_cache();
        Ok(ObservabilityGuard {
            tracer_provider,
            meter_provider,
            shutdown_timeout: self.shutdown_timeout,
            _writer: writer_guard,
        })
    }

    fn writer(&self) -> Result<Option<Box<dyn io::Write + Send>>, io::Error> {
        match &self.output {
            LogOutput::Stderr => Ok(Some(Box::new(io::stderr()))),
            LogOutput::File(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                Ok(Some(Box::new(
                    OpenOptions::new().create(true).append(true).open(path)?,
                )))
            }
            LogOutput::Disabled => Ok(None),
        }
    }

    fn resource(&self) -> Resource {
        let builder = Resource::builder();
        let builder = if std::env::var_os("OTEL_SERVICE_NAME").is_some() {
            builder
        } else {
            builder.with_service_name(self.service_name.clone())
        };
        builder
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

fn tracer_provider(
    endpoint: &str,
    resource: Resource,
) -> Result<SdkTracerProvider, ObservabilityError> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(signal_endpoint(endpoint, "traces"))
        .with_protocol(Protocol::HttpBinary)
        .with_http_client(reqwest::blocking::Client::new())
        .build()?;
    Ok(SdkTracerProvider::builder()
        .with_resource(resource)
        .with_span_processor(BatchSpanProcessor::builder(exporter).build())
        .build())
}

fn meter_provider(
    endpoint: &str,
    resource: Resource,
) -> Result<SdkMeterProvider, ObservabilityError> {
    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(signal_endpoint(endpoint, "metrics"))
        .with_protocol(Protocol::HttpBinary)
        .with_http_client(reqwest::blocking::Client::new())
        .build()?;
    Ok(SdkMeterProvider::builder()
        .with_resource(resource)
        .with_reader(PeriodicReader::builder(exporter).build())
        .build())
}

fn signal_endpoint(endpoint: &str, signal: &str) -> String {
    let base = endpoint
        .strip_suffix("/v1/traces")
        .or_else(|| endpoint.strip_suffix("/v1/metrics"))
        .unwrap_or(endpoint);
    format!("{base}/v1/{signal}")
}

impl ObservabilityGuard {
    pub fn shutdown(&mut self) -> Result<(), ObservabilityError> {
        let timeout = self.shutdown_timeout;
        let tracer = self.tracer_provider.take();
        let meter = self.meter_provider.take();
        let (tracer, meter) = std::thread::scope(|scope| {
            let tracer = scope.spawn(move || {
                tracer.map(|provider| match timeout {
                    Some(timeout) => provider.shutdown_with_timeout(timeout),
                    None => provider.shutdown(),
                })
            });
            let meter = meter.map(|provider| match timeout {
                Some(timeout) => provider.shutdown_with_timeout(timeout),
                None => provider.shutdown(),
            });
            (tracer.join(), meter)
        });
        if let Ok(tracer) = tracer {
            tracer.transpose()?;
        }
        meter.transpose()?;
        Ok(())
    }
}

impl Drop for ObservabilityGuard {
    fn drop(&mut self) {
        drop(self.shutdown());
    }
}
