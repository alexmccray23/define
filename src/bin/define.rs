use anyhow::{Result, bail};
use std::env;
use std::fmt::Write;

use define::{fetch_definition, get_word, send_notification};

// https://dictionaryapi.com/api/v3/references/collegiate/json/test?key=${DICTIONARY_API_KEY}
const DICTIONARY_BASE_URL: &str = "https://dictionaryapi.com/api/v3/references/collegiate/json";

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = env!("DICTIONARY_API_KEY");

    // Get word from command line argument or clipboard
    let word = get_word().await?;

    // Validate input
    if word.trim().is_empty() || word.contains('/') {
        bail!("Invalid input");
    }

    // Fetch definitions
    let entries = fetch_definition(&word, api_key, DICTIONARY_BASE_URL).await?;

    // Format all definitions for display
    let mut definitions = String::new();
    for entry in &entries {
        writeln!(
            definitions,
            "\n{}: {}",
            entry.fl(),
            entry.format_definitions()
        )?;
    }

    // Send notification
    send_notification(&entries[0].word(), &definitions).await?;

    Ok(())
}
