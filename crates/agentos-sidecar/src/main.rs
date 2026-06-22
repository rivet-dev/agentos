fn main() {
    // The sidecar speaks its binary protocol over stdout (see
    // `secure_exec_sidecar::stdio`), so tracing output MUST go to stderr —
    // otherwise log lines are interleaved into the frame stream and the client
    // misreads them as (garbage-length) frames. See crates/CLAUDE.md:
    // "Control channels must be out-of-band."
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::ERROR)
        .init();
    if let Err(error) =
        secure_exec_sidecar::stdio::run_with_extensions(agentos_sidecar_wrapper::extensions())
    {
        tracing::error!(?error, "agentos-sidecar startup failed");
        std::process::exit(1);
    }
}
