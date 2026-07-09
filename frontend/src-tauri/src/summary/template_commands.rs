use crate::summary::templates;
use serde::{Deserialize, Serialize};
use tauri::Runtime;
use tracing::{info, warn};

/// Template metadata for UI display
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateInfo {
    /// Template identifier (e.g., "daily_standup", "standard_meeting")
    pub id: String,

    /// Display name for the template
    pub name: String,

    /// Brief description of the template's purpose
    pub description: String,
}

/// Detailed template structure for preview/debugging
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateDetails {
    /// Template identifier
    pub id: String,

    /// Display name
    pub name: String,

    /// Description
    pub description: String,

    /// List of section titles in order
    pub sections: Vec<String>,
}

/// Lists all available templates
///
/// Returns templates from both built-in (embedded) and custom (user data directory) sources.
/// Templates are automatically discovered - no code changes needed to add new templates.
///
/// # Returns
/// Vector of TemplateInfo with id, name, and description for each template
#[tauri::command]
pub async fn api_list_templates<R: Runtime>(
    _app: tauri::AppHandle<R>,
) -> Result<Vec<TemplateInfo>, String> {
    info!("api_list_templates called");

    let templates = templates::list_templates();

    let template_infos: Vec<TemplateInfo> = templates
        .into_iter()
        .map(|(id, name, description)| TemplateInfo {
            id,
            name,
            description,
        })
        .collect();

    info!("Found {} available templates", template_infos.len());

    Ok(template_infos)
}

/// Gets detailed information about a specific template
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup")
///
/// # Returns
/// TemplateDetails with full template structure
#[tauri::command]
pub async fn api_get_template_details<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateDetails, String> {
    info!("api_get_template_details called for template_id: {}", template_id);

    let template = templates::get_template(&template_id)?;

    let section_titles: Vec<String> = template
        .sections
        .iter()
        .map(|section| section.title.clone())
        .collect();

    let details = TemplateDetails {
        id: template_id,
        name: template.name,
        description: template.description,
        sections: section_titles,
    };

    info!("Retrieved template details for '{}'", details.name);

    Ok(details)
}

/// Raw template content for the template editor UI
#[derive(Debug, Serialize, Deserialize)]
pub struct TemplateContent {
    /// Template identifier
    pub id: String,

    /// Raw JSON content of the template
    pub content: String,

    /// Where the active version comes from: "custom", "bundled" or "builtin"
    pub source: String,

    /// Whether a bundled/built-in default exists for this ID
    /// (if true, deleting the custom version reverts to the default)
    pub has_default: bool,
}

/// Gets the raw JSON content of a template for editing
///
/// # Arguments
/// * `template_id` - Template identifier (e.g., "daily_standup")
#[tauri::command]
pub async fn api_get_template_content<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<TemplateContent, String> {
    info!("api_get_template_content called for template_id: {}", template_id);

    let (content, source) = templates::get_template_raw(&template_id)?;

    Ok(TemplateContent {
        has_default: templates::has_default_template(&template_id),
        id: template_id,
        content,
        source: source.to_string(),
    })
}

/// Saves a template to the user's custom templates directory
///
/// Validates the JSON first. Saving with a built-in/bundled ID creates a
/// custom override; deleting that override later restores the default.
///
/// # Arguments
/// * `template_id` - Target identifier (letters, digits, '_' and '-' only)
/// * `template_json` - Raw JSON content of the template
///
/// # Returns
/// The saved template's display name
#[tauri::command]
pub async fn api_save_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
    template_json: String,
) -> Result<String, String> {
    info!("api_save_template called for template_id: {}", template_id);

    match templates::save_custom_template(&template_id, &template_json) {
        Ok(template) => {
            info!("Template '{}' saved as '{}'", template.name, template_id);
            Ok(template.name)
        }
        Err(e) => {
            warn!("Failed to save template '{}': {}", template_id, e);
            Err(e)
        }
    }
}

/// Deletes a custom template (or custom override of a default template)
///
/// # Arguments
/// * `template_id` - Template identifier
///
/// # Returns
/// `true` if a bundled/built-in default remains (the template reverted to it),
/// `false` if the template is gone entirely
#[tauri::command]
pub async fn api_delete_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_id: String,
) -> Result<bool, String> {
    info!("api_delete_template called for template_id: {}", template_id);

    templates::delete_custom_template(&template_id)?;

    Ok(templates::has_default_template(&template_id))
}

/// Validates a custom template JSON string
///
/// Useful for template editor UI or validation before saving custom templates
///
/// # Arguments
/// * `template_json` - Raw JSON string of the template
///
/// # Returns
/// Ok(template_name) if valid, Err(error_message) if invalid
#[tauri::command]
pub async fn api_validate_template<R: Runtime>(
    _app: tauri::AppHandle<R>,
    template_json: String,
) -> Result<String, String> {
    info!("api_validate_template called");

    match templates::validate_and_parse_template(&template_json) {
        Ok(template) => {
            info!("Template '{}' validated successfully", template.name);
            Ok(template.name)
        }
        Err(e) => {
            warn!("Template validation failed: {}", e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_templates() {
        // This test requires the templates to be embedded/available
        // In a real test environment, you might want to mock the templates module

        // For now, just verify the function compiles and runs
        // You can expand this with more specific assertions
    }

    #[tokio::test]
    async fn test_validate_template_valid() {
        let valid_json = r#"
        {
            "name": "Test Template",
            "description": "A test template",
            "sections": [
                {
                    "title": "Summary",
                    "instruction": "Provide a summary",
                    "format": "paragraph"
                }
            ]
        }"#;

        // Mock app handle would be needed for actual testing
        // For now, test the validation logic directly
        let result = templates::validate_and_parse_template(valid_json);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_validate_template_invalid() {
        let invalid_json = "invalid json";

        let result = templates::validate_and_parse_template(invalid_json);
        assert!(result.is_err());
    }
}
