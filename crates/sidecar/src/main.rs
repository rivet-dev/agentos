mod stdio;

fn main() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .init();
    if let Err(error) = stdio::run() {
        tracing::error!(?error, "agent-os-sidecar startup failed");
        std::process::exit(1);
    }
}
