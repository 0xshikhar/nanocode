pub use nanocode_agent::{NanocodexError, Result, ExecutionPolicyDisposition, CompactionRecovery};
pub use crate::client::transport::ResponsesError;
use crate::client::ResponseError;
impl nanocode_agent::BackendFailure for ResponseError {
    fn requires_image_repair(&self) -> bool { matches!(self.responses_error(), Some(ResponsesError::InvalidImageRequest { .. })) }
    fn as_any(&self) -> &dyn std::any::Any { self }
}
impl From<ResponseError> for NanocodexError {
    fn from(error: ResponseError) -> Self { Self::Response(nanocode_agent::BackendError(Box::new(error))) }
}
/// Codex diagnostics for a shared lifecycle error.
pub trait ErrorExt {
    /// Returns the underlying Responses transport diagnostic.
    fn responses_error(&self) -> Option<&ResponsesError>;
    /// Describes repairs supported after a rejected request.
    fn rejected_request_repair(&self) -> crate::client::__private::RejectedRequestRepair<'_>;
}
impl ErrorExt for NanocodexError {
    fn responses_error(&self) -> Option<&ResponsesError> {
        match self {
            Self::Response(error) => error.as_any().downcast_ref::<ResponseError>().and_then(ResponseError::responses_error),
            Self::Shutdown(error) => error.responses_error(),
            _ => None,
        }
    }
    fn rejected_request_repair(&self) -> crate::client::__private::RejectedRequestRepair<'_> {
        use crate::client::__private::RejectedRequestRepair;
        RejectedRequestRepair { replace_images: self.requires_image_repair(), ..self.responses_error().map(RejectedRequestRepair::for_error).unwrap_or_default() }
    }
}
