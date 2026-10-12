//! Terminal causes are observed, never inferred from a requested signal or exit code.
mod common;

use agentos_client::{OpenShellOptions, SpawnOptions};
use anyhow::{ensure, Context, Result};
use std::time::{Duration, Instant};

fn program(kind: &str) -> String {
    match kind {
        "normal143" => "process.exit(143)".into(),
        "caught" => "process.on('SIGTERM', () => process.exit(0)); console.log('ready'); setInterval(() => {}, 1000)".into(),
        _ => "console.log('ready'); setInterval(() => {}, 1000)".into(),
    }
}

#[tokio::test]
async fn process_status_distinguishes_actual_signal_normal_143_and_caught_signal() -> Result<()> {
    if !common::require_sidecar(
        "process_status_distinguishes_actual_signal_normal_143_and_caught_signal",
    ) {
        return Ok(());
    }
    let vm = common::new_vm().await;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        for (kind, code, signal) in [
            ("signal", 143, Some(15)),
            ("normal143", 143, None),
            ("caught", 0, None),
        ] {
            let process = vm.spawn(
                "node",
                vec!["-e".into(), program(kind)],
                SpawnOptions {
                    retain_output: true,
                    ..Default::default()
                },
            )?;
            let (send, receive) = tokio::sync::oneshot::channel();
            let _subscription = vm.on_process_exit(process.pid, move |exit| {
                if send.send(exit).is_err() {
                    tracing::warn!("exit observer retired during bounded test cleanup");
                }
            })?;
            if kind != "normal143" {
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let replay = vm
                        .read_process_output(process.pid, None, None, None)
                        .await?;
                    let bytes: Vec<_> = replay
                        .events
                        .into_iter()
                        .flat_map(|event| event.data)
                        .collect();
                    if String::from_utf8_lossy(&bytes).contains("ready") {
                        break;
                    }
                    ensure!(Instant::now() < deadline, "process readiness exceeded5s");
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                vm.signal_process_awaited(process.pid, "SIGTERM").await?;
            }
            let exit = vm.wait_process_exit(process.pid).await?;
            ensure!(
                exit.exit_code == code && exit.exit_signal == signal,
                "wrong {kind} status: {exit:?}"
            );
            ensure!(receive.await? == exit, "live callback and wait disagree");
            ensure!(
                vm.wait_process(process.pid).await? == code,
                "scalar compatibility wait changed"
            );
            ensure!(
                vm.wait_process_exit(process.pid).await? == exit,
                "late atomic wait changed status"
            );
            let info = vm.get_process(process.pid)?;
            ensure!(
                info.exit_code == Some(code) && info.exit_signal == signal,
                "get differs from wait"
            );
            ensure!(
                vm.list_processes()
                    .into_iter()
                    .find(|info| info.pid == process.pid)
                    == Some(info),
                "list differs from get"
            );
            let replay = vm
                .read_process_output(process.pid, None, None, None)
                .await?;
            ensure!(
                replay.exit_code == Some(code) && replay.exit_signal == signal,
                "replay differs from wait"
            );
            let tree = vm.all_processes().await?;
            let info = tree
                .into_iter()
                .find(|info| info.tracked_pid == Some(process.pid))
                .context("tracked exited process missing from topology")?;
            ensure!(
                info.exit_code == Some(code) && info.exit_signal == signal,
                "topology differs from wait"
            );
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("process status controls exceeded30s");
    let cleanup = vm.shutdown().await;
    result??;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn terminal_atomic_status_survives_close_and_preserves_caught_signal() -> Result<()> {
    if !common::require_sidecar("terminal_atomic_status_survives_close_and_preserves_caught_signal")
    {
        return Ok(());
    }
    let vm = common::new_vm().await;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        for (kind, code, signal) in [
            ("signal", 143, Some(15)),
            ("normal143", 143, None),
            ("caught", 0, None),
        ] {
            let shell = vm.open_shell(OpenShellOptions {
                command: Some("node".into()),
                args: vec!["-e".into(), program(kind)],
                ..Default::default()
            })?;
            if kind != "normal143" {
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    let page = vm
                        .snapshot_shell(&shell.shell_id, None, None)
                        .await
                        .with_context(|| format!("terminal {kind}: readiness snapshot"))?;
                    let bytes: Vec<_> = page
                        .events
                        .into_iter()
                        .flat_map(|event| event.data)
                        .collect();
                    if String::from_utf8_lossy(&bytes).contains("ready") {
                        break;
                    }
                    ensure!(Instant::now() < deadline, "terminal readiness exceeded5s");
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                vm.close_shell_awaited(&shell.shell_id)
                    .await
                    .with_context(|| format!("terminal {kind}: close acknowledgement"))?;
            }
            let exit = vm
                .wait_shell_exit(&shell.shell_id)
                .await
                .with_context(|| format!("terminal {kind}: atomic wait after close"))?;
            ensure!(
                exit.exit_code == code && exit.exit_signal == signal,
                "wrong terminal {kind}: {exit:?}"
            );
            ensure!(
                vm.wait_shell(&shell.shell_id)
                    .await
                    .with_context(|| format!("terminal {kind}: scalar wait"))?
                    == code,
                "scalar terminal wait changed"
            );
            let page = vm
                .snapshot_shell(&shell.shell_id, None, None)
                .await
                .with_context(|| format!("terminal {kind}: final replay"))?;
            ensure!(
                page.exit_code == Some(code) && page.exit_signal == signal,
                "terminal replay differs from atomic wait"
            );
        }
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("terminal status controls exceeded30s");
    let cleanup = vm.shutdown().await;
    result??;
    cleanup?;
    Ok(())
}

#[tokio::test]
async fn terminal_ignored_signal_close_acknowledges_before_actual_exit() -> Result<()> {
    if !common::require_sidecar("terminal_ignored_signal_close_acknowledges_before_actual_exit") {
        return Ok(());
    }
    let vm = common::new_vm().await;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let script = "const fs=require('fs');process.on('SIGTERM',()=>fs.writeFileSync('/tmp/terminal-ignored','1'));fs.writeFileSync('/tmp/terminal-ready','1');setInterval(()=>{if(fs.existsSync('/tmp/terminal-release'))process.exit(0)},10)";
        let shell = vm.open_shell(OpenShellOptions { command: Some("node".into()), args: vec!["-e".into(), script.into()], ..Default::default() })?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !vm.exists("/tmp/terminal-ready").await? {
            ensure!(Instant::now() < deadline, "ignored terminal readiness exceeded5s");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        tokio::time::timeout(Duration::from_secs(5), vm.close_shell_awaited(&shell.shell_id)).await
            .context("close acknowledgement waited for an ignored signal to cause exit")??;
        ensure!(matches!(vm.write_shell_awaited(&shell.shell_id, agentos_client::StdinInput::Text("closed".into())).await,
            Err(agentos_client::ClientError::ShellNotFound(_))), "close admitted terminal input");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !vm.exists("/tmp/terminal-ignored").await? {
            ensure!(Instant::now() < deadline, "ignored signal delivery exceeded5s");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        ensure!(tokio::time::timeout(Duration::from_millis(20), vm.wait_shell_exit(&shell.shell_id)).await.is_err(),
            "ignored SIGTERM completed or discarded the terminal waiter before actual exit");
        vm.write_file("/tmp/terminal-release", "1").await?;
        let exit = vm.wait_shell_exit(&shell.shell_id).await?;
        ensure!(exit.exit_code == 0 && exit.exit_signal.is_none(), "ignored signal invented an exit cause: {exit:?}");
        ensure!(vm.wait_shell(&shell.shell_id).await? == 0, "late scalar wait changed");
        Ok::<_, anyhow::Error>(())
    }).await.context("ignored terminal controls exceeded30s");
    let cleanup = vm.shutdown().await;
    result??;
    cleanup?;
    Ok(())
}
