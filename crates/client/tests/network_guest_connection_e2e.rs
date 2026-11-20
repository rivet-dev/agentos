//! Rust/TypeScript connection parity against a real guest, without host socket fallback.
mod common;

use agentos_client::config::{
    AgentOsConfig, AgentOsSidecarConfig, PatternPermissions, PermissionMode, Permissions,
};
use agentos_client::{AgentOs, ProcessStream, SpawnOptions};
use anyhow::{ensure, Context};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn trusted_connection_streams_duplex_and_never_opens_a_host_listener() {
    // Missing prerequisites fail; this qualification must never silently skip.
    assert!(
        common::sidecar_available(),
        "build the sidecar or set AGENTOS_SIDECAR_BIN"
    );
    let sidecar = AgentOs::create_sidecar(None).await.expect("create sidecar");
    let created = AgentOs::create(AgentOsConfig {
        sidecar: Some(AgentOsSidecarConfig::Explicit {
            handle: sidecar.clone(),
        }),
        permissions: Some(Permissions {
            network: Some(PatternPermissions::Mode(PermissionMode::Deny)),
            ..Default::default()
        }),
        ..Default::default()
    })
    .await;
    let vm = match created {
        Ok(vm) => vm,
        Err(error) => {
            sidecar.dispose().await.expect("dispose failed boot");
            panic!("{error}");
        }
    };
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let host = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let host_port = host.local_addr()?.port();
        ensure!(
            vm.network_connect(host_port).await.is_err(),
            "trusted connection reached host loopback"
        );
        ensure!(
            tokio::time::timeout(Duration::from_millis(20), host.accept())
                .await
                .is_err(),
            "host received a connection"
        );

        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let ready = Arc::new(Mutex::new((Some(ready_tx), Vec::<u8>::new())));
        let child = vm.spawn(
            "node",
            vec![
                "--input-type=module".into(),
                "-e".into(),
                r#"
import net from 'node:net';
net.createServer({allowHalfOpen:true},socket=>{
  socket.on('data',chunk=>socket.write(chunk));
  socket.on('end',()=>socket.end('tail',()=>process.exit(0)));
}).listen(19091,'127.0.0.1',()=>console.log('READY'));
"#
                .into(),
            ],
            SpawnOptions {
                retain_output: true,
                ..Default::default()
            },
        )?;
        let _output = vm.on_process_output(child.pid, move |event| {
            if event.stream != ProcessStream::Stdout {
                return;
            }
            let mut state = ready.lock().expect("readiness lock");
            let available = 4096usize.saturating_sub(state.1.len());
            state
                .1
                .extend_from_slice(&event.data[..event.data.len().min(available)]);
            if state.1.windows(5).any(|part| part == b"READY") {
                if let Some(sender) = state.0.take() {
                    if sender.send(()).is_err() {
                        eprintln!("guest readiness receiver was closed");
                    }
                }
            }
        })?;
        ready_rx.await.context("guest listener readiness")?;
        let connection = vm.network_connect(19091).await?;
        let input: Vec<u8> = (0..192 * 1024).map(|index| (index % 251) as u8).collect();
        let write = async {
            connection.write(&input).await?;
            connection.end().await
        };
        let read = async {
            let mut output = Vec::new();
            loop {
                let chunk = connection.read(4096).await?;
                ensure!(chunk.body.len() <= 4096, "read exceeded requested bound");
                ensure!(
                    output.len() + chunk.body.len() <= input.len() + 4,
                    "echo exceeded expected bounded output"
                );
                output.extend_from_slice(&chunk.body);
                if chunk.done {
                    break;
                }
            }
            Ok::<_, anyhow::Error>(output)
        };
        let ((), output) = tokio::try_join!(write, read)?;
        ensure!(output.len() == input.len() + 4, "guest echo ended early");
        ensure!(
            &output[..input.len()] == input.as_slice(),
            "duplex bytes changed"
        );
        ensure!(
            &output[input.len()..] == b"tail",
            "server-exit tail was lost"
        );
        ensure!(
            vm.wait_process(child.pid).await? == 0,
            "guest server failed"
        );
        connection.close().await?;
        connection.close().await?;
        Ok::<_, anyhow::Error>(())
    })
    .await;
    let shutdown = vm.shutdown().await;
    let dispose = sidecar.dispose().await;
    shutdown.expect("shutdown guest VM");
    dispose.expect("dispose sidecar");
    result
        .expect("guest connection deadline")
        .expect("guest connection assertions");
}
