//! Registry resolution against a local registry with the release layout
//! `<version>/software/manifest.json`. These tests never reach the network.

use std::collections::HashMap;
use std::sync::Arc;

use agentos_client::config::AgentOsConfig;
use agentos_client::software_registry::software_registry_cache_size;
use agentos_client::{
    default_registry_version, resolve_registry_software, AgentOs, ClientError,
    PackageResolverOptions, SoftwareRegistryOptions, SoftwareSource,
};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

mod common;

const DIGEST_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

type Routes = HashMap<String, (u16, Vec<u8>)>;

#[derive(Clone, Default)]
struct Registry {
    url: String,
    routes: Arc<parking_lot::Mutex<Routes>>,
    requests: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl Registry {
    async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let registry = Registry {
            url: format!("http://{}/agent-os/", listener.local_addr().unwrap()),
            ..Registry::default()
        };
        let served = registry.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let served = served.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut buffer = [0u8; 1024];
                    while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                        let Ok(read) = stream.read(&mut buffer).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&buffer[..read]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    served.requests.lock().push(path.clone());
                    let (status, body) = served
                        .routes
                        .lock()
                        .get(&path)
                        .cloned()
                        .unwrap_or((404, Vec::new()));
                    let head = format!(
                        "HTTP/1.1 {status} X\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                });
            }
        });
        registry
    }

    fn options(&self) -> SoftwareRegistryOptions {
        SoftwareRegistryOptions {
            url: Some(self.url.clone()),
            version: None,
        }
    }

    fn serve_raw(&self, version: &str, status: u16, body: &[u8]) {
        self.routes.lock().insert(
            format!("/agent-os/{version}/software/manifest.json"),
            (status, body.to_vec()),
        );
    }

    fn serve(&self, version: &str, names: &[&str]) {
        let artifacts: Vec<_> = names
            .iter()
            .map(|name| {
                serde_json::json!({
                    "name": name,
                    "digest": format!("sha256:{DIGEST_HEX}"),
                    "size": 1,
                    "path": format!("packages/{name}/{DIGEST_HEX}.aospkg"),
                })
            })
            .collect();
        let manifest = serde_json::json!({ "schemaVersion": 1, "artifacts": artifacts });
        self.serve_raw(version, 200, manifest.to_string().as_bytes());
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().clone()
    }
}

#[test]
fn default_version_matches_the_client_release() {
    assert_eq!(default_registry_version("0.4.2"), "0.4.2");
    assert_eq!(default_registry_version("0.4.3-rc.1"), "0.4.3-rc.1");
    assert_eq!(default_registry_version("0.0.1"), "latest");
    assert_eq!(
        default_registry_version("0.0.0-my-branch.abc1234"),
        "latest"
    );
}

#[tokio::test]
async fn checks_transport_security_when_resolving_each_artifact_url() {
    let registry = Registry::start().await;
    let manifest = serde_json::json!({"schemaVersion": 1, "artifacts": [{
        "name": "git", "size": 1, "digest": format!("sha256:{DIGEST_HEX}"),
        "path": "http://registry.gameinc.io/git.aospkg"
    }]});
    registry.serve_raw("latest", 200, manifest.to_string().as_bytes());
    let options = SoftwareRegistryOptions {
        url: Some(registry.url.clone()),
        version: None,
    };
    let error = resolve_registry_software("git", Some("latest"), Some(&options))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("artifact URL must use https"),
        "{error}"
    );
}

#[tokio::test]
async fn resolves_a_name_to_a_digest_pinned_url() {
    let registry = Registry::start().await;
    registry.serve("latest", &["git", "ripgrep"]);
    let resolved = resolve_registry_software("git", None, Some(&registry.options()))
        .await
        .unwrap();
    assert_eq!(resolved.version, "latest");
    assert_eq!(resolved.digest, format!("sha256:{DIGEST_HEX}"));
    assert_eq!(
        resolved.url,
        format!(
            "{}latest/software/packages/git/{DIGEST_HEX}.aospkg",
            registry.url
        )
    );
    assert_eq!(
        resolved.manifest_url,
        format!("{}latest/software/manifest.json", registry.url)
    );
}

#[tokio::test]
async fn explicit_versions_win_and_manifests_are_cached() {
    let registry = Registry::start().await;
    registry.serve("0.4.2", &["git"]);
    registry.serve("0.4.1", &["git"]);
    let pinned = SoftwareRegistryOptions {
        version: Some(String::from("0.4.1")),
        ..registry.options()
    };
    let explicit = resolve_registry_software("git", Some("0.4.2"), Some(&pinned))
        .await
        .unwrap();
    assert_eq!(explicit.version, "0.4.2");
    for _ in 0..2 {
        let defaulted = resolve_registry_software("git", None, Some(&pinned))
            .await
            .unwrap();
        assert_eq!(defaulted.version, "0.4.1");
    }
    assert_eq!(
        registry.requests(),
        vec![
            "/agent-os/0.4.2/software/manifest.json",
            "/agent-os/0.4.1/software/manifest.json",
        ]
    );
}

#[tokio::test]
async fn the_manifest_cache_is_bounded() {
    let registry = Registry::start().await;
    for patch in 0..40 {
        let version = format!("1.0.{patch}");
        registry.serve(&version, &["git"]);
        resolve_registry_software("git", Some(&version), Some(&registry.options()))
            .await
            .unwrap();
    }
    assert!(software_registry_cache_size() <= 32);
}

#[tokio::test]
async fn failed_fetches_are_not_cached() {
    let registry = Registry::start().await;
    registry.serve_raw("latest", 503, b"");
    let error = resolve_registry_software("git", None, Some(&registry.options()))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains(&format!(
            "failed to fetch agentOS software manifest {}latest/software/manifest.json: HTTP 503",
            registry.url
        )),
        "{error}"
    );
    registry.serve("latest", &["git"]);
    resolve_registry_software("git", None, Some(&registry.options()))
        .await
        .unwrap();
}

#[tokio::test]
async fn unknown_names_list_close_matches() {
    let registry = Registry::start().await;
    registry.serve("latest", &["git", "gitleaks", "ripgrep"]);
    let error = resolve_registry_software("gti", None, Some(&registry.options()))
        .await
        .unwrap_err();
    assert!(matches!(error, ClientError::SoftwareNotFound(_)));
    assert!(
        error.to_string().contains("did you mean \"git\"?"),
        "{error}"
    );
}

#[tokio::test]
async fn missing_releases_and_bad_input_fail_explicitly() {
    let registry = Registry::start().await;
    let missing = resolve_registry_software("git", Some("9.9.9"), Some(&registry.options()))
        .await
        .unwrap_err();
    assert!(
        missing.to_string().contains(&format!(
            "has no release \"9.9.9\": GET {}9.9.9/software/manifest.json returned 404",
            registry.url
        )),
        "{missing}"
    );
    registry.serve_raw("latest", 200, b"{not json");
    let malformed = resolve_registry_software("git", None, Some(&registry.options()))
        .await
        .unwrap_err();
    assert!(
        malformed.to_string().contains("schemaVersion 1"),
        "{malformed}"
    );
    for (name, version, message) in [
        ("../git", None, "invalid agentOS registry package name"),
        ("git", Some("../latest"), "invalid agentOS registry version"),
    ] {
        let error = resolve_registry_software(name, version, Some(&registry.options()))
            .await
            .unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    for url in ["ftp://mirror/", "http://mirror.example.com/"] {
        let options = SoftwareRegistryOptions {
            url: Some(url.to_string()),
            version: None,
        };
        let error = resolve_registry_software("git", None, Some(&options))
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("must use https, or plain http only on a loopback address"),
            "{error}"
        );
    }
}

fn test_package() -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::<u8>::new());
    let manifest = br#"{"name":"registry-tool","version":"1.0.0"}"#;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "agentos-package.json", &manifest[..])
        .unwrap();
    let command = b"#!/usr/bin/env node\nprocess.stdout.write('registry-ok\\n');\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(command.len() as u64);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "bin/registry-cmd", &command[..])
        .unwrap();
    let tar = builder.into_inner().unwrap();
    vfs::package_format::pack::pack_aospkg_from_tar_bytes(&tar)
        .unwrap()
        .0
}

fn serve_package(registry: &Registry, digest_override: Option<&str>) -> String {
    let bytes = test_package();
    let hex = format!("{:x}", Sha256::digest(&bytes));
    let manifest = serde_json::json!({
        "schemaVersion": 1,
        "artifacts": [{
            "name": "registry-tool",
            "digest": digest_override.map(str::to_string).unwrap_or(format!("sha256:{hex}")),
            "size": bytes.len(),
            "path": format!("packages/registry-tool/{hex}.aospkg"),
        }],
    });
    registry.serve_raw("latest", 200, manifest.to_string().as_bytes());
    registry.routes.lock().insert(
        format!("/agent-os/latest/software/packages/registry-tool/{hex}.aospkg"),
        (200, bytes),
    );
    format!("sha256:{hex}")
}

fn vm_config(registry: &Registry, sources: Vec<SoftwareSource>) -> AgentOsConfig {
    AgentOsConfig {
        software_sources: sources,
        software_registry: Some(registry.options()),
        package_resolver: Some(PackageResolverOptions {
            allow_insecure_local_http: true,
            ..PackageResolverOptions::default()
        }),
        ..AgentOsConfig::default()
    }
}

fn registry_tool() -> SoftwareSource {
    SoftwareSource::Registry {
        name: String::from("registry-tool"),
        version: None,
    }
}

#[tokio::test]
async fn create_installs_registry_sources_before_returning() {
    if !common::require_sidecar("create_installs_registry_sources_before_returning") {
        return;
    }
    let registry = Registry::start().await;
    let digest = serve_package(&registry, None);
    let os = AgentOs::create(vm_config(&registry, vec![registry_tool()]))
        .await
        .expect("create VM");
    let installed = os.installed_software();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].digest, digest);
    assert!(os
        .exists("/opt/agentos/bin/registry-cmd")
        .await
        .expect("check linked command"));
    os.shutdown().await.expect("shutdown VM");
}

#[tokio::test]
async fn install_software_accepts_a_registry_source() {
    if !common::require_sidecar("install_software_accepts_a_registry_source") {
        return;
    }
    let registry = Registry::start().await;
    let digest = serve_package(&registry, None);
    let os = AgentOs::create(vm_config(&registry, Vec::new()))
        .await
        .expect("create VM");
    let installed = os.install_software(registry_tool()).await.expect("install");
    assert_eq!(installed.digest, digest);
    assert_eq!(installed.package_name, "registry-tool");
    os.shutdown().await.expect("shutdown VM");
}

#[tokio::test]
async fn create_fails_on_digest_mismatch_and_unknown_names() {
    if !common::require_sidecar("create_fails_on_digest_mismatch_and_unknown_names") {
        return;
    }
    let registry = Registry::start().await;
    serve_package(&registry, Some(&format!("sha256:{}", "0".repeat(64))));
    let mismatch = AgentOs::create(vm_config(&registry, vec![registry_tool()]))
        .await
        .err()
        .expect("digest mismatch must fail create");
    assert!(mismatch.to_string().contains("digest"), "{mismatch}");

    let unknown = AgentOs::create(vm_config(
        &registry,
        vec![SoftwareSource::Registry {
            name: String::from("registry-tol"),
            version: None,
        }],
    ))
    .await
    .err()
    .expect("unknown name must fail create");
    assert!(
        unknown
            .to_string()
            .contains("did you mean \"registry-tool\"?"),
        "{unknown}"
    );
}
