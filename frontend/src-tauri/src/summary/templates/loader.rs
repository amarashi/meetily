use super::defaults;
use super::types::Template;
use std::path::PathBuf;
use tracing::{debug, info, warn};
use once_cell::sync::Lazy;
use std::sync::RwLock;

// Global storage for the bundled templates directory path
static BUNDLED_TEMPLATES_DIR: Lazy<RwLock<Option<PathBuf>>> = Lazy::new(|| RwLock::new(None));

/// Set the bundled templates directory path (called once at app startup)
pub fn set_bundled_templates_dir(path: PathBuf) {
    info!("Bundled templates directory set to: {:?}", path);
    if let Ok(mut dir) = BUNDLED_TEMPLATES_DIR.write() {
        *dir = Some(path);
    }
}

/// Get the user's custom templates directory path
///
/// Returns the platform-specific application data directory for custom templates:
/// - macOS: ~/Library/Application Support/Meetily/templates/
/// - Windows: %APPDATA%\Meetily\templates\
/// - Linux: ~/.config/Meetily/templates/
fn get_custom_templates_dir() -> Option<PathBuf> {
    let mut path = dirs::data_dir()?;
    path.push("Meetily");
    path.push("templates");
    Some(path)
}

/// Load a template from the bundled resources directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_bundled_template(template_id: &str) -> Option<String> {
    let bundled_dir = BUNDLED_TEMPLATES_DIR.read().ok()?.clone()?;
    let template_path = bundled_dir.join(format!("{}.json", template_id));

    debug!("Checking for bundled template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded bundled template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No bundled template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load a template from the user's custom templates directory
///
/// # Arguments
/// * `template_id` - Template identifier (without .json extension)
///
/// # Returns
/// The template JSON content if found, None otherwise
fn load_custom_template(template_id: &str) -> Option<String> {
    let custom_dir = get_custom_templates_dir()?;
    let template_path = custom_dir.join(format!("{}.json", template_id));

    debug!("Checking for custom template at: {:?}", template_path);

    match std::fs::read_to_string(&template_path) {
        Ok(content) => {
            info!("Loaded custom template '{}' from {:?}", template_id, template_path);
            Some(content)
        }
        Err(e) => {
            debug!("No custom template '{}' found: {}", template_id, e);
            None
        }
    }
}

/// Load and parse a template by identifier
///
/// This function implements a fallback strategy:
/// 1. Check user's custom templates directory
/// 2. Check bundled resources directory (app templates)
/// 3. Fall back to built-in embedded templates
/// 4. Return error if not found in any location
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup", "standard_meeting")
///
/// # Returns
/// Parsed and validated Template struct
pub fn get_template(template_id: &str) -> Result<Template, String> {
    info!("Loading template: {}", template_id);

    // Try custom template first, then bundled, then built-in
    let json_content = if let Some(custom_content) = load_custom_template(template_id) {
        debug!("Using custom template for '{}'", template_id);
        custom_content
    } else if let Some(bundled_content) = load_bundled_template(template_id) {
        debug!("Using bundled template for '{}'", template_id);
        bundled_content
    } else if let Some(builtin_content) = defaults::get_builtin_template(template_id) {
        debug!("Using built-in template for '{}'", template_id);
        builtin_content.to_string()
    } else {
        return Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ));
    };

    // Parse and validate
    validate_and_parse_template(&json_content)
}

/// Validate a template identifier for use as a file name
///
/// Restricts IDs to `[a-zA-Z0-9_-]` so a template ID can never escape the
/// templates directory (path traversal) or produce an unusable file name.
pub fn validate_template_id(template_id: &str) -> Result<(), String> {
    if template_id.is_empty() {
        return Err("Template ID cannot be empty".to_string());
    }
    if !template_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(format!(
            "Invalid template ID '{}'. Use only letters, digits, '_' and '-'",
            template_id
        ));
    }
    Ok(())
}

/// Whether a default (bundled or built-in) version of this template exists
///
/// Used to decide if deleting a custom template reverts to a default or
/// removes the template entirely.
pub fn has_default_template(template_id: &str) -> bool {
    defaults::get_builtin_template(template_id).is_some()
        || load_bundled_template(template_id).is_some()
}

/// Whether a custom (user-saved) version of this template exists
pub fn custom_template_exists(template_id: &str) -> bool {
    get_custom_templates_dir()
        .map(|dir| dir.join(format!("{}.json", template_id)).exists())
        .unwrap_or(false)
}

/// Load the raw JSON content of a template plus where it came from
///
/// Uses the same precedence as `get_template`: custom > bundled > built-in.
///
/// # Returns
/// `(json_content, source)` where source is "custom", "bundled" or "builtin"
pub fn get_template_raw(template_id: &str) -> Result<(String, &'static str), String> {
    validate_template_id(template_id)?;

    if let Some(content) = load_custom_template(template_id) {
        Ok((content, "custom"))
    } else if let Some(content) = load_bundled_template(template_id) {
        Ok((content, "bundled"))
    } else if let Some(content) = defaults::get_builtin_template(template_id) {
        Ok((content.to_string(), "builtin"))
    } else {
        Err(format!(
            "Template '{}' not found. Available templates: {}",
            template_id,
            list_template_ids().join(", ")
        ))
    }
}

/// Save a template to the user's custom templates directory
///
/// Validates the JSON first; saving over a built-in/bundled ID creates a
/// custom override (the default stays untouched on disk and can be restored
/// by deleting the override).
pub fn save_custom_template(template_id: &str, json_content: &str) -> Result<Template, String> {
    validate_template_id(template_id)?;
    let template = validate_and_parse_template(json_content)?;

    let custom_dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not resolve the user data directory".to_string())?;
    std::fs::create_dir_all(&custom_dir)
        .map_err(|e| format!("Failed to create templates directory: {}", e))?;

    // Re-serialize so files on disk are consistently pretty-printed.
    let pretty = serde_json::to_string_pretty(&template)
        .map_err(|e| format!("Failed to serialize template: {}", e))?;

    let path = custom_dir.join(format!("{}.json", template_id));
    std::fs::write(&path, pretty)
        .map_err(|e| format!("Failed to write template file: {}", e))?;

    info!("Saved custom template '{}' to {:?}", template_id, path);
    Ok(template)
}

/// Delete a template from the user's custom templates directory
///
/// If the ID also has a bundled/built-in default, this reverts the template
/// to that default rather than removing it from the list.
pub fn delete_custom_template(template_id: &str) -> Result<(), String> {
    validate_template_id(template_id)?;

    let custom_dir = get_custom_templates_dir()
        .ok_or_else(|| "Could not resolve the user data directory".to_string())?;
    let path = custom_dir.join(format!("{}.json", template_id));

    if !path.exists() {
        return Err(if has_default_template(template_id) {
            format!("Template '{}' is a default template and cannot be deleted", template_id)
        } else {
            format!("Template '{}' not found", template_id)
        });
    }

    std::fs::remove_file(&path)
        .map_err(|e| format!("Failed to delete template file: {}", e))?;

    info!("Deleted custom template '{}' at {:?}", template_id, path);
    Ok(())
}

/// Validate and parse template JSON
///
/// # Arguments
/// * `json_content` - Raw JSON string
///
/// # Returns
/// Parsed and validated Template struct
pub fn validate_and_parse_template(json_content: &str) -> Result<Template, String> {
    let template: Template = serde_json::from_str(json_content)
        .map_err(|e| format!("Failed to parse template JSON: {}", e))?;

    template.validate()?;

    Ok(template)
}

/// List all available template identifiers
///
/// Returns a combined list of:
/// - Built-in template IDs
/// - Bundled template IDs (from app resources)
/// - Custom template IDs (from user's data directory)
pub fn list_template_ids() -> Vec<String> {
    let mut ids: Vec<String> = defaults::list_builtin_template_ids()
        .into_iter()
        .map(|s| s.to_string())
        .collect();

    // Add bundled templates if directory is set
    if let Ok(bundled_dir_lock) = BUNDLED_TEMPLATES_DIR.read() {
        if let Some(bundled_dir) = bundled_dir_lock.as_ref() {
            if bundled_dir.exists() {
                match std::fs::read_dir(bundled_dir) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            if let Some(filename) = entry.file_name().to_str() {
                                if filename.ends_with(".json") {
                                    let id = filename.trim_end_matches(".json").to_string();
                                    if !ids.contains(&id) {
                                        ids.push(id);
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to read bundled templates directory: {}", e);
                    }
                }
            }
        }
    }

    // Add custom templates if directory exists
    if let Some(custom_dir) = get_custom_templates_dir() {
        if custom_dir.exists() {
            match std::fs::read_dir(&custom_dir) {
                Ok(entries) => {
                    for entry in entries.flatten() {
                        if let Some(filename) = entry.file_name().to_str() {
                            if filename.ends_with(".json") {
                                let id = filename.trim_end_matches(".json").to_string();
                                if !ids.contains(&id) {
                                    ids.push(id);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to read custom templates directory: {}", e);
                }
            }
        }
    }

    ids.sort();
    ids
}

/// List all available templates with their metadata
///
/// Returns a list of (id, name, description) tuples
pub fn list_templates() -> Vec<(String, String, String)> {
    let mut templates = Vec::new();

    for id in list_template_ids() {
        match get_template(&id) {
            Ok(template) => {
                templates.push((id, template.name, template.description));
            }
            Err(e) => {
                warn!("Failed to load template '{}': {}", id, e);
            }
        }
    }

    templates
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_builtin_template() {
        let template = get_template("daily_standup");
        assert!(template.is_ok());

        let template = template.unwrap();
        assert_eq!(template.name, "Daily Standup");
        assert!(!template.sections.is_empty());
    }

    #[test]
    fn test_get_nonexistent_template() {
        let result = get_template("nonexistent_template");
        assert!(result.is_err());
    }

    #[test]
    fn test_list_template_ids() {
        let ids = list_template_ids();
        assert!(ids.contains(&"daily_standup".to_string()));
        assert!(ids.contains(&"standard_meeting".to_string()));
    }

    #[test]
    fn test_validate_invalid_json() {
        let result = validate_and_parse_template("invalid json");
        assert!(result.is_err());
    }
}
