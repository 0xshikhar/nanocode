use std::{future::Future, pin::Pin, sync::Arc};

use futures_util::{Stream, TryStreamExt};
use http::{HeaderMap, StatusCode};

pub type HttpFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type HttpBody = Pin<Box<dyn Stream<Item = Result<Vec<u8>, HttpError>>>>;

pub trait HttpTransport: Send + Sync + 'static {
    fn post(&self, request: HttpRequest) -> HttpFuture<'_, Result<HttpResponse, HttpError>>;
}

pub struct HttpRequest {
    pub endpoint: String,
    pub headers: HeaderMap,
    pub body: String,
}

#[derive(Clone)]
pub struct HttpClient(pub(super) Arc<dyn HttpTransport>);

impl HttpClient {
    pub fn new(transport: impl HttpTransport) -> Self {
        Self(Arc::new(transport))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("{detail}")]
    Transport {
        detail: String,
        retryable: bool,
        timeout: bool,
    },
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub struct HttpResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: HttpBody,
}

impl HttpResponse {
    pub fn new(status: StatusCode, headers: HeaderMap, body: HttpBody) -> Self {
        Self {
            status,
            headers,
            body,
        }
    }

    pub const fn status(&self) -> StatusCode {
        self.status
    }

    pub const fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub fn bytes_stream(self) -> HttpBody {
        self.body
    }

    async fn bytes(self) -> Result<Vec<u8>, HttpError> {
        self.body
            .try_fold(Vec::new(), |mut bytes, chunk| async move {
                bytes.extend(chunk);
                Ok(bytes)
            })
            .await
    }

    pub async fn text(self) -> Result<String, HttpError> {
        Ok(String::from_utf8_lossy(&self.bytes().await?).into_owned())
    }

    pub async fn json<T: serde::de::DeserializeOwned>(self) -> Result<T, HttpError> {
        Ok(serde_json::from_slice(&self.bytes().await?)?)
    }
}
