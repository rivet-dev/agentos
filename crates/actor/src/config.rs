//! Actor-context package resolution layered over the shared hosted config.

use std::collections::BTreeSet;
use std::time::Duration;

pub(crate) use agentos_actor_contract::config::*;
use anyhow::{Context, Result};
use futures::{stream, StreamExt, TryStreamExt};

const CONFIG_PACKAGE_RESOLUTION_CONCURRENCY: usize = 8;
const CONFIG_PACKAGE_RESOLUTION_TIMEOUT: Duration = Duration::from_secs(60);

/// Resolve and pin every URL before a replacement becomes durable. The actor
/// stores only verified content identities while retaining the URL for future
/// acquisition and preload observations.
pub(crate) async fn resolve_remote_software(
    sources: Vec<RemotePackageSource>,
) -> Result<Vec<RemotePackageSource>> {
    let resolution = stream::iter(sources)
        .map(|source| async move {
            let url = source.url.clone();
            let installed = crate::preload::acquire_required_package(source.to_core(), None)
                .await
                .with_context(|| format!("resolve required software URL {url:?}"))?;
            Ok::<_, anyhow::Error>(RemotePackageSource::resolved(url, &installed))
        })
        .buffered(CONFIG_PACKAGE_RESOLUTION_CONCURRENCY)
        .try_collect::<Vec<_>>();
    let resolved = tokio::time::timeout(CONFIG_PACKAGE_RESOLUTION_TIMEOUT, resolution)
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "config_package_resolution_timeout: required software resolution exceeded {}ms; raise CONFIG_PACKAGE_RESOLUTION_TIMEOUT",
                CONFIG_PACKAGE_RESOLUTION_TIMEOUT.as_millis()
            )
        })??;

    let mut package_ids = BTreeSet::new();
    Ok(resolved
        .into_iter()
        .filter(|source| {
            source
                .package_id
                .as_ref()
                .is_some_and(|package_id| package_ids.insert(package_id.clone()))
        })
        .collect())
}
