use super::RetryAfter;

use super::api_error::{
    api_error_has_code, api_error_is_checkpoint_missing, api_error_is_terminal,
    invalid_tool_schema_path, retryable_api_error, retryable_api_error_received,
};

/// Errors produced by the standard `OpenAI` Responses transports.
///
/// The API is identical across native and hosted targets. Platform adapters
/// retain their complete error detail while reducing retry behavior to explicit
/// typed fields before the error reaches the shared state machine.
#[derive(Debug, thiserror::Error)]
pub enum ResponsesError {
    /// Authorization could not be resolved.
    #[error("failed to resolve OpenAI authorization: {detail}")]
    Authorization {
        /// Credential-resolution detail without the credential value.
        detail: String,
    },
    /// The embedding did not install the transport required by this target.
    #[error("the Responses host transport is not configured")]
    HostUnavailable,
    /// The configured WebSocket URL was invalid.
    #[error("invalid Responses WebSocket URL: {detail}")]
    InvalidUrl {
        /// Complete parser failure detail.
        detail: String,
    },
    /// An authorization value could not be encoded for the handshake.
    #[error("invalid OpenAI authorization header: {detail}")]
    InvalidAuthorization {
        /// Complete header encoding failure detail.
        detail: String,
    },
    /// A session identity could not be encoded for the handshake.
    #[error("invalid Responses session identifier header: {detail}")]
    InvalidSessionId {
        /// Complete header encoding failure detail.
        detail: String,
    },
    /// The WebSocket handshake exceeded its deadline.
    #[error("Responses WebSocket handshake exceeded {seconds} seconds")]
    HandshakeTimeout {
        /// Configured timeout in seconds.
        seconds: u64,
    },
    /// The WebSocket handshake failed at the transport layer.
    #[error("Responses WebSocket handshake failed: {detail}")]
    Handshake {
        /// Complete platform failure detail.
        detail: String,
        /// Whether opening a replacement socket may safely recover.
        reconnectable: bool,
    },
    /// The server rejected the WebSocket handshake.
    #[error("Responses WebSocket handshake was rejected with HTTP {status}: {body}")]
    HandshakeRejected {
        /// HTTP response status.
        status: u16,
        /// Retained response body.
        body: String,
        /// Server-requested retry delay when present.
        retry_after: Option<RetryAfter>,
    },
    /// Sending a WebSocket frame failed.
    #[error("failed to send a Responses WebSocket frame: {detail}")]
    Send {
        /// Complete platform failure detail.
        detail: String,
        /// Whether opening a replacement socket may safely recover.
        reconnectable: bool,
    },
    /// Sending a WebSocket frame exceeded its deadline.
    #[error("sending a Responses WebSocket frame exceeded {seconds} seconds")]
    SendTimeout {
        /// Configured timeout in seconds.
        seconds: u64,
    },
    /// The WebSocket stream ended without a close frame.
    #[error("Responses WebSocket closed without a close frame")]
    UnexpectedEnd,
    /// Receiving a WebSocket frame failed.
    #[error("failed to receive a Responses WebSocket frame: {detail}")]
    Receive {
        /// Complete platform failure detail.
        detail: String,
        /// Whether opening a replacement socket may safely recover.
        reconnectable: bool,
    },
    /// A received WebSocket event was not valid JSON.
    #[error("Responses WebSocket event was not valid JSON")]
    InvalidJson(#[source] serde_json::Error),
    /// The endpoint returned a binary frame where text JSON was required.
    #[error("Responses WebSocket returned a binary data frame; expected JSON text")]
    UnexpectedBinary,
    /// A typed request could not be serialized.
    #[error("failed to encode a Responses WebSocket request")]
    EncodeRequest(#[source] serde_json::Error),
    /// An event's payload did not match the shape declared by its type.
    #[error("Responses API event did not match its declared type: {event}")]
    InvalidPayload {
        /// Typed payload decode failure.
        #[source]
        source: serde_json::Error,
        /// Complete retained provider event.
        event: String,
    },
    /// The WebSocket closed with provider-supplied detail.
    #[error("Responses WebSocket closed {detail}")]
    Closed {
        /// Close code and reason.
        detail: String,
    },
    /// The Responses API returned a typed error event.
    #[error("Responses API returned an error event: {event}")]
    Api {
        /// Complete retained provider event.
        event: String,
        /// Server advice captured when the event was received.
        retry_after: Option<RetryAfter>,
    },
    /// The request exceeded the model context window.
    #[error("Responses input exceeded the model context window")]
    ContextWindowExceeded {
        /// Complete retained provider event.
        event: String,
    },
    /// The provider rejected malformed or unsupported image data.
    #[error("Responses API rejected invalid image data: {event}")]
    InvalidImageRequest {
        /// Complete retained provider event.
        event: String,
    },
    /// The provider identified an invalid function schema in discovery history.
    #[error("{source}")]
    InvalidToolSchema {
        /// Original provider failure, including its complete error detail.
        #[source]
        source: Box<Self>,
        /// Exact discovered definition selected by the provider's parameter path.
        definition: Box<serde_json::Value>,
    },
    /// Sending or reading an HTTPS request failed.
    #[error("Responses HTTPS request failed: {detail}")]
    HttpRequest {
        /// Complete platform failure detail.
        detail: String,
        /// Whether replaying the request may recover.
        retryable: bool,
        /// Whether the request exceeded a configured deadline.
        timeout: bool,
    },
    /// The server rejected an HTTPS request.
    #[error("Responses HTTPS request was rejected with HTTP {status}: {body}")]
    HttpRejected {
        /// HTTP response status.
        status: u16,
        /// Retained response body.
        body: String,
        /// Server-requested retry delay when present.
        retry_after: Option<RetryAfter>,
    },
    /// An SSE response body contained invalid UTF-8.
    #[error("Responses HTTPS stream contained invalid UTF-8: {detail}")]
    InvalidSseUtf8 {
        /// Complete UTF-8 failure detail.
        detail: String,
    },
}

const INVALID_IMAGE_ERROR: &str = "The image data you provided does not represent a valid image";

impl ResponsesError {
    /// Returns the SDK-owned retry classification, if retrying is safe.
    #[must_use]
    pub fn retry_advice(&self) -> Option<RetryAdvice> {
        if matches!(self, Self::HttpRejected { body, .. } | Self::HandshakeRejected { body, .. }
            if api_error_is_terminal(body))
        {
            return None;
        }
        let (class, server_delay) = match self {
            Self::Handshake {
                reconnectable: true,
                ..
            } => ("handshake_transport", None),
            Self::HandshakeTimeout { .. } => ("handshake_timeout", None),
            // ChatGPT's edge can transiently reject an otherwise valid upgrade. Treating the
            // rejection as bounded recovery also unlocks the standard HTTPS fallback.
            Self::HandshakeRejected {
                status: 403,
                retry_after,
                ..
            } => ("handshake_forbidden", *retry_after),
            Self::HandshakeRejected {
                status,
                retry_after,
                ..
            } if *status == 429 => ("handshake_rate_limit", *retry_after),
            Self::HandshakeRejected {
                status,
                retry_after,
                ..
            } if (500..=599).contains(status) => ("handshake_server", *retry_after),
            Self::SendTimeout { .. } => ("send_timeout", None),
            Self::Send {
                reconnectable: true,
                ..
            } => ("send_transport", None),
            Self::UnexpectedEnd | Self::Closed { .. } => ("premature_close", None),
            Self::Receive {
                reconnectable: true,
                ..
            } => ("receive_transport", None),
            Self::Api { event, retry_after } => (retryable_api_error(event)?.0, *retry_after),
            // Hosted stream adapters surface parser failures through the same
            // reader rejection as network failures. Replaying a malformed
            // provider stream cannot repair its protocol and must also remain
            // terminal when durability consults this retry policy.
            Self::HttpRequest { detail, .. } if invalid_provider_stream(detail) => return None,
            Self::HttpRequest { timeout: true, .. } => ("https_timeout", None),
            Self::HttpRequest {
                retryable: true, ..
            } => ("https_transport", None),
            Self::HttpRejected {
                status,
                retry_after,
                ..
            } if *status == 429 => ("https_rate_limit", *retry_after),
            Self::HttpRejected {
                status,
                retry_after,
                ..
            } if (500..=599).contains(status) => ("https_server", *retry_after),
            _ => return None,
        };
        Some(RetryAdvice {
            class,
            server_delay,
        })
    }

    /// Returns advice captured at receipt, without restarting its interval.
    #[must_use]
    pub const fn retry_after(&self) -> Option<RetryAfter> {
        match self {
            Self::Api { retry_after, .. }
            | Self::HttpRejected { retry_after, .. }
            | Self::HandshakeRejected { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    pub(crate) fn is_terminal_provider_error(&self) -> bool {
        match self {
            Self::Api { event, .. } => api_error_is_terminal(event),
            Self::HttpRejected { body, .. } | Self::HandshakeRejected { body, .. } => {
                api_error_is_terminal(body)
            }
            _ => false,
        }
    }

    /// Returns a stable low-cardinality error class for telemetry.
    #[must_use]
    pub fn class(&self) -> &'static str {
        match self {
            Self::Authorization { .. } => "authorization",
            Self::HostUnavailable => "host_unavailable",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::InvalidAuthorization { .. } => "invalid_authorization",
            Self::InvalidSessionId { .. } => "invalid_session_id",
            Self::HandshakeTimeout { .. } => "handshake_timeout",
            Self::Handshake { .. } => "handshake",
            Self::HandshakeRejected { .. } => "handshake_rejected",
            Self::Send { .. } => "send",
            Self::SendTimeout { .. } => "send_timeout",
            Self::UnexpectedEnd => "premature_close",
            Self::Receive { .. } => "receive",
            Self::InvalidJson(_) => "invalid_json",
            Self::UnexpectedBinary => "unexpected_binary",
            Self::EncodeRequest(_) => "encode_request",
            Self::InvalidPayload { .. } => "invalid_payload",
            Self::Closed { .. } => "closed",
            Self::Api { event, .. } if api_error_is_checkpoint_missing(event) => {
                "checkpoint_missing"
            }
            Self::Api { event, .. }
                if api_error_has_code(event, "misalignment_policy_violation") =>
            {
                "misalignment_policy_violation"
            }
            Self::Api { .. } => "api",
            Self::ContextWindowExceeded { .. } => "context_window_exceeded",
            Self::InvalidImageRequest { .. } => "invalid_image_request",
            Self::InvalidToolSchema { .. } => "invalid_tool_schema",
            Self::HttpRequest { detail, .. } if invalid_provider_stream(detail) => {
                "invalid_provider_stream"
            }
            Self::HttpRequest { timeout: true, .. } => "https_timeout",
            Self::HttpRequest { .. } => "https_transport",
            Self::HttpRejected { body, .. }
                if api_error_has_code(body, "misalignment_policy_violation") =>
            {
                "misalignment_policy_violation"
            }
            Self::HttpRejected { status: 429, .. } => "https_rate_limit",
            Self::HttpRejected { status, .. } if (500..=599).contains(status) => "https_server",
            Self::HttpRejected { .. } => "https_rejected",
            Self::InvalidSseUtf8 { .. } => "invalid_sse_utf8",
        }
    }

    /// Returns whether the provider no longer recognizes a continuation ID.
    #[must_use]
    pub fn is_checkpoint_missing(&self) -> bool {
        matches!(self, Self::Api { event, .. } if api_error_is_checkpoint_missing(event))
    }

    /// Returns whether the provider rejected the request for context exhaustion.
    #[must_use]
    pub const fn is_context_window_exceeded(&self) -> bool {
        matches!(self, Self::ContextWindowExceeded { .. })
    }

    /// Returns the rejected discovery definition identified on the failed request.
    #[must_use]
    pub fn invalid_tool_schema(&self) -> Option<&serde_json::Value> {
        match self {
            Self::InvalidToolSchema { definition, .. } => Some(definition),
            _ => None,
        }
    }

    pub(crate) fn with_request_input<'a>(
        self,
        mut input: impl Iterator<Item = &'a crate::client::ResponseItem>,
    ) -> Self {
        let event = match &self {
            Self::Api { event, .. } => event,
            Self::HttpRejected {
                status: 400, body, ..
            } => body,
            _ => return self,
        };
        let Some(indices) = invalid_tool_schema_path(event) else {
            return self;
        };
        let Some(crate::client::ResponseItem::ToolSearchOutput { tools, .. }) = input.nth(indices[0])
        else {
            return self;
        };
        let Some(tool) = tools.get(indices[1]) else {
            return self;
        };
        let mut definition = tool.as_value();
        for index in &indices[2..] {
            if definition["type"] != "namespace" {
                return self;
            }
            let Some(child) = definition
                .get("tools")
                .and_then(serde_json::Value::as_array)
                .and_then(|tools| tools.get(*index))
            else {
                return self;
            };
            definition = child;
        }
        if definition["type"] != "function" || !definition["parameters"].is_object() {
            return self;
        }
        Self::InvalidToolSchema {
            source: Box::new(self),
            definition: Box::new(definition.clone()),
        }
    }

    /// Returns whether Astra's misalignment monitor stopped the conversation.
    #[must_use]
    pub fn is_misalignment_policy_violation(&self) -> bool {
        match self {
            Self::Api { event, .. } => api_error_has_code(event, "misalignment_policy_violation"),
            Self::HttpRejected { body, .. } => {
                api_error_has_code(body, "misalignment_policy_violation")
            }
            _ => false,
        }
    }

    pub(crate) fn http_rejected(
        status: u16,
        body: String,
        retry_after: Option<RetryAfter>,
    ) -> Self {
        if status == 400 {
            let classified = Self::api_event(body.clone());
            if matches!(
                classified,
                Self::InvalidImageRequest { .. } | Self::ContextWindowExceeded { .. }
            ) {
                return classified;
            }
        }
        Self::HttpRejected {
            status,
            body,
            retry_after,
        }
    }

    /// Classifies a received provider error and captures its advice once.
    /// Pass this error between layers rather than reparsing it after observers.
    #[must_use]
    pub fn api_event(event: String) -> Self {
        Self::api_event_received(event, super::RetryReceipt::now())
    }

    pub(crate) fn api_event_received(event: String, received: super::RetryReceipt) -> Self {
        // Provider policy stops take precedence over incidental image diagnostics.
        if [
            "misalignment_policy_violation",
            "cyber_policy",
            "bio_policy",
        ]
        .iter()
        .any(|code| api_error_has_code(&event, code))
        {
            return Self::Api {
                event,
                retry_after: None,
            };
        }
        // Validation failures identify the rejected input field, whereas older image
        // decoding failures only carry the provider's diagnostic message.
        let invalid_image_url = serde_json::from_str::<serde_json::Value>(&event)
            .ok()
            .is_some_and(|event| {
                let error = event
                    .get("error")
                    .or_else(|| event.pointer("/response/error"));
                error.is_some_and(|error| {
                    error.get("type").and_then(serde_json::Value::as_str)
                        == Some("invalid_request_error")
                        && error.get("code").and_then(serde_json::Value::as_str)
                            == Some("invalid_value")
                        && error
                            .get("param")
                            .and_then(serde_json::Value::as_str)
                            .is_some_and(|param| {
                                param.starts_with("input[") && param.ends_with(".image_url")
                            })
                })
            });
        if invalid_image_url
            || api_error_has_code(&event, "invalid_image")
            || event.contains(INVALID_IMAGE_ERROR)
        {
            Self::InvalidImageRequest { event }
        } else if api_error_has_code(&event, "context_length_exceeded") {
            Self::ContextWindowExceeded { event }
        } else {
            let retry_after =
                retryable_api_error_received(&event, received).and_then(|(_, advice)| advice);
            Self::Api { event, retry_after }
        }
    }
}

// This is the adapter-owned diagnostic, not a heuristic for arbitrary provider
// messages. Older hosts include an Error prefix and a JavaScript stack trace.
fn invalid_provider_stream(detail: &str) -> bool {
    detail.lines().next().is_some_and(|line| {
        matches!(
            line.trim(),
            "Responses: invalid provider stream" | "Error: Responses: invalid provider stream"
        )
    })
}

/// Retry metadata derived from one typed transport or API error.
#[derive(Clone, Copy, Debug)]
pub struct RetryAdvice {
    /// Stable low-cardinality retry class.
    pub class: &'static str,
    /// Original receipt-time server deadline, if any.
    pub server_delay: Option<RetryAfter>,
}
