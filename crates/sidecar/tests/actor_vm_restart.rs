//! Opt-in live actor test. Start a disposable, isolated local Rivet engine on
//! 127.0.0.1:6420 first; actor metadata remains in that engine's test storage.
//! The actor binary under test is launched separately and uses its own
//! `sidecar` entry point to boot the VM.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::process::Stdio;
use std::time::Duration;

use agentos_actor::{
    ActorSpawnOptions, AgentOsActor, CronCancel, CronFiredEvent, CronList, CronSchedule,
    VmLifecycleState, VmRestart, VmStatus, ACTOR_NAME,
};
use anyhow::{bail, Context, Result};
use nix::sys::signal::{kill, killpg, Signal};
use nix::unistd::Pid;
use rivetkit::client::{Client, ClientConfig, ConnectionStatus};
use rivetkit::TypedClientExt;
use tokio::process::{Child, Command};

struct ActorProcess {
    child: Child,
    group: Pid,
    stopped: bool,
}

impl ActorProcess {
    fn signal_group(&self, signal: Signal) -> Result<()> {
        match killpg(self.group, signal) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => Ok(()),
            Err(error) => Err(error).context("signal live test process group"),
        }
    }

    async fn shutdown(&mut self) -> Result<()> {
        match kill(self.group, Signal::SIGTERM) {
            Ok(()) | Err(nix::errno::Errno::ESRCH) => {}
            Err(error) => return Err(error).context("terminate live actor worker"),
        }
        let graceful = tokio::time::timeout(Duration::from_secs(15), self.child.wait()).await;
        self.signal_group(Signal::SIGKILL)?;
        if graceful.is_err() {
            tokio::time::timeout(Duration::from_secs(5), self.child.wait())
                .await
                .context("reap killed live actor worker timed out")??;
        }
        self.stopped = true;
        graceful
            .context("live actor worker did not shut down gracefully")?
            .context("reap live actor worker")?;
        Ok(())
    }
}

impl Drop for ActorProcess {
    fn drop(&mut self) {
        if !self.stopped {
            if let Err(error) = self.signal_group(Signal::SIGKILL) {
                eprintln!("stop live actor test process group: {error:#}");
            }
        }
        // Tokio's kill-on-drop child handling reaps without blocking this thread.
    }
}

fn actor_log(path: &std::path::Path) -> String {
    (|| -> std::io::Result<String> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(8192)))?;
        let mut tail = Vec::new();
        file.take(8192).read_to_end(&mut tail)?;
        Ok(String::from_utf8_lossy(&tail).into_owned())
    })()
    .unwrap_or_else(|error| format!("read actor log: {error}"))
}

#[tokio::test]
async fn probe_cleanup_reaps_its_isolated_process_group() -> Result<()> {
    let child = Command::new("/bin/sleep")
        .arg("30")
        .process_group(0)
        .kill_on_drop(true)
        .spawn()?;
    let group = Pid::from_raw(i32::try_from(child.id().context("test child has no PID")?)?);
    let mut process = ActorProcess {
        child,
        group,
        stopped: false,
    };
    process.shutdown().await?;
    anyhow::ensure!(
        process.child.try_wait()?.is_some(),
        "probe child was not reaped"
    );
    Ok(())
}

/// Requires an already-healthy local Rivet engine. Keeping this opt-in avoids
/// making normal CI depend on engine downloads, native software staging, or
/// the host's available ports.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires a healthy local Rivet engine at 127.0.0.1:6420"]
async fn live_actor_restart_and_cron_use_rivetkit_durability() -> Result<()> {
    let temporary = tempfile::tempdir().context("create live actor test directory")?;
    let log_path = temporary.path().join("actor.log");
    let stdout = File::create(&log_path).context("create actor stdout log")?;
    let stderr = stdout.try_clone().context("clone actor log handle")?;
    let pool = format!("agentos-vm-restart-{}", uuid::Uuid::new_v4());
    let child = Command::new(env!("CARGO_BIN_EXE_agentos-sidecar"))
        .arg("actor")
        .arg("--package-cache-dir")
        .arg(temporary.path().join("package-cache"))
        .env_remove("AGENTOS_INSPECTOR_TABS_DIR")
        .env_remove("AGENTOS_PACKAGE_CACHE_DIR")
        .env("AGENTOS_SIDECAR_BIN", env!("CARGO_BIN_EXE_agentos-sidecar"))
        .env("RIVETKIT_ENGINE_SPAWN", "never")
        .env("RIVET_ENDPOINT", "http://127.0.0.1:6420")
        .env("RIVET_TOKEN", "dev")
        .env("RIVET_NAMESPACE", "default")
        .env("RIVET_POOL_NAME", &pool)
        .env("AGENTOS_LOG", "warn")
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr))
        .process_group(0)
        .kill_on_drop(true)
        .spawn()
        .context("start agentOS actor binary")?;
    let group = Pid::from_raw(i32::try_from(
        child.id().context("actor child has no PID")?,
    )?);
    let mut child = ActorProcess {
        child,
        group,
        stopped: false,
    };
    let client = Client::new(
        ClientConfig::new("http://127.0.0.1:6420")
            .token("dev")
            .namespace("default")
            .pool_name(pool),
    );
    let actor = client.get_or_create_typed_default::<AgentOsActor>(
        ACTOR_NAME,
        [format!("vm-restart-{}", uuid::Uuid::new_v4())],
    )?;
    let job_name = format!("restart-check-{}", uuid::Uuid::new_v4());
    let firing_job_name = format!("fire-check-{}", uuid::Uuid::new_v4());
    let mut connection = None;
    // This bounds every action, including schedule/list/cancel calls below.
    let result = tokio::time::timeout(Duration::from_secs(240), async {
        let before = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                match actor.call(VmStatus {}).await {
                    Ok(status) => break Ok::<_, anyhow::Error>(status),
                    Err(error) => {
                        if child.child.try_wait()?.is_some() {
                            break Err(
                                error.context("actor binary exited before accepting actions")
                            );
                        }
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
            }
        })
        .await
        .with_context(|| format!("wait for actor VM status; {}", actor_log(&log_path)))??;
        if before.lifecycle != VmLifecycleState::Ready {
            bail!(
                "initial VM was not ready: {before:?}; actor log: {}",
                actor_log(&log_path)
            );
        }

        let scheduled = actor
            .call(CronSchedule {
                name: Some(job_name.clone()),
                expression: "0 0 1 1 *".to_owned(),
                timezone: Some("UTC".to_owned()),
                command: "echo".to_owned(),
                args: vec!["cron restart check".to_owned()],
                options: ActorSpawnOptions::default(),
                max_history: Some(2),
            })
            .await
            .with_context(|| format!("schedule live cron job; {}", actor_log(&log_path)))?;
        anyhow::ensure!(scheduled.name == job_name);

        let after = tokio::time::timeout(Duration::from_secs(60), actor.call(VmRestart {}))
            .await
            .with_context(|| format!("restart live VM; {}", actor_log(&log_path)))??;
        anyhow::ensure!(
            after.lifecycle == VmLifecycleState::Ready,
            "restarted VM was not ready: {after:?}; actor log: {}",
            actor_log(&log_path)
        );
        anyhow::ensure!(
            after.generation > before.generation,
            "VM restart reused generation {}",
            before.generation
        );
        anyhow::ensure!(
            after.applied_config_revision == before.applied_config_revision,
            "VM restart changed applied config revision"
        );
        let observed = actor.call(VmStatus {}).await?;
        anyhow::ensure!(observed.generation == after.generation);
        let jobs = actor.call(CronList {}).await?;
        anyhow::ensure!(
            jobs.iter().any(|job| job.name == job_name),
            "scheduled job was lost after VM restart: {jobs:?}"
        );
        anyhow::ensure!(
            actor
                .call(CronCancel {
                    name: job_name.clone()
                })
                .await?
        );
        anyhow::ensure!(actor.call(CronList {}).await?.is_empty());

        // A connected client observes the private scheduled invocation through
        // the public event, without needing access to the private action name.
        let connection = connection.insert(actor.connect());
        let (event_tx, mut event_rx) = tokio::sync::mpsc::channel(1);
        let _subscription = connection
            .on::<CronFiredEvent>(move |event| {
                if let Err(error) = event_tx.try_send(event) {
                    eprintln!("deliver live cron test event: {error}");
                }
            })
            .await;
        tokio::time::timeout(Duration::from_secs(15), async {
            while connection.inner().conn_status() != ConnectionStatus::Connected {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .with_context(|| format!("connect for live cron event; {}", actor_log(&log_path)))?;

        actor
            .call(CronSchedule {
                name: Some(firing_job_name.clone()),
                expression: "* * * * *".to_owned(),
                timezone: Some("UTC".to_owned()),
                command: "echo".to_owned(),
                args: vec!["cron fire check".to_owned()],
                options: ActorSpawnOptions::default(),
                max_history: Some(2),
            })
            .await?;
        let event = tokio::time::timeout(Duration::from_secs(90), async {
            loop {
                let event = event_rx.recv().await.context("cron event stream closed")?;
                if event.schedule_name == firing_job_name {
                    break Ok::<_, anyhow::Error>(event);
                }
            }
        })
        .await
        .with_context(|| format!("wait for live cron event; {}", actor_log(&log_path)))??;
        anyhow::ensure!(
            event.error.is_none() && event.process.is_some(),
            "scheduled cron command failed: {event:?}; actor log: {}",
            actor_log(&log_path)
        );
        anyhow::ensure!(
            actor
                .call(CronCancel {
                    name: firing_job_name.clone()
                })
                .await?
        );
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("live restart/cron probe exceeded 240 seconds")
    .and_then(|result| result);

    // Clean both uniquely named jobs even if an assertion or timed-out call
    // interrupted the probe. The engine's isolated storage owns actor metadata.
    let mut cleanup_error = None;
    for name in [job_name, firing_job_name] {
        if let Err(error) =
            tokio::time::timeout(Duration::from_secs(10), actor.call(CronCancel { name }))
                .await
                .context("cancel live test cron job timed out")
                .and_then(|result| result)
        {
            eprintln!("cancel live test cron job: {error:#}");
            cleanup_error = Some(error);
        }
    }
    if let Some(connection) = connection {
        if let Err(error) =
            tokio::time::timeout(Duration::from_secs(5), connection.disconnect()).await
        {
            eprintln!("disconnect live actor test client: {error}");
            cleanup_error = Some(error.into());
        }
    }
    if let Err(error) = child.shutdown().await {
        eprintln!("shut down live actor test process: {error:#}");
        cleanup_error = Some(error);
    }
    match (result, cleanup_error) {
        (Err(error), Some(cleanup)) => {
            Err(error.context(format!("live probe cleanup also failed: {cleanup:#}")))
        }
        (Err(error), None) => Err(error),
        (Ok(()), Some(error)) => Err(error),
        (Ok(()), None) => Ok(()),
    }
}
