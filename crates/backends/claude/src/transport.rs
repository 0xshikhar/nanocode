#[cfg(not(target_family = "wasm"))]
pub type HttpClient = reqwest::Client;
#[cfg(not(target_family = "wasm"))]
pub type HttpError = reqwest::Error;
#[cfg(not(target_family = "wasm"))]
pub type HttpResponse = reqwest::Response;

#[cfg(target_family = "wasm")]
mod host;
#[cfg(target_family = "wasm")]
pub use host::{
    HttpBody, HttpClient, HttpError, HttpFuture, HttpRequest, HttpResponse, HttpTransport,
};

#[cfg(not(target_family = "wasm"))]
pub(crate) fn is_transient(error: &HttpError) -> bool {
    error.is_timeout() || error.is_request() || error.is_body()
}

#[cfg(target_family = "wasm")]
pub(crate) const fn is_transient(error: &HttpError) -> bool {
    matches!(
        error,
        HttpError::Transport {
            retryable: true,
            ..
        } | HttpError::Transport { timeout: true, .. }
    )
}

#[cfg(not(target_family = "wasm"))]
pub(crate) async fn post(
    client: &HttpClient,
    endpoint: &str,
    mut headers: http::HeaderMap,
    body: String,
) -> Result<HttpResponse, HttpError> {
    headers
        .entry(http::header::ACCEPT_ENCODING)
        .or_insert(http::HeaderValue::from_static("gzip, br, deflate, zstd"));
    client
        .post(endpoint)
        .header("anthropic-version", crate::ANTHROPIC_VERSION)
        .headers(headers)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map(nanocode_http::decode_response)
}

#[cfg(target_family = "wasm")]
pub(crate) async fn post(
    client: &HttpClient,
    endpoint: &str,
    mut headers: http::HeaderMap,
    body: String,
) -> Result<HttpResponse, HttpError> {
    headers
        .entry("anthropic-version")
        .or_insert(http::HeaderValue::from_static(crate::ANTHROPIC_VERSION));
    headers.append(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/json"),
    );
    client
        .0
        .post(HttpRequest {
            endpoint: endpoint.to_owned(),
            headers,
            body,
        })
        .await
}
