// https://dictionaryapi.com/api/v3/references/collegiate/json/test?key=${DICTIONARY_API_KEY}

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fmt::Write as _;

// ============================================================================
// Core Dictionary Types
// ============================================================================

/// A single dictionary entry from the Merriam-Webster API
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DictEntry {
    /// Entry metadata (id, offensive flag, etc.)
    meta: Meta,

    /// Headword information (the word itself, pronunciation)
    hwi: Hwi,

    /// Functional label (part of speech: noun, verb, adjective, etc.)
    fl: String,

    /// Simple array of up to 3 definitions (easiest to use)
    #[serde(default)]
    shortdef: Vec<String>,

    /// Full definition structure (complex nested format)
    /// Reserved for future use when we need examples, sense numbers, etc.
    #[serde(default)]
    def: Vec<DefSection>,

    /// Undefined run-ons (related words derived from main entry)
    #[serde(default)]
    uros: Vec<UndefinedRunOn>,

    /// Etymology
    #[serde(default)]
    et: Vec<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Meta {
    id: String,
    uuid: String,

    /// Alphabetical sort key
    sort: String,

    /// Source dictionary
    src: String,

    /// All searchable forms of this word
    stems: Vec<String>,

    /// Whether this entry contains offensive content
    offensive: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Hwi {
    /// Headword with syllable markers (e.g., "mer*cu*ri*al")
    hw: String,

    /// Pronunciations (Merriam-Webster format + audio files)
    #[serde(default)]
    prs: Vec<Pronunciation>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Pronunciation {
    /// Merriam-Webster phonetic notation
    mw: String,

    /// Sound file information
    #[serde(default)]
    sound: Option<Sound>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Sound {
    audio: String,
    #[serde(rename = "ref")]
    ref_field: String,
    stat: String,
}

/// Full definition section (complex nested structure)
/// The actual definitions are in sseq (sense sequences)
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct DefSection {
    /// Nested array: [`sense_group`][sense_item][type, data]
    /// Example: [[[["sense", {dt: [...]}]]]]
    sseq: Vec<Vec<Vec<Value>>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct UndefinedRunOn {
    /// The run-on entry text (e.g., "mercurially")
    ure: String,

    /// Part of speech for the run-on
    fl: String,
}

// ============================================================================
// Display & Formatting
// ============================================================================

impl DictEntry {
    /// Get a formatted string of all definitions for display
    /// Currently uses shortdef, but can be extended to parse full def structure
    fn format_definitions(&self) -> String {
        if self.shortdef.is_empty() {
            // Fallback: could implement full def parsing here in the future
            String::from("(No definitions available)")
        } else {
            // Simple format using shortdef - use write! to avoid intermediate allocations
            let mut result = String::new();
            for (i, def) in self.shortdef.iter().enumerate() {
                if i > 0 {
                    result.push('\n');
                }
                write!(result, ". {def}").unwrap();
            }
            result
        }
    }

    /// Get the clean word without syllable markers
    fn word(&self) -> String {
        self.hwi.hw.replace('*', "")
    }
}

// ============================================================================
// API Client
// ============================================================================

const API_BASE_URL: &str = "https://dictionaryapi.com/api/v3/references/collegiate/json";

/// Fetch dictionary entries from Merriam-Webster API
async fn fetch_definition(word: &str, api_key: &str) -> Result<Vec<DictEntry>> {
    let url = format!("{API_BASE_URL}/{word}?key={api_key}");

    let response = reqwest::Client::new()
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("Failed to connect to dictionary API")?;

    if !response.status().is_success() {
        bail!("API request failed with status: {}", response.status());
    }

    let body = response.text().await?;

    // The API returns an array - could be DictEntry objects or just strings (suggestions)
    // First, try to parse as DictEntry array
    match serde_json::from_str::<Vec<DictEntry>>(&body) {
        Ok(entries) if !entries.is_empty() => Ok(entries),
        Ok(_) => bail!("No definitions found for '{word}'"),
        Err(_) => {
            // Might be an array of suggestions (strings)
            if let Ok(suggestions) = serde_json::from_str::<Vec<String>>(&body)
                && !suggestions.is_empty() {
                    bail!(
                        "No definitions found. Did you mean: {}?",
                        suggestions.join(", ")
                    );
                }
            bail!("Invalid word or unexpected API response")
        }
    }
}

// ============================================================================
// Clipboard Integration
// ============================================================================

/// Get word from command line args or clipboard
async fn get_word() -> Result<String> {
    // Try command line argument first
    // if let Some(word) = env::args().nth(1) {
    //     return Ok(word);
    // }

    // Fall back to clipboard (wl-paste for Wayland, xclip for X11)
    let word = tokio::process::Command::new("wl-paste")
        .output()
        .await
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .or_else(|| {
            // Fallback to xclip for X11
            std::process::Command::new("xclip")
                .args(["-o", "-selection", "primary"])
                .output()
                .ok()
                .filter(|output| output.status.success())
                .and_then(|output| String::from_utf8(output.stdout).ok())
        })
        .context("No word provided and clipboard is empty")?;

    Ok(word.trim().to_string())
}

// ============================================================================
// Notification
// ============================================================================

/// Send desktop notification with the definition
async fn send_notification(word: &str, definitions: &str) -> Result<()> {
    let status = tokio::process::Command::new("notify-send")
        .args([
            "-t", "60000",  // 60 second timeout
            &format!("{word} -"),
            definitions,
        ])
        .status()
        .await
        .context("Failed to execute notify-send")?;

    if !status.success() {
        bail!("notify-send command failed");
    }

    Ok(())
}

// ============================================================================
// Main
// ============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Get API key from environment
    let api_key = env::var("DICTIONARY_API_KEY")
        .context("DICTIONARY_API_KEY environment variable not set")?;

    // Get word from command line argument or clipboard
    let word = get_word().await?;

    // Validate input
    if word.trim().is_empty() || word.contains('/') {
        bail!("Invalid input");
    }

    // Fetch definitions
    let entries = fetch_definition(&word, &api_key).await?;

    // Format all definitions for display
    let mut definitions = String::new();
    for entry in &entries {
        writeln!(definitions, "\n{}{}", entry.fl, entry.format_definitions())?;
    }

    // Send notification
    send_notification(&entries[0].word(), &definitions).await?;

    // Also print to stdout for debugging/scripting
    println!("{} -{}", entries[0].word(), definitions);

    Ok(())
}
