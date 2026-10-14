use crate::client::ResponseError;
pub use crate::client::transport::ResponsesError;
pub use nanocode_agent::{CompactionRecovery, ExecutionPolicyDisposition, NanocodexError, Result};
impl nanocode_agent::BackendFailure for ResponseError {
    fn requires_image_repair(&self) -> bool {
        matches!(
            self.responses_error(),
            Some(ResponsesError::InvalidImageRequest { .. })
        )
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
impl From<ResponseError> for NanocodexError {
    fn from(error: ResponseError) -> Self {
        Self::Response(nanocode_agent::BackendError(Box::new(error)))
    }
}

pub trait ErrorExt {
    fn responses_error(&self) -> Option<&ResponsesError>;

    fn rejected_request_repair(&self) -> crate::client::__private::RejectedRequestRepair<'_>;
}
impl ErrorExt for NanocodexError {
    fn responses_error(&self) -> Option<&ResponsesError> {
        match self {
            Self::Response(error) => error
                .as_any()
                .downcast_ref::<ResponseError>()
                .and_then(ResponseError::responses_error),
            Self::Shutdown(error) => error.responses_error(),
            _ => None,
        }
    }
    fn rejected_request_repair(&self) -> crate::client::__private::RejectedRequestRepair<'_> {
        use crate::client::__private::RejectedRequestRepair;
        RejectedRequestRepair {
            replace_images: self.requires_image_repair(),
            ..self
                .responses_error()
                .map(RejectedRequestRepair::for_error)
                .unwrap_or_default()
        }
    }
}
