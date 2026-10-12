//! Embedder-owned transport. No ambient host access is available through this path.
use std::{future::Future, pin::Pin};
use crate::ObscuraNetError;
pub type HostFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, ObscuraNetError>> + Send + 'a>>;
pub trait BrowserHost: Send + Sync {
    fn request(&self, request: reqwest::Request) -> HostFuture<'_, reqwest::Response>;
    fn read_file<'a>(&'a self, path: &'a std::path::Path, max_bytes: usize) -> HostFuture<'a, Vec<u8>>;
}
