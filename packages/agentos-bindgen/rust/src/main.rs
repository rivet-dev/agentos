fn main() -> anyhow::Result<()> {
    let contract = agentos_actor::contract::export();
    serde_json::to_writer_pretty(std::io::stdout(), &contract)?;
    println!();
    Ok(())
}
