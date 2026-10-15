use std::{sync::Arc, time::Duration};

use nanocode_mcp::oauth::{ClientAuthentication, TokenError, TokenResponse};
use tokio::time::timeout;

use super::{McpOAuthCredentials, OAuthRuntime, now_millis};

const REFRESH_SKEW_MILLIS: u64 = 30_000;
const REFRESH_LOCK_TIMEOUT: Duration = Duration::from_secs(60);
const REFRESH_REQUEST_TIMEOUT: Duration = Duration::from_secs(45);

impl McpOAuthCredentials {
    fn needs_refresh(&self) -> bool {
        self.expires_at_millis.is_some_and(|expires_at| {
            now_millis().saturating_add(REFRESH_SKEW_MILLIS) >= expires_at
        })
    }

    fn from_refresh_response(client_id: String, response: &TokenResponse, previous: &Self) -> Self {
        let mut credentials =
            Self::from_token_response(client_id, response, previous.issuer.clone());
        if response.refresh_token.is_none() {
            credentials
                .refresh_token
                .clone_from(&previous.refresh_token);
        }
        if response.scope.is_none() {
            credentials.scopes.clone_from(&previous.scopes);
        }
        credentials
    }
}

impl OAuthRuntime {
    pub fn refresh_if_needed<'call>(
        self: &'call Arc<Self>,
    ) -> futures_util::future::BoxFuture<'call, Result<(), String>> {
        Box::pin(async move {
            let refresh_needed = self
                .credentials
                .lock()
                .await
                .as_ref()
                .is_none_or(McpOAuthCredentials::needs_refresh);
            if !refresh_needed {
                return Ok(());
            }
            self.spawn_refresh("expiry", None).await
        })
    }

    pub(crate) async fn refresh_after_rejection(
        self: &Arc<Self>,
        rejected_access_token: &str,
    ) -> Result<String, String> {
        self.spawn_refresh("rejected", Some(rejected_access_token.to_owned()))
            .await?;
        self.current_access_token().await
    }

    async fn spawn_refresh(
        self: &Arc<Self>,
        reason: &'static str,
        rejected_access_token: Option<String>,
    ) -> Result<(), String> {
        let runtime = Arc::clone(self);
        let task = tokio::spawn(async move {
            let result = runtime
                .refresh_transaction(rejected_access_token.as_deref())
                .await;
            if let Err(error) = &result {
                tracing::warn!(
                    server = %runtime.server_name,
                    refresh_reason = reason,
                    %error,
                    "MCP OAuth refresh transaction failed"
                );
            }
            result
        });
        task.await.map_err(|error| {
            format!(
                "OAuth refresh task failed for MCP server `{}`: {error}",
                self.server_name
            )
        })?
    }

    async fn refresh_transaction(&self, rejected_access_token: Option<&str>) -> Result<(), String> {
        let _refresh_lock = timeout(
            REFRESH_LOCK_TIMEOUT,
            self.store
                .acquire_refresh_lock(&self.server_name, &self.server_url),
        )
        .await
        .map_err(|_| {
            format!(
                "timed out after {REFRESH_LOCK_TIMEOUT:?} waiting for the OAuth refresh lock for `{}`",
                self.server_name
            )
        })??;

        let latest = self
            .store
            .load(&self.server_name, &self.server_url)
            .await
            .map_err(|error| {
                format!("failed to reread authoritative OAuth credentials: {error}")
            })?;
        let Some(latest) = latest else {
            *self.credentials.lock().await = None;
            return Err(format!(
                "OAuth credentials for `{}` were removed; authorization required",
                self.server_name
            ));
        };

        let metadata = timeout(
            REFRESH_REQUEST_TIMEOUT,
            super::discover(&self.http, &self.base_url),
        )
        .await
        .map_err(|_| {
            format!(
                "timed out after {REFRESH_REQUEST_TIMEOUT:?} resolving current OAuth metadata for `{}`",
                self.server_name
            )
        })?
        .map_err(|error| {
            format!(
                "failed to resolve current OAuth metadata for `{}`: {error}",
                self.server_name
            )
        })?
        .metadata;
        super::validate_authorization_server_endpoints(&metadata).map_err(|error| {
            format!(
                "current OAuth metadata for `{}` is invalid: {error}",
                self.server_name
            )
        })?;
        let current_issuer = super::authorization_issuer(&metadata)?;
        super::validate_refresh_token_issuer(&latest, current_issuer.as_deref()).map_err(
            |error| {
                format!(
                    "OAuth refresh credentials for `{}` cannot be used: {error}",
                    self.server_name
                )
            },
        )?;

        let rejected = rejected_access_token.is_some_and(|token| token == latest.access_token);
        if !latest.needs_refresh() && !rejected {
            *self.credentials.lock().await = Some(latest);
            return Ok(());
        }
        let Some(refresh_token) = latest
            .refresh_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
        else {
            return Err(format!(
                "OAuth credentials for `{}` cannot be refreshed; authorization required",
                self.server_name
            ));
        };

        *self.credentials.lock().await = Some(latest.clone());
        let mut scopes = latest.scopes.clone();
        metadata.add_offline_access(&mut scopes);
        let request = nanocode_mcp::oauth::refresh_token_request(
            &ClientAuthentication::public(latest.client_id.clone()),
            refresh_token,
            &scopes,
            self.base_url.as_str(),
        );
        let response = match timeout(
            REFRESH_REQUEST_TIMEOUT,
            super::token_endpoint_request(&self.http, &metadata.token_endpoint, request),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(error @ TokenError::Rejected(_))) => {
                return Err(format!(
                    "OAuth refresh token for `{}` was rejected; authorization required: OAuth refresh token was rejected: {error}",
                    self.server_name
                ));
            }
            Ok(Err(error)) => {
                return Err(format!(
                    "temporarily failed to refresh OAuth credentials for `{}`: OAuth token refresh failed: {error}",
                    self.server_name
                ));
            }
            Err(_) => {
                return Err(format!(
                    "timed out after {REFRESH_REQUEST_TIMEOUT:?} refreshing OAuth credentials for `{}`",
                    self.server_name
                ));
            }
        };
        let refreshed = McpOAuthCredentials::from_refresh_response(
            latest.client_id.clone(),
            &response,
            &latest,
        );

        if let Err(error) = self
            .store
            .save(&self.server_name, &self.server_url, &refreshed)
            .await
        {
            return Err(format!(
                "failed to persist refreshed OAuth credentials for `{}`: {error}",
                self.server_name
            ));
        }

        *self.credentials.lock().await = Some(refreshed);
        Ok(())
    }
}
