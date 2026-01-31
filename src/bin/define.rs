use anyhow::{Context, Result, bail};
use std::env;
use std::fmt::Write;
use tokio::fs;

use define::{fetch_definition, get_word, send_notification};

// https://dictionaryapi.com/api/v3/references/collegiate/json/test?key=${DICTIONARY_API_KEY}
const DICTIONARY_API_URL: &str = "https://dictionaryapi.com/api/v3/references/collegiate/json";

#[tokio::main]
async fn main() -> Result<()> {
    // Get API key from ~/.env file or environment variable
    let api_key = if let Ok(contents) = fs::read_to_string("/home/alexm/.env").await {
        contents
            .lines()
            .find(|line| line.contains("DICTIONARY_API_KEY="))
            .and_then(|line| line.split_once('='))
            .map(|(_, value)| value.trim().to_string())
            .context("DICTIONARY_API_KEY not found in ~/.env")?
    } else {
        env::var("DICTIONARY_API_KEY")
            .context("DICTIONARY_API_KEY not set in ~/.env or environment")?
    };

    // Get word from command line argument or clipboard
    let word = get_word().await?;

    // Validate input
    if word.trim().is_empty() || word.contains('/') {
        bail!("Invalid input");
    }

    // Fetch definitions
    let entries = fetch_definition(&word, &api_key, DICTIONARY_API_URL).await?;

    // Format all definitions for display
    let mut definitions = String::new();
    for entry in &entries {
        writeln!(
            definitions,
            "\n{}{}",
            entry.fl(),
            entry.format_definitions()
        )?;
    }

    // Send notification
    send_notification(&entries[0].word(), &definitions).await?;

    Ok(())
}
