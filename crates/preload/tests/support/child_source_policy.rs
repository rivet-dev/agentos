#[tokio::test]
#[ignore = "requires fresh same-version AGENTOS_SIDECAR_BIN; run this exact test alone for startup-only policy qualification"]
async fn preload_factory_raised_url_reaches_native_child() -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    anyhow::ensure!(
        std::env::var_os("AGENTOS_SIDECAR_BIN").is_some(),
        "fresh native fixture AGENTOS_SIDECAR_BIN is required"
    );
    let policy: PreloadValidationPolicy = serde_json::from_str(r#"{"maxUrlBytes":8192}"#)?;
    configure_preload_validation_policy(policy)?;
    let mut options = hosted_package_options();
    anyhow::ensure!(
        options.max_source_url_bytes == 8192,
        "configured factory failed to project source URL budget"
    );
    options.allow_insecure_local_http = true;
    options.download_timeout_ms = 1000;
    options.connect_timeout_ms = 1000;
    let mut tar = tar::Builder::new(Vec::new());
    for (path, contents, mode) in [
        (
            "agentos-package.json",
            br#"{"name":"source-policy-fixture","version":"1.0.0"}"#.as_slice(),
            0o644,
        ),
        ("bin/fixture", b"#!/bin/sh\nexit 0\n".as_slice(), 0o755),
    ] {
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(mode);
        header.set_cksum();
        tar.append_data(&mut header, path, contents)?;
    }
    let bytes = vfs::package_format::pack::pack_aospkg_from_tar_bytes(&tar.into_inner()?)?.0;
    let expected_size = bytes.len() as u64;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let url = format!(
        "http://{address}/fixture.aospkg?padding={}",
        "x".repeat(5000)
    );
    let sidecar = AgentOs::create_sidecar(None).await?;
    let mut server = Some(tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(3), async move {
            let (mut stream, _) = listener.accept().await?;
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            while !request.windows(4).any(|part| part == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await?;
                anyhow::ensure!(count > 0, "fixture request ended before headers");
                request.extend_from_slice(&chunk[..count]);
                anyhow::ensure!(
                    request.len() <= 16384,
                    "fixture request exceeds16384-byte guard"
                );
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            );
            stream.write_all(header.as_bytes()).await?;
            stream.write_all(&bytes).await?;
            Ok::<_, anyhow::Error>(())
        })
        .await
        .context("fixture3s deadline")?
    }));
    let mut session = None;
    let body = async {
        session = Some(sidecar.open_package_session().await?);
        let session = session.as_ref().unwrap();
        let source = PackageSource::Url {
            url: url.clone(),
            expected_digest: None,
        };
        let first = session
            .acquire(source, false, Some(2000), Some(&options))
            .await?;
        anyhow::ensure!(
            first.package_name == "source-policy-fixture" && first.size_bytes == expected_size,
            "native acquisition did not verify fixture bytes"
        );
        server
            .take()
            .unwrap()
            .await
            .context("fixture server join")??; // no listener remains for the cached case
        let cached = PackageSource::Url {
            url: url.clone(),
            expected_digest: Some(first.digest.clone()),
        };
        let second = session
            .acquire(cached.clone(), true, Some(2000), Some(&options))
            .await?;
        anyhow::ensure!(
            second.digest == first.digest,
            "source/digest cache did not preserve verified bytes"
        );
        let defaults = PackageResolverOptions {
            allow_insecure_local_http: true,
            ..Default::default()
        };
        let rejected = session
            .acquire(cached, false, Some(2000), Some(&defaults))
            .await
            .err()
            .context("default4096 must reject the same cached long source")?;
        anyhow::ensure!(
            rejected.to_string().contains("max_source_url_bytes"),
            "wrong default rejection: {rejected}"
        );
        for invalid in [
            PackageSource::Url {
                url: "file:///tmp/fixture.aospkg".into(),
                expected_digest: None,
            },
            PackageSource::Url {
                url: url.clone(),
                expected_digest: Some("not-a-digest".into()),
            },
        ] {
            anyhow::ensure!(
                session
                    .acquire(invalid, false, Some(2000), Some(&options))
                    .await
                    .is_err(),
                "raised source budget bypassed syntax/digest validation"
            );
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    // Preserve all failures while always retiring package ownership and child.
    let mut failures = Vec::new();
    if let Err(error) = body {
        failures.push(format!("body: {error:#}"));
    }
    if let Some(session) = session {
        if let Err(error) = session.close().await {
            failures.push(format!("close: {error}"));
        }
    }
    if let Err(error) = sidecar.dispose().await {
        failures.push(format!("sidecar disposal: {error}"));
    }
    if let Some(server) = server {
        server.abort();
        match server.await {
            Err(error) if error.is_cancelled() => {}
            Ok(Ok(())) => {}
            Ok(Err(error)) => failures.push(format!("fixture: {error:#}")),
            Err(error) => failures.push(format!("fixture join: {error}")),
        }
    }
    anyhow::ensure!(failures.is_empty(), "{}", failures.join("; "));
    Ok(())
}
