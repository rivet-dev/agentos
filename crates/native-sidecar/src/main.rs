use std::os::fd::{FromRawFd, OwnedFd};

use nix::fcntl::{fcntl, FcntlArg};

const CONTROL_FD: i32 = 3;

fn main() {
    // Default to WARN so near-limit / backpressure warnings actually surface
    // (they were swallowed at ERROR-only); operators can tune via AGENTOS_LOG
    // (e.g. `error` to quiet, `debug` for queue snapshots). Logs MUST go to stderr:
    // stdout is the framed wire-protocol channel, so logging there would corrupt it.
    let level = std::env::var("AGENTOS_LOG")
        .ok()
        .and_then(|value| value.parse::<tracing::Level>().ok())
        .unwrap_or(tracing::Level::WARN);
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .init();
    agentos_v8_runtime::native_heap::configure_native_heap();
    if let Err(error) = run_sidecar(std::env::args().skip(1)) {
        tracing::error!(?error, "agentOS sidecar failed");
        std::process::exit(1);
    }
}

fn run_sidecar(args: impl Iterator<Item = String>) -> Result<(), String> {
    if let Err(error) = fcntl(CONTROL_FD, FcntlArg::F_GETFD) {
        tracing::error!(
            ?error,
            fd = CONTROL_FD,
            "missing inherited sidecar response/control descriptor"
        );
        return Err(format!(
            "missing inherited sidecar response/control descriptor: {error}"
        ));
    }
    // SAFETY: the process launch contract reserves fd 3 for the inherited
    // response/control socket and transfers its sole ownership to the sidecar.
    // The fcntl probe above establishes that the descriptor is open before it
    // is adopted.
    let control_fd = unsafe { OwnedFd::from_raw_fd(CONTROL_FD) };
    agentos_native_sidecar::process_entrypoint::run(control_fd, args)
}
