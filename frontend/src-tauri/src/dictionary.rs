// dictionary.rs
//
// User dictionary for transcription fixes: names, companies, medications, or
// any word the user pronounces differently. Entries are either a correction
// pair (misheard -> correct, applied deterministically to every transcribed
// segment) or a plain vocabulary term (correct only, used to bias Whisper via
// its initial prompt and to guide the dictation cleanup LLM).
//
// Entries come from two places: manual adds in Preferences, and automatic
// extraction when the user edits a transcript segment in meeting details.

use log::{info, warn};
use regex::RegexBuilder;
use serde::{Deserialize, Serialize};
use std::sync::RwLock;
use tauri::{AppHandle, Runtime};
use tauri_plugin_store::StoreExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DictionaryEntry {
    pub id: String,
    /// What the transcription engine typically produces (None for plain
    /// vocabulary terms that only bias recognition/cleanup).
    #[serde(default)]
    pub misheard: Option<String>,
    /// The correct spelling the user wants.
    pub correct: String,
}

/// Compiled view of the dictionary, cached so the transcription hot path never
/// touches the store or recompiles regexes.
struct CompiledDictionary {
    entries: Vec<DictionaryEntry>,
    /// (whole-word case-insensitive matcher for `misheard`, replacement)
    corrections: Vec<(regex::Regex, String)>,
}

static DICTIONARY: RwLock<Option<CompiledDictionary>> = RwLock::new(None);

const DICTIONARY_STORE: &str = "dictionary.json";

fn compile(entries: Vec<DictionaryEntry>) -> CompiledDictionary {
    let corrections = entries
        .iter()
        .filter_map(|entry| {
            let misheard = entry.misheard.as_deref()?.trim();
            if misheard.is_empty() {
                return None;
            }
            // Whole-word, case-insensitive. \b is Unicode-aware, so this works
            // for Persian words too.
            let pattern = format!(r"\b{}\b", regex::escape(misheard));
            match RegexBuilder::new(&pattern).case_insensitive(true).build() {
                Ok(re) => Some((re, entry.correct.clone())),
                Err(e) => {
                    warn!("Failed to compile dictionary pattern '{}': {}", misheard, e);
                    None
                }
            }
        })
        .collect();

    CompiledDictionary { entries, corrections }
}

fn load_entries<R: Runtime>(app: &AppHandle<R>) -> Vec<DictionaryEntry> {
    let store = match app.store(DICTIONARY_STORE) {
        Ok(store) => store,
        Err(e) => {
            warn!("Failed to access dictionary store: {}", e);
            return Vec::new();
        }
    };

    store
        .get("entries")
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

fn save_entries<R: Runtime>(app: &AppHandle<R>, entries: &[DictionaryEntry]) -> Result<(), String> {
    let store = app
        .store(DICTIONARY_STORE)
        .map_err(|e| format!("Failed to access dictionary store: {}", e))?;

    let value = serde_json::to_value(entries)
        .map_err(|e| format!("Failed to serialize dictionary: {}", e))?;
    store.set("entries", value);
    store
        .save()
        .map_err(|e| format!("Failed to save dictionary: {}", e))?;

    // Refresh the compiled cache so the change applies immediately.
    if let Ok(mut cache) = DICTIONARY.write() {
        *cache = Some(compile(entries.to_vec()));
    }
    Ok(())
}

/// Load the dictionary into the in-memory cache. Called once at app startup;
/// mutations refresh the cache themselves.
pub fn init<R: Runtime>(app: &AppHandle<R>) {
    let entries = load_entries(app);
    info!("Loaded user dictionary with {} entries", entries.len());
    if let Ok(mut cache) = DICTIONARY.write() {
        *cache = Some(compile(entries));
    }
}

/// Apply all correction pairs to a transcribed segment (whole-word,
/// case-insensitive). Cheap: precompiled regexes, no store access.
pub fn apply_corrections(text: &str) -> String {
    let cache = match DICTIONARY.read() {
        Ok(cache) => cache,
        Err(_) => return text.to_string(),
    };
    let Some(dict) = cache.as_ref() else {
        return text.to_string();
    };

    let mut result = text.to_string();
    for (re, replacement) in &dict.corrections {
        if re.is_match(&result) {
            result = re.replace_all(&result, replacement.as_str()).into_owned();
        }
    }
    result
}

/// Vocabulary hint for Whisper's initial prompt: the correct spellings of all
/// entries, capped so it doesn't eat decoding context. Returns None when the
/// dictionary is empty.
pub fn whisper_vocabulary_hint() -> Option<String> {
    const MAX_TERMS: usize = 40;

    let cache = DICTIONARY.read().ok()?;
    let dict = cache.as_ref()?;

    let terms: Vec<&str> = dict
        .entries
        .iter()
        .map(|e| e.correct.trim())
        .filter(|t| !t.is_empty())
        .take(MAX_TERMS)
        .collect();

    if terms.is_empty() {
        None
    } else {
        Some(format!("Glossary: {}.", terms.join(", ")))
    }
}

/// Dictionary section for the dictation cleanup LLM prompt: exact correction
/// pairs plus preferred spellings, so the model fixes similar-sounding
/// mistranscriptions the deterministic pass can't catch.
pub fn cleanup_prompt_section() -> String {
    let cache = match DICTIONARY.read() {
        Ok(cache) => cache,
        Err(_) => return String::new(),
    };
    let Some(dict) = cache.as_ref() else {
        return String::new();
    };
    if dict.entries.is_empty() {
        return String::new();
    }

    let mut section = String::from("\n\nUser dictionary — apply these known fixes:");

    let corrections: Vec<String> = dict
        .entries
        .iter()
        .filter_map(|e| {
            let m = e.misheard.as_deref()?.trim();
            if m.is_empty() {
                None
            } else {
                Some(format!("\"{}\" -> \"{}\"", m, e.correct))
            }
        })
        .collect();
    if !corrections.is_empty() {
        section.push_str("\nCorrections (also apply when the text is merely similar-sounding): ");
        section.push_str(&corrections.join("; "));
    }

    let vocabulary: Vec<&str> = dict
        .entries
        .iter()
        .filter(|e| e.misheard.as_deref().map_or(true, |m| m.trim().is_empty()))
        .map(|e| e.correct.trim())
        .filter(|t| !t.is_empty())
        .collect();
    if !vocabulary.is_empty() {
        section.push_str("\nPreferred spellings of names and terms the speaker uses: ");
        section.push_str(&vocabulary.join(", "));
    }

    section
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn get_dictionary_entries<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<DictionaryEntry>, String> {
    Ok(load_entries(&app))
}

#[tauri::command]
pub async fn add_dictionary_entry<R: Runtime>(
    app: AppHandle<R>,
    misheard: Option<String>,
    correct: String,
) -> Result<DictionaryEntry, String> {
    let correct = correct.trim().to_string();
    if correct.is_empty() {
        return Err("Correct form cannot be empty".to_string());
    }
    let misheard = misheard
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    // Identical misheard->correct pairs would be typed twice by the corrector;
    // treat re-adding as a no-op and return the existing entry.
    let mut entries = load_entries(&app);
    if let Some(existing) = entries.iter().find(|e| {
        e.correct.eq_ignore_ascii_case(&correct)
            && match (&e.misheard, &misheard) {
                (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
                (None, None) => true,
                _ => false,
            }
    }) {
        return Ok(existing.clone());
    }

    let entry = DictionaryEntry {
        id: format!("dict-{}", uuid::Uuid::new_v4()),
        misheard,
        correct,
    };
    entries.push(entry.clone());
    save_entries(&app, &entries)?;

    info!(
        "Added dictionary entry: {:?} -> '{}'",
        entry.misheard, entry.correct
    );
    Ok(entry)
}

#[tauri::command]
pub async fn update_dictionary_entry<R: Runtime>(
    app: AppHandle<R>,
    entry: DictionaryEntry,
) -> Result<(), String> {
    let mut entries = load_entries(&app);
    let Some(existing) = entries.iter_mut().find(|e| e.id == entry.id) else {
        return Err(format!("Dictionary entry not found: {}", entry.id));
    };
    *existing = entry;
    save_entries(&app, &entries)
}

#[tauri::command]
pub async fn delete_dictionary_entry<R: Runtime>(
    app: AppHandle<R>,
    id: String,
) -> Result<(), String> {
    let mut entries = load_entries(&app);
    entries.retain(|e| e.id != id);
    save_entries(&app, &entries)
}
