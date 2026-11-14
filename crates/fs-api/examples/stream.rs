// Usage : cargo run -p fs-api --example stream -- <agentId> "<message>"
// Défaut : agent -2003 (Claude Sonnet 5.5), message "Salut".
use fs_api::{ChatEvent, Client};
use std::io::Write;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let base = std::env::var("FS_BASE")?;
    let key = std::env::var("FS_KEY")?;
    let client = Client::new(&base, key.trim())?;

    let mut args = std::env::args().skip(1);
    let agent: i64 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(-2003);
    let prompt = args.next().unwrap_or_else(|| "Salut".to_string());

    client
        .stream_chat(agent, &prompt, &[], |ev| match ev {
            ChatEvent::Opened { chat_id } => eprintln!("[ouvert] chat {chat_id}"),
            ChatEvent::Delta(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            ChatEvent::Completed { chat_id, message } => {
                println!();
                eprintln!("[fini] chat {chat_id}, message {}", message.id);
                eprintln!("[tokens] {:?}", message.token_usage);
            }
            ChatEvent::Error(e) => eprintln!("[erreur] {e}"),
            _ => {}
        })
        .await?;
    Ok(())
}
