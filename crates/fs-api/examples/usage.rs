use fs_api::Client;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("FS_BASE")?;
    let key = std::env::var("FS_KEY")?;
    let client = Client::new(&base, key.trim())?;

    let usage: serde_json::Value = client.get("/user/usage").await?;
    println!("{}", serde_json::to_string_pretty(&usage)?);
    Ok(())
}
