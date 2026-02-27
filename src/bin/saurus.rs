use anyhow::{Result, bail};
use std::env;
use std::fmt::Write;

use define::{fetch_definition, get_word, send_notification};

// https://dictionaryapi.com/api/v3/references/thesaurus/json/test?key=${THESAURUS_API_KEY}
const THESAURUS_BASE_URL: &str = "https://dictionaryapi.com/api/v3/references/thesaurus/json";

#[tokio::main]
async fn main() -> Result<()> {
    let api_key = env!("THESAURUS_API_KEY");

    // Get word from command line argument or clipboard
    let word = get_word().await?;

    // Validate input
    if word.trim().is_empty() || word.contains('/') {
        bail!("Invalid input");
    }

    // Fetch definitions
    let entries = fetch_definition(&word, &api_key, THESAURUS_BASE_URL).await?;

    // Format all definitions for display
    let mut definitions = String::new();
    for entry in &entries {
        writeln!(definitions, "\nSynonyms: {}", entry.format_synonyms())?;
        writeln!(definitions, "\nAntonyms: {}", entry.format_antonyms())?;
    }

    // Send notification
    send_notification(&entries[0].word(), &definitions).await?;

    Ok(())
}
