//! VM lifecycle work must not stall live processes in other VMs.
//!
//! Both VMs share one real `agentos-native-sidecar`. One VM has a large root
//! filesystem, so its create (shadow-root materialization, mount table) and
//! dispose (final root snapshot, kernel teardown) take a long time. A process
//! in the other VM echoes stdin the whole time and must keep answering.

mod common;

use std::time::{Duration, Instant};

use agentos_client::config::{
    AgentOsConfig, AgentOsSidecarConfig, RootFilesystemConfig, RootLowerInput,
};
use agentos_client::fs::{
    DirEntryType, FilesystemEntry, FilesystemEntryEncoding, FilesystemSnapshotEntries,
    FilesystemSnapshotExport, RootSnapshotExport, SnapshotExportKind,
};
use agentos_client::{AgentOs, ProcessStream, SpawnOptions, StdinInput, Subscription};
use tokio::sync::mpsc;

const LARGE_ROOT_FILES: usize = 16_000;
const ROUND_TRIP_TIMEOUT: Duration = Duration::from_secs(30);
const READY: &[u8] = b"ready\n";

fn large_root_filesystem() -> RootFilesystemConfig {
    let entry = |path: String, entry_type, mode: &str, content: Option<String>| FilesystemEntry {
        path,
        entry_type,
        mode: mode.to_owned(),
        uid: 0,
        gid: 0,
        encoding: content.as_ref().map(|_| FilesystemEntryEncoding::Utf8),
        content,
        target: None,
    };
    let mut entries = vec![entry(
        String::from("/data"),
        DirEntryType::Directory,
        "0755",
        None,
    )];
    entries.extend((0..LARGE_ROOT_FILES).map(|index| {
        entry(
            format!("/data/file-{index}.txt"),
            DirEntryType::File,
            "0644",
            Some("x".repeat(256)),
        )
    }));
    RootFilesystemConfig {
        lowers: vec![RootLowerInput::SnapshotExport(RootSnapshotExport {
            kind: SnapshotExportKind::SnapshotExport,
            source: FilesystemSnapshotExport {
                format: String::from("agentos-filesystem-snapshot-v1"),
                filesystem: FilesystemSnapshotEntries { entries },
            },
        })],
        ..RootFilesystemConfig::default()
    }
}

/// A VM with a large root filesystem in the same sidecar as the echo VM.
async fn create_large_vm(pool: &str) -> Result<AgentOs, agentos_client::ClientError> {
    AgentOs::create(AgentOsConfig {
        root_filesystem: large_root_filesystem(),
        sidecar: Some(AgentOsSidecarConfig::Shared {
            pool: Some(pool.to_owned()),
        }),
        ..AgentOsConfig::default()
    })
    .await
}

/// A guest process that writes every stdin chunk back to stdout.
struct Echo {
    vm: AgentOs,
    pid: u32,
    stdout: mpsc::UnboundedReceiver<Vec<u8>>,
    _subscription: Subscription,
    received: Vec<u8>,
    sent: Vec<u8>,
    next_token: u64,
}

impl Echo {
    async fn start(pool: &str) -> Self {
        let vm = common::new_vm_with_sidecar_pool(pool).await;
        let handle = vm
            .spawn_process(
                "node",
                vec![
                    String::from("-e"),
                    String::from(
                        "process.stdin.on('data', (chunk) => process.stdout.write(chunk)); process.stdout.write('ready\\n')",
                    ),
                ],
                SpawnOptions::default(),
            )
            .expect("spawn echo process");
        let (tx, stdout) = mpsc::unbounded_channel();
        let subscription = vm
            .on_process_output(handle.pid, move |output| {
                if output.stream == ProcessStream::Stdout {
                    let _ = tx.send(output.data);
                }
            })
            .expect("subscribe to echo output");
        let mut echo = Self {
            vm,
            pid: handle.pid,
            stdout,
            _subscription: subscription,
            received: Vec::new(),
            sent: READY.to_vec(),
            next_token: 0,
        };
        echo.read_until(READY).await;
        echo
    }

    async fn round_trip(&mut self) -> Duration {
        self.next_token += 1;
        let token = format!("<{}>", self.next_token).into_bytes();
        let started = Instant::now();
        self.vm
            .write_process_stdin_awaited(self.pid, StdinInput::Bytes(token.clone()))
            .await
            .expect("write echo stdin");
        self.sent.extend_from_slice(&token);
        self.read_until(&token).await;
        started.elapsed()
    }

    async fn read_until(&mut self, marker: &[u8]) {
        tokio::time::timeout(ROUND_TRIP_TIMEOUT, async {
            while !self.received.ends_with(marker) {
                let chunk = self.stdout.recv().await.expect("echo output stream ended");
                self.received.extend_from_slice(&chunk);
            }
        })
        .await
        .expect("echo output timed out");
    }

    /// Echo round trips while `operation` runs. The round trip in flight
    /// when the operation ends is included, so a stall is always counted.
    async fn round_trips_during<T: Send + 'static>(
        &mut self,
        operation: impl std::future::Future<Output = T> + Send + 'static,
    ) -> (T, Duration, Vec<Duration>) {
        let started = Instant::now();
        let operation = tokio::spawn(operation);
        let mut round_trips = Vec::new();
        while !operation.is_finished() {
            round_trips.push(self.round_trip().await);
        }
        let output = operation.await.expect("lifecycle operation task");
        (output, started.elapsed(), round_trips)
    }
}

fn assert_not_stalled(phase: &str, took: Duration, round_trips: &[Duration]) {
    let slowest = round_trips.iter().max().copied().unwrap_or_default();
    eprintln!(
        "{phase}: took {took:?}, {} echo round trips, slowest {slowest:?}",
        round_trips.len()
    );
    assert!(
        round_trips.len() >= 3,
        "{phase} took {took:?}; too short to overlap echo round trips ({} samples)",
        round_trips.len()
    );
    assert!(
        slowest < took / 3,
        "a live process waited {slowest:?} for one stdin echo while another VM's {phase} ran for {took:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn large_vm_create_and_dispose_do_not_stall_another_vms_stdin() {
    if !common::require_sidecar("large_vm_create_and_dispose_do_not_stall_another_vms_stdin") {
        return;
    }
    const POOL: &str = "lifecycle-isolation-create-dispose";
    let mut echo = Echo::start(POOL).await;

    let (large, took, round_trips) = echo.round_trips_during(create_large_vm(POOL)).await;
    let large = large.expect("create VM with a large root filesystem");
    assert_not_stalled("create", took, &round_trips);

    let (shutdown, took, round_trips) = echo
        .round_trips_during(async move { large.shutdown().await })
        .await;
    shutdown.expect("dispose VM with a large root filesystem");
    assert_not_stalled("dispose", took, &round_trips);

    // Every write reached the process, in order, with nothing lost or reordered.
    assert_eq!(
        String::from_utf8_lossy(&echo.received),
        String::from_utf8_lossy(&echo.sent)
    );
    echo.vm.shutdown().await.expect("shutdown echo VM");
}
