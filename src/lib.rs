use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::fmt::Write;

// ============================================================================
// Core Dictionary Types
// ============================================================================

/// A single dictionary entry from the Merriam-Webster API
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct DictEntry {
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
    uros: Option<Vec<UndefinedRunOn>>,

    /// Etymology
    #[serde(default)]
    et: Option<Vec<Vec<Value>>>,

    /// Etymology
    #[serde(default)]
    target: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Meta {
    id: String,
    uuid: String,

    /// Alphabetical sort key
    #[serde(default)]
    sort: Option<String>,

    /// Source reference
    src: String,

    /// All searchable forms of this word
    stems: Vec<String>,

    /// Synonyms
    #[serde(default)]
    syns: Option<Vec<Vec<String>>>,

    /// Antonyms
    #[serde(default)]
    ants: Option<Vec<Vec<String>>>,

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
    prs: Option<Vec<Pronunciation>>,
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
    /// Optional grammatical label dividing verb senses (e.g., "transitive verb")
    #[serde(default)]
    vd: Option<String>,

    /// Sense sequences: sseq[group][item] = [`type_tag`, `sense_data`]
    /// `type_tag` is one of: "sense", "bs" (binding substitute), "pseq" (parenthetical seq)
    sseq: Vec<Vec<Vec<Value>>>,
}

impl DefSection {
    /// Extract all plain-text definitions from this section's sense sequences.
    fn definitions(&self) -> Vec<String> {
        let mut defs = Vec::new();
        for sense_seq in &self.sseq {
            for sense_item in sense_seq {
                collect_sense_defs(sense_item, &mut defs);
            }
        }
        defs
    }
}

/// Recursively collect definition strings from a `[type_tag, data]` sense item.
fn collect_sense_defs(sense_item: &[Value], defs: &mut Vec<String>) {
    if sense_item.len() < 2 {
        return;
    }
    match sense_item[0].as_str() {
        Some("sense") => {
            if let Some(text) = extract_dt_text(&sense_item[1]) {
                defs.push(text);
            }
        }
        Some("bs") => {
            // Binding substitute: { "sense": { "sn": ..., "dt": [...] } }
            if let Some(inner) = sense_item[1].get("sense")
                && let Some(text) = extract_dt_text(inner)
            {
                defs.push(text);
            }
        }
        Some("pseq") => {
            // Parenthetical sense sequence: nested Vec<Vec<[tag, data]>>
            if let Some(groups) = sense_item[1].as_array() {
                for group in groups {
                    if let Some(items) = group.as_array() {
                        for item in items {
                            if let Some(arr) = item.as_array() {
                                collect_sense_defs(arr, defs);
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

/// Pull the first "text" entry out of a sense's `dt` array and strip MW markup.
fn extract_dt_text(sense_data: &Value) -> Option<String> {
    let dt = sense_data.get("dt")?.as_array()?;
    for item in dt {
        let pair = item.as_array()?;
        if pair.first().and_then(Value::as_str) == Some("text")
            && let Some(raw) = pair.get(1).and_then(Value::as_str)
        {
            return Some(strip_mw_markup(raw));
        }
    }
    None
}

/// Strip Merriam-Webster inline markup tokens from definition text.
///
/// Handles tokens like `{bc}` (bold colon → ": "), `{it}`/`{/it}` (italic markers,
/// removed), `{ldquo}`/`{rdquo}` (typographic quotes), and cross-reference tokens
/// like `{sx|word||}` (synonym cross-reference → the word itself).
fn strip_mw_markup(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '{' {
            result.push(ch);
            continue;
        }
        let mut tag = String::new();
        for c in chars.by_ref() {
            if c == '}' {
                break;
            }
            tag.push(c);
        }
        match tag.as_str() {
            "ldquo" => result.push('\u{201C}'),
            "rdquo" => result.push('\u{201D}'),
            // Formatting markers with no plain-text equivalent
            "it" | "/it" | "b" | "/b" | "sc" | "/sc" | "inf" | "/inf" | "sup" | "/sup" | "bc" => {}
            // Cross-reference tokens: {type|display_word|...} → display_word
            _ => {
                if let Some(word) = tag.split('|').nth(1) {
                    result.push_str(word);
                }
                // Unknown bare tags (no '|') are silently dropped
            }
        }
    }
    result
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
    /// Get a formatted string of all definitions for display.
    ///
    /// Prefers the full `def` structure (which contains all senses) over `shortdef`
    /// (which is capped at three). Falls back to `shortdef` if `def` yields nothing.
    #[must_use]
    pub fn format_definitions(&self) -> String {
        // Collect from full def sections first
        let from_def: Vec<String> = self.def.iter().flat_map(DefSection::definitions).collect();

        let defs: &[String] = if !from_def.is_empty() {
            &from_def
        } else if !self.shortdef.is_empty() {
            &self.shortdef
        } else {
            return String::from("(No definitions available)");
        };

        let mut result = String::new();
        for def in defs {
            result.push('\n');
            write!(result, "• {def}").unwrap();
        }
        result
    }

    /// Get a formatted string of all synonyms for display
    #[must_use]
    pub fn format_synonyms(&self) -> String {
        if let Some(ref syns) = self.meta.syns
            && !syns.is_empty()
        {
            let mut result = String::new();
            for (i, syn) in syns[0].iter().enumerate() {
                if i == 0 {
                    write!(result, "{syn}",).unwrap();
                } else {
                    write!(result, ", {syn}").unwrap();
                }
            }
            result
        } else {
            String::from("(No synonyms available)")
        }
    }

    /// Get a formatted string of all antonyms for display
    #[must_use]
    pub fn format_antonyms(&self) -> String {
        if let Some(ref ants) = self.meta.ants
            && !ants.is_empty()
        {
            let mut result = String::new();
            for (i, ant) in ants[0].iter().enumerate() {
                if i == 0 {
                    write!(result, "{ant}",).unwrap();
                } else {
                    write!(result, ", {ant}").unwrap();
                }
            }
            result
        } else {
            String::from("(No antonyms available)")
        }
    }

    /// Returns dictionary word
    #[must_use]
    pub fn word(&self) -> String {
        self.hwi.hw.replace('*', "")
    }
    /// Returns functional label/part of speech, (e.g., noun, verb, etc)
    #[must_use]
    pub fn fl(&self) -> String {
        self.fl.clone()
    }
}

// ============================================================================
// API Client
// ============================================================================

/// Fetch dictionary entries from Merriam-Webster API
///
/// # Errors
///
/// This function will return an error if:
/// - The dictionary API is unavailable or the API request fails
/// - The received JSON data could not be parsed into Vec<DictEntry>
pub async fn fetch_definition(word: &str, api_key: &str, api_url: &str) -> Result<Vec<DictEntry>> {
    let url = format!("{api_url}/{word}?key={api_key}");

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
                && !suggestions.is_empty()
            {
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
///
/// # Errors
///
/// This function will return an error if:
/// - No word was provided and the clipboard is empty
pub async fn get_word() -> Result<String> {
    // Try command line argument first
    if let Some(word) = env::args().nth(1) {
        return Ok(word);
    }

    // Try wl-paste (Wayland clipboard - Ctrl+C/Ctrl+V)
    let clipboard = tokio::process::Command::new("wl-paste").output().await;

    if let Some(word) = clipboard
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
    {
        return Ok(word.trim().to_string());
    }

    // Try wl-paste -p (Wayland primary selection - text selection/highlight)
    let primary = tokio::process::Command::new("wl-paste")
        .arg("-p")
        .output()
        .await;

    if let Some(word) = primary
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
    {
        return Ok(word.trim().to_string());
    }

    // Fallback to xclip for X11 primary selection
    let xclip = tokio::process::Command::new("xclip")
        .args(["-o", "-selection", "primary"])
        .output()
        .await;

    let word = xclip
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .context("No word provided and clipboard is empty")?;

    Ok(word.trim().to_string())
}

// ============================================================================
// Notification
// ============================================================================

/// Send desktop notification with the definition
///
/// # Errors
///
/// This function will return an error if:
/// - The `notify-send` command is unavailable or fails to run.
pub async fn send_notification(word: &str, definitions: &str) -> Result<()> {
    let status = tokio::process::Command::new("notify-send")
        .args([
            "-t",
            "60000", // 60 second timeout
            word,
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
