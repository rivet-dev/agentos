//! Experimental Obscura embedding. This is not registered as a VM executor.
//!
//! The transport adapter exercises the production executor host contract;
//! process lifecycle and native DOM accounting must be completed before the
//! browser can accept guest commands in a shipping VM.

use agentos_executor_contract::backend::{
    direct_host_reply_channel, HostCallIdentity, HostCallReply, HostServiceError, PayloadLimit,
};
use agentos_executor_contract::host::{
    BoundedBytes, BoundedString, BoundedUsize, BoundedVec, FilesystemOperation, HostOperation,
    HttpHeader, NetworkOperation, ProcessHostCapabilitySet,
};
use base64::Engine;
use obscura_net::host::{BrowserHost, HostFuture};
use obscura_net::ObscuraNetError;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

/// An immutable process-bound authority supplied by the VM, never by page JS.
pub struct ExecutorBrowserHost {
    services: ProcessHostCapabilitySet,
    next_call: AtomicU64,
    bytes: PayloadLimit,
    headers: PayloadLimit,
    strings: PayloadLimit,
}

impl ExecutorBrowserHost {
    pub fn new(
        services: ProcessHostCapabilitySet,
        max_payload_bytes: usize,
    ) -> Result<Self, HostServiceError> {
        Ok(Self {
            services,
            next_call: AtomicU64::new(1),
            bytes: PayloadLimit::with_stderr_warning(
                "limits.obscura.maxPayloadBytes",
                max_payload_bytes,
            )?,
            headers: PayloadLimit::with_stderr_warning("limits.obscura.maxHeaders", 128)?,
            strings: PayloadLimit::with_stderr_warning("limits.obscura.maxStringBytes", 8192)?,
        })
    }

    async fn call(
        &self,
        operation: HostOperation,
        retained_bytes: usize,
    ) -> Result<HostCallReply, HostServiceError> {
        let call_id = self
            .next_call
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| HostServiceError::new("EOVERFLOW", "Obscura host call ids exhausted"))?;
        let process = self.services.process();
        let (reply, response) = direct_host_reply_channel(
            HostCallIdentity {
                generation: process.generation,
                pid: process.pid,
                call_id,
            },
            self.bytes.maximum(),
        )?;
        let admission = self.services.admit_request(retained_bytes)?;
        self.services.submit(operation, reply, admission)?;
        response.await
    }

    async fn http(&self, request: reqwest::Request) -> Result<reqwest::Response, HostServiceError> {
        let body = match request.body() {
            Some(body) => body.as_bytes().ok_or_else(|| {
                HostServiceError::new(
                    "ENOTSUP",
                    "streaming Obscura request bodies are not supported",
                )
            })?,
            None => &[],
        };
        self.bytes.admit(body.len())?;
        self.headers.admit(request.headers().len())?;
        let mut retained_bytes = request.url().as_str().len() + request.method().as_str().len();
        retained_bytes = retained_bytes.saturating_add(body.len());
        let mut headers = Vec::with_capacity(request.headers().len());
        for (name, value) in request.headers() {
            let value = value.to_str().map_err(|_| {
                HostServiceError::new("EINVAL", "Obscura request contains a non-text HTTP header")
            })?;
            retained_bytes = retained_bytes.saturating_add(name.as_str().len() + value.len());
            headers.push(HttpHeader {
                name: BoundedString::try_new(name.as_str().to_owned(), &self.strings)?,
                value: BoundedString::try_new(value.to_owned(), &self.strings)?,
            });
        }
        let count_limit =
            PayloadLimit::with_warning_hook(self.bytes.name(), self.bytes.maximum(), None)?;
        let max = BoundedUsize::try_new(self.bytes.maximum(), &count_limit)?;
        let operation = HostOperation::Network(NetworkOperation::HttpRequest {
            url: BoundedString::try_new(request.url().to_string(), &self.strings)?,
            method: BoundedString::try_new(request.method().to_string(), &self.strings)?,
            headers: BoundedVec::try_new(headers, &self.headers)?,
            body: BoundedBytes::try_new(body.to_vec(), &self.bytes)?,
            max_response_bytes: max,
            max_header_bytes: max,
            max_body_bytes: max,
        });
        let HostCallReply::Json(value) = self.call(operation, retained_bytes).await? else {
            return Err(HostServiceError::new(
                "EPROTO",
                "expected HTTP response metadata",
            ));
        };
        let status = value["status"]
            .as_u64()
            .and_then(|status| u16::try_from(status).ok())
            .ok_or_else(|| HostServiceError::new("EPROTO", "invalid HTTP response status"))?;
        let encoded = value["bodyBase64"]
            .as_str()
            .ok_or_else(|| HostServiceError::new("EPROTO", "missing HTTP response body"))?;
        self.bytes.admit(encoded.len())?;
        let body = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| HostServiceError::new("EPROTO", "invalid HTTP response body"))?;
        let mut response = http::Response::builder().status(status);
        let headers = value["headers"]
            .as_object()
            .ok_or_else(|| HostServiceError::new("EPROTO", "missing HTTP response headers"))?;
        self.headers.admit(headers.len())?;
        for (name, value) in headers {
            let value = value
                .as_str()
                .ok_or_else(|| HostServiceError::new("EPROTO", "invalid HTTP response header"))?;
            self.strings.admit(name.len())?;
            self.strings.admit(value.len())?;
            response = response.header(name, value);
        }
        response
            .body(reqwest::Body::from(body))
            .map(reqwest::Response::from)
            .map_err(|error| HostServiceError::new("EPROTO", error.to_string()))
    }

    async fn file(&self, path: &Path, max_bytes: usize) -> Result<Vec<u8>, HostServiceError> {
        let path = path
            .to_str()
            .ok_or_else(|| HostServiceError::new("EINVAL", "Obscura file URL is not UTF-8"))?;
        let path = BoundedString::try_new(path.to_owned(), &self.strings)?;
        let size = path.as_str().len();
        let count_limit =
            PayloadLimit::with_warning_hook(self.bytes.name(), self.bytes.maximum(), None)?;
        let max_bytes = BoundedUsize::try_new(max_bytes.min(self.bytes.maximum()), &count_limit)?;
        match self
            .call(
                HostOperation::Filesystem(FilesystemOperation::ReadFileAt {
                    dir_fd: u32::MAX - 99, // Linux AT_FDCWD (-100)
                    path,
                    max_bytes,
                }),
                size,
            )
            .await?
        {
            HostCallReply::Raw(bytes) => {
                self.bytes.admit(bytes.len())?;
                Ok(bytes)
            }
            _ => Err(HostServiceError::new("EPROTO", "expected file bytes")),
        }
    }
}

fn net_error(error: HostServiceError) -> ObscuraNetError {
    ObscuraNetError::Network(error.to_string())
}

impl BrowserHost for ExecutorBrowserHost {
    fn request(&self, request: reqwest::Request) -> HostFuture<'_, reqwest::Response> {
        Box::pin(async move { self.http(request).await.map_err(net_error) })
    }

    fn read_file<'a>(&'a self, path: &'a Path, max_bytes: usize) -> HostFuture<'a, Vec<u8>> {
        Box::pin(async move { self.file(path, max_bytes).await.map_err(net_error) })
    }
}
