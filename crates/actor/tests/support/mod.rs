use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Method, StatusCode, Url};
use serde_json::Value;

pub(crate) struct Fixture {
    pub endpoint: String,
    pub namespace: String,
    pub pool: String,
    pub token: String,
    pub node_token: String,
    pub admin_token: String,
    http: reqwest::Client,
}

impl Fixture {
    pub fn from_env() -> Result<Self> {
        anyhow::ensure!(
            std::env::var("AGENTOS_HOSTED_TEST_REQUIRE_LOCAL").as_deref() == Ok("1"),
            "explicit owned hosted fixture required"
        );
        let endpoint = std::env::var("AGENTOS_HOSTED_TEST_ENDPOINT")?;
        let parsed = Url::parse(&endpoint)?;
        anyhow::ensure!(
            parsed.scheme() == "http"
                && parsed.host_str() == Some("127.0.0.1")
                && parsed.port().is_some()
                && parsed.username().is_empty()
                && parsed.password().is_none(),
            "owned fixture requires an explicit loopback endpoint without URL credentials"
        );
        Ok(Self {
            endpoint,
            namespace: std::env::var("AGENTOS_HOSTED_TEST_NAMESPACE")?,
            pool: std::env::var("AGENTOS_HOSTED_TEST_POOL")?,
            token: std::env::var("AGENTOS_HOSTED_TEST_TOKEN")?,
            node_token: std::env::var("AGENTOS_HOSTED_TEST_NODE_TOKEN")?,
            admin_token: std::env::var("AGENTOS_HOSTED_TEST_ADMIN_TOKEN")?,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(2))
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .build()?,
        })
    }

    pub async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
        token: &str,
    ) -> Result<(StatusCode, Value)> {
        let url = Url::parse(&self.endpoint)?.join(path)?;
        anyhow::ensure!(
            url.origin() == Url::parse(&self.endpoint)?.origin(),
            "owned fixture request escaped its endpoint"
        );
        let mut request = self.http.request(method, url).bearer_auth(token);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let mut response = request.send().await.context("owned fixture request")?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            anyhow::ensure!(
                bytes.len().saturating_add(chunk.len()) <= 1024 * 1024,
                "limit_exceeded: owned fixture response exceeds1MiB"
            );
            bytes.extend_from_slice(&chunk);
        }
        let value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        };
        Ok((status, value))
    }

    pub async fn issue_token(&self, grants: Value, duration: u64) -> Result<(String, i64)> {
        let (status, response) = self.request(Method::POST, "/auth/tokens", Some(serde_json::json!({
            "namespace": self.namespace, "duration": duration, "subject": "owned-rust-control", "grants": grants,
        })), &self.admin_token).await?;
        anyhow::ensure!(status.is_success(), "owned JWT issuance failed: {status}");
        Ok((
            response["token"]
                .as_str()
                .context("JWT issuance omitted token")?
                .to_owned(),
            response["expires_ts"]
                .as_i64()
                .context("JWT issuance omitted expiry")?,
        ))
    }
}
