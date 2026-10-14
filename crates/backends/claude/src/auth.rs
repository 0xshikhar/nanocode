use std::{sync::Arc, time::Duration};

use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};
use tokio::sync::Mutex;
use web_time::SystemTime;

use crate::{ClaudeAuthFuture, ClaudeAuthProvider, ClaudeAuthUnavailable};

pub struct ClaudeAccessToken {
    authorization: HeaderValue,
    expires_at: SystemTime,
}

impl ClaudeAccessToken {
    pub fn new(
        token: impl AsRef<str>,
        expires_at: SystemTime,
    ) -> Result<Self, ClaudeAuthUnavailable> {
        let token = token.as_ref();
        if token.is_empty()
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"-._~+/=".contains(&byte))
        {
            return Err(ClaudeAuthUnavailable::new(
                "the Claude access token is empty or contains characters outside the OAuth token alphabet",
            ));
        }
        let mut authorization = HeaderValue::from_str(&format!("Bearer {token}")).map_err(|_| {
            ClaudeAuthUnavailable::new("the Claude access token is not a valid HTTP header value")
        })?;
        authorization.set_sensitive(true);
        Ok(Self {
            authorization,
            expires_at,
        })
    }

    fn usable(&self, refresh_margin: Duration) -> bool {
        self.expires_at
            .duration_since(SystemTime::now())
            .is_ok_and(|remaining| remaining > refresh_margin)
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(AUTHORIZATION, self.authorization.clone());
        headers
    }
}

pub trait ClaudeTokenSource: Send + Sync {
    fn refresh(&self) -> ClaudeAuthFuture<'_, Result<ClaudeAccessToken, ClaudeAuthUnavailable>>;
}

pub struct RefreshingClaudeAuth {
    source: Arc<dyn ClaudeTokenSource>,
    refresh_margin: Duration,
    token: Mutex<Option<ClaudeAccessToken>>,
}

impl RefreshingClaudeAuth {
    pub fn new(source: Arc<dyn ClaudeTokenSource>, refresh_margin: Duration) -> Self {
        Self {
            source,
            refresh_margin,
            token: Mutex::new(None),
        }
    }
}

impl ClaudeAuthProvider for RefreshingClaudeAuth {
    fn headers(&self) -> ClaudeAuthFuture<'_, Result<HeaderMap, ClaudeAuthUnavailable>> {
        Box::pin(async move {
            let mut cached = self.token.lock().await;
            if let Some(token) = cached
                .as_ref()
                .filter(|token| token.usable(self.refresh_margin))
            {
                return Ok(token.headers());
            }

            *cached = None;
            let token = self.source.refresh().await?;
            if !token.usable(self.refresh_margin) {
                return Err(ClaudeAuthUnavailable::new(
                    "the refreshed Claude access token is already expired or within its refresh margin",
                ));
            }
            let headers = token.headers();
            *cached = Some(token);
            Ok(headers)
        })
    }

    fn recover_unauthorized<'a>(
        &'a self,
        rejected: &'a HeaderMap,
    ) -> ClaudeAuthFuture<'a, Result<bool, ClaudeAuthUnavailable>> {
        Box::pin(async move {
            let mut cached = self.token.lock().await;

            if cached
                .as_ref()
                .is_some_and(|token| rejected.get(AUTHORIZATION) == Some(&token.authorization))
            {
                *cached = None;
            }
            Ok(true)
        })
    }
}
