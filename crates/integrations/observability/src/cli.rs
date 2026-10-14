use std::path::{Path, PathBuf};

use clap::{Args, ValueEnum, builder::NonEmptyStringValueParser};

use crate::{LogFormat, LogOutput, ObservabilityBuilder, ObservabilityError, ObservabilityGuard};

#[derive(Args)]
pub struct ObservabilityOutputArgs {
    #[arg(long, env = "NANOCODEX_LOG_FORMAT", default_value_t, value_enum)]
    log_format: LogFormatArg,

    #[arg(long, env = "NANOCODEX_LOG_FILE")]
    log_file: Option<PathBuf>,

    #[arg(
        long,
        env = "OTEL_EXPORTER_OTLP_ENDPOINT",
        value_parser = NonEmptyStringValueParser::new()
    )]
    otel_endpoint: Option<String>,

    #[arg(
        long,
        env = "OTEL_DEPLOYMENT_ENVIRONMENT",
        default_value = "development",
        value_parser = NonEmptyStringValueParser::new()
    )]
    otel_environment: String,
}

impl ObservabilityOutputArgs {
    #[must_use]
    pub fn log_file(&self) -> Option<&Path> {
        self.log_file.as_deref()
    }

    pub fn install(
        &self,
        service_name: impl Into<String>,
        service_version: impl Into<String>,
        log_filter: impl Into<String>,
        otel_filter: impl Into<String>,
        default_output: LogOutput,
    ) -> Result<ObservabilityGuard, ObservabilityError> {
        let output = self
            .log_file
            .clone()
            .map_or(default_output, LogOutput::File);
        let mut builder = ObservabilityBuilder::new(service_name, service_version)
            .filter(log_filter)
            .otel_filter(otel_filter)
            .format(self.log_format.into())
            .output(output)
            .environment(self.otel_environment.clone());
        if let Some(endpoint) = &self.otel_endpoint {
            builder = builder.otlp_endpoint(endpoint.clone());
        }
        builder.install()
    }
}

#[derive(Clone, Copy, Default, ValueEnum)]
enum LogFormatArg {
    Pretty,
    #[default]
    Compact,
    Json,
}

impl From<LogFormatArg> for LogFormat {
    fn from(format: LogFormatArg) -> Self {
        match format {
            LogFormatArg::Pretty => Self::Pretty,
            LogFormatArg::Compact => Self::Compact,
            LogFormatArg::Json => Self::Json,
        }
    }
}
