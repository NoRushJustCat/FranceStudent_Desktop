use fs_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("FS_BASE")?;
    let key = std::env::var("FS_KEY")?;
    let client = Client::new(&base, key.trim())?;

    for a in client.agents().await? {
        println!(
            "{:>6}  {:<26} {:<10} ctx={:?} compact={:?}{}",
            a.id,
            a.name,
            a.model_provider,
            a.context_window,
            a.compact_threshold,
            if a.is_default { "  [défaut]" } else { "" }
        );
    }
    Ok(())
}
