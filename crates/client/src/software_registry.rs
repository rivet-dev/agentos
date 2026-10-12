//! Registry software resolution (= TS `packages/core/src/software-registry.ts`).
//!
//! Each agentOS release publishes its software catalog as digest-addressed
//! `.aospkg` files next to `<registry>/<version>/software/manifest.json`. A
//! registry source resolves to that release's URL and exact digest, then
//! installs through the ordinary URL source, so the sidecar still verifies the
//! bytes. The manifest is fetched by this host process, not by the guest, so VM
//! network permissions do not apply to it.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;
use reqwest::Url;
use serde::Deserialize;

use crate::error::ClientError;
use crate::software::PackageSource;

/// Root that holds `<version>/software/manifest.json` for each release.
pub const DEFAULT_SOFTWARE_REGISTRY_URL: &str = "https://releases.rivet.dev/agent-os/";
/// Environment variable that overrides the default registry root.
pub const SOFTWARE_REGISTRY_URL_ENV: &str = "AGENTOS_SOFTWARE_REGISTRY_URL";

const MANIFEST_FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const MANIFEST_MAX_BYTES: usize = 8 * 1024 * 1024;
const MANIFEST_CACHE_LIMIT: usize = 32;
// `latest` moves with every release; exact versions are immutable.
const LATEST_MANIFEST_TTL: Duration = Duration::from_secs(5 * 60);

/// Registry location and default release for name-based software.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoftwareRegistryOptions {
    /// Root URL that holds `<version>/software/manifest.json`. Defaults to
    /// `AGENTOS_SOFTWARE_REGISTRY_URL`, then `https://releases.rivet.dev/agent-os/`.
    pub url: Option<String>,
    /// Release to resolve sources that name no version. Defaults to this
    /// crate's version, or `latest` for unreleased workspace and preview builds.
    pub version: Option<String>,
}

/// A software source for [`crate::AgentOs::install_software`] and
/// [`crate::AgentOsConfig::software_sources`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SoftwareSource {
    /// A URL or trusted local path.
    Package(PackageSource),
    /// A package in an agentOS release manifest, resolved on this host to a
    /// digest-pinned URL source.
    Registry {
        name: String,
        version: Option<String>,
    },
}

impl From<PackageSource> for SoftwareSource {
    fn from(source: PackageSource) -> Self {
        SoftwareSource::Package(source)
    }
}

/// A registry package resolved to its exact artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedRegistrySoftware {
    pub name: String,
    /// Release whose manifest listed the package, such as `0.4.2` or `latest`.
    pub version: String,
    /// Absolute `.aospkg` URL.
    pub url: String,
    /// Exact `sha256:<64 lowercase hex>` content digest.
    pub digest: String,
    pub size_bytes: u64,
    pub manifest_url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ManifestArtifact {
    name: String,
    digest: String,
    size: u64,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: serde_json::Value,
    artifacts: Vec<serde_json::Value>,
}

struct CachedManifest {
    manifest_url: String,
    expires_at: Option<Instant>,
    artifacts: Arc<Vec<ManifestArtifact>>,
}

// Process-wide like the TS module cache, bounded by MANIFEST_CACHE_LIMIT
// distinct registry/version pairs. The guard is never held across an await.
static MANIFEST_CACHE: Lazy<parking_lot::Mutex<VecDeque<CachedManifest>>> =
    Lazy::new(|| parking_lot::Mutex::new(VecDeque::new()));

/// The release a registry source resolves against when it names no version.
///
/// A published client matches its own release so a given agentOS version gets
/// the software built with it. The committed workspace version `0.0.1` and
/// `0.0.0-*` preview builds never publish software, so they use `latest`.
pub fn default_registry_version(version: &str) -> String {
    if version == "0.0.1" || version == "0.0.0" || version.starts_with("0.0.0-") {
        return String::from("latest");
    }
    version.to_string()
}

fn registry_root(options: Option<&SoftwareRegistryOptions>) -> Result<Url, ClientError> {
    let raw = options
        .and_then(|options| options.url.clone())
        .or_else(|| std::env::var(SOFTWARE_REGISTRY_URL_ENV).ok())
        .unwrap_or_else(|| DEFAULT_SOFTWARE_REGISTRY_URL.to_string());
    let normalized = if raw.ends_with('/') {
        raw.clone()
    } else {
        format!("{raw}/")
    };
    let root = Url::parse(&normalized).map_err(|_| {
        ClientError::InvalidConfig(format!(
            "invalid agentOS software registry URL {raw:?}; pass an absolute http(s) URL in software_registry.url or {SOFTWARE_REGISTRY_URL_ENV}"
        ))
    })?;
    ensure_secure_registry_url(&root, "registry URL")?;
    Ok(root)
}

// The manifest supplies the digest every package is verified against, so it
// needs the same transport integrity as the packages: HTTPS, or plain HTTP only
// to a loopback mirror.
fn ensure_secure_registry_url(url: &Url, label: &str) -> Result<(), ClientError> {
    let loopback = url.host_str().is_some_and(|host| {
        host == "localhost"
            || host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .parse::<std::net::IpAddr>()
                .is_ok_and(|ip| ip.is_loopback())
    });
    if url.scheme() == "https" || (url.scheme() == "http" && loopback) {
        return Ok(());
    }
    Err(ClientError::InvalidConfig(format!(
        "agentOS software {label} must use https, or plain http only on a loopback address, got {url}"
    )))
}

fn validate_name(name: &str) -> Result<(), ClientError> {
    let mut chars = name.chars();
    let valid = matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid {
        return Err(ClientError::InvalidPackageSource(format!(
            "invalid agentOS registry package name {name:?}; names are lowercase letters, digits, and hyphens, such as \"git\""
        )));
    }
    Ok(())
}

fn validate_version(version: &str) -> Result<(), ClientError> {
    let valid = version == "latest" || {
        let (core, prerelease) = match version.split_once('-') {
            Some((core, prerelease)) => (core, Some(prerelease)),
            None => (version, None),
        };
        let parts: Vec<&str> = core.split('.').collect();
        parts.len() == 3
            && parts
                .iter()
                .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
            && prerelease.is_none_or(|prerelease| {
                !prerelease.is_empty()
                    && prerelease
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            })
    };
    if !valid {
        return Err(ClientError::InvalidPackageSource(format!(
            "invalid agentOS registry version {version:?}; pass a release version such as \"0.4.2\", or \"latest\""
        )));
    }
    Ok(())
}

fn is_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'))
    })
}

fn parse_manifest(bytes: &[u8], manifest_url: &str) -> Result<Vec<ManifestArtifact>, ClientError> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|error| {
        ClientError::PackageDownload(format!(
            "agentOS software manifest {manifest_url} is not a schemaVersion 1 manifest with an artifacts list: {error}"
        ))
    })?;
    if manifest.schema_version != serde_json::json!(1) {
        return Err(ClientError::PackageDownload(format!(
            "agentOS software manifest {manifest_url} is not a schemaVersion 1 manifest with an artifacts list"
        )));
    }
    manifest
        .artifacts
        .into_iter()
        .enumerate()
        .map(|(index, value)| {
            serde_json::from_value::<ManifestArtifact>(value)
                .ok()
                .filter(|artifact| is_digest(&artifact.digest))
                .ok_or_else(|| {
                    ClientError::PackageDownload(format!(
                        "agentOS software manifest {manifest_url} has a malformed artifact at index {index}"
                    ))
                })
        })
        .collect()
}

async fn fetch_manifest(
    manifest_url: &str,
    version: &str,
) -> Result<Vec<ManifestArtifact>, ClientError> {
    let unreachable = |error: &dyn std::fmt::Display| {
        ClientError::PackageDownload(format!(
            "failed to fetch agentOS software manifest {manifest_url}: {error}; check host network access or set software_registry.url / {SOFTWARE_REGISTRY_URL_ENV} to a reachable mirror"
        ))
    };
    let client = reqwest::Client::builder()
        .timeout(MANIFEST_FETCH_TIMEOUT)
        .build()
        .map_err(|error| unreachable(&error))?;
    let mut response = client
        .get(manifest_url)
        .send()
        .await
        .map_err(|error| unreachable(&error))?;
    ensure_secure_registry_url(response.url(), "manifest redirect")?;
    let status = response.status();
    if status == reqwest::StatusCode::NOT_FOUND {
        return Err(ClientError::PackageDownload(format!(
            "agentOS software registry has no release \"{version}\": GET {manifest_url} returned 404; pass a published agentOS version or \"latest\""
        )));
    }
    if !status.is_success() {
        return Err(ClientError::PackageDownload(
            format!(
                "failed to fetch agentOS software manifest {manifest_url}: HTTP {} {}",
                status.as_u16(),
                status.canonical_reason().unwrap_or_default()
            )
            .trim_end()
            .to_string(),
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MANIFEST_MAX_BYTES as u64)
    {
        return Err(ClientError::PackageDownload(format!(
            "agentOS software manifest {manifest_url} exceeds the {MANIFEST_MAX_BYTES}-byte limit"
        )));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| unreachable(&error))?
    {
        if body.len() + chunk.len() > MANIFEST_MAX_BYTES {
            return Err(ClientError::PackageDownload(format!(
                "agentOS software manifest {manifest_url} exceeds the {MANIFEST_MAX_BYTES}-byte limit"
            )));
        }
        body.extend_from_slice(&chunk);
    }
    parse_manifest(&body, manifest_url)
}

async fn load_manifest(
    manifest_url: &str,
    version: &str,
) -> Result<Arc<Vec<ManifestArtifact>>, ClientError> {
    {
        let mut cache = MANIFEST_CACHE.lock();
        let now = Instant::now();
        if let Some(index) = cache
            .iter()
            .position(|entry| entry.manifest_url == manifest_url)
        {
            if cache[index]
                .expires_at
                .is_none_or(|expires_at| expires_at > now)
            {
                return Ok(cache[index].artifacts.clone());
            }
            cache.remove(index);
        }
    }
    // Failed fetches are never cached, so the next call retries.
    let artifacts = Arc::new(fetch_manifest(manifest_url, version).await?);
    let mut cache = MANIFEST_CACHE.lock();
    cache.retain(|entry| entry.manifest_url != manifest_url);
    while cache.len() >= MANIFEST_CACHE_LIMIT {
        cache.pop_front();
    }
    cache.push_back(CachedManifest {
        manifest_url: manifest_url.to_string(),
        expires_at: if version == "latest" {
            Some(Instant::now() + LATEST_MANIFEST_TTL)
        } else {
            None
        },
        artifacts: artifacts.clone(),
    });
    Ok(artifacts)
}

fn edit_distance(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();
    let mut previous: Vec<usize> = (0..=right.len()).collect();
    for (i, left_char) in left.chars().enumerate() {
        let mut current = vec![i + 1];
        for (j, right_char) in right.iter().enumerate() {
            current.push(
                (previous[j + 1] + 1)
                    .min(current[j] + 1)
                    .min(previous[j] + usize::from(left_char != *right_char)),
            );
        }
        previous = current;
    }
    previous[right.len()]
}

fn close_matches<'a>(name: &str, names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut scored: Vec<(usize, &str)> = names
        .map(|candidate| {
            let distance = if candidate.contains(name) || name.contains(candidate) {
                0
            } else {
                edit_distance(name, candidate)
            };
            (distance, candidate)
        })
        .filter(|(distance, _)| *distance <= 2)
        .collect();
    scored.sort();
    scored
        .into_iter()
        .take(5)
        .map(|(_, candidate)| candidate)
        .collect()
}

/// Resolve a registry package name to the exact `.aospkg` URL and digest listed
/// in a release manifest. `version` overrides `registry.version`. Manifests are
/// cached per registry and version for the life of the process; `latest` is
/// refreshed every five minutes.
pub async fn resolve_registry_software(
    name: &str,
    version: Option<&str>,
    registry: Option<&SoftwareRegistryOptions>,
) -> Result<ResolvedRegistrySoftware, ClientError> {
    validate_name(name)?;
    let version = version
        .map(str::to_string)
        .or_else(|| registry.and_then(|registry| registry.version.clone()))
        .unwrap_or_else(|| default_registry_version(env!("CARGO_PKG_VERSION")));
    validate_version(&version)?;
    let manifest_url = registry_root(registry)?
        .join(&format!("{version}/software/manifest.json"))
        .map_err(|error| ClientError::InvalidConfig(format!("invalid registry URL: {error}")))?;
    let manifest_url = manifest_url.to_string();
    let artifacts = load_manifest(&manifest_url, &version).await?;
    let Some(artifact) = artifacts.iter().find(|artifact| artifact.name == name) else {
        let suggestions = close_matches(
            name,
            artifacts.iter().map(|artifact| artifact.name.as_str()),
        );
        let hint = if suggestions.is_empty() {
            String::from(
                "; browse https://agentos-sdk.dev/agentos/docs/software for available packages",
            )
        } else {
            format!(
                "; did you mean {}?",
                suggestions
                    .iter()
                    .map(|suggestion| format!("\"{suggestion}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        return Err(ClientError::SoftwareNotFound(format!(
            "agentOS registry release \"{version}\" has no package \"{name}\" ({manifest_url}){hint}"
        )));
    };
    let url = Url::parse(&manifest_url)
        .and_then(|base| base.join(&artifact.path))
        .map_err(|error| {
            ClientError::PackageDownload(format!(
                "agentOS software manifest {manifest_url} lists {name} at an invalid path: {error}"
            ))
        })?;
    if url.scheme() != "https" && url.scheme() != "http" {
        return Err(ClientError::PackageDownload(format!(
            "agentOS software manifest {manifest_url} lists {name} at a non-http(s) URL: {url}"
        )));
    }
    Ok(ResolvedRegistrySoftware {
        name: name.to_string(),
        version,
        url: url.to_string(),
        digest: artifact.digest.clone(),
        size_bytes: artifact.size,
        manifest_url,
    })
}

/// Resolve a [`SoftwareSource`] to the package source the sidecar acquires.
pub async fn resolve_software_source(
    source: SoftwareSource,
    registry: Option<&SoftwareRegistryOptions>,
) -> Result<PackageSource, ClientError> {
    match source {
        SoftwareSource::Package(source) => Ok(source),
        SoftwareSource::Registry { name, version } => {
            let resolved = resolve_registry_software(&name, version.as_deref(), registry).await?;
            Ok(PackageSource::Url {
                url: resolved.url,
                expected_digest: Some(resolved.digest),
            })
        }
    }
}

/// Test hook: forget cached manifests.
#[doc(hidden)]
pub fn clear_software_registry_cache() {
    MANIFEST_CACHE.lock().clear();
}

/// Test hook: number of cached manifests.
#[doc(hidden)]
pub fn software_registry_cache_size() -> usize {
    MANIFEST_CACHE.lock().len()
}
