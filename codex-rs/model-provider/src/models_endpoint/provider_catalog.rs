use codex_models_manager::model_info::model_info_from_slug;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use serde_json::Value;
use std::collections::HashSet;

const MAX_MODELS: usize = 1024;

pub(super) fn parse(bytes: &[u8]) -> Result<Vec<ModelInfo>> {
    let invalid = || CodexErr::InvalidRequest("provider returned an invalid model catalog".to_string());
    let body: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    let rows = body.get("models").or_else(|| body.get("data"))
        .and_then(Value::as_array).ok_or_else(invalid)?;
    if rows.len() > MAX_MODELS {
        return Err(CodexErr::InvalidRequest("provider catalog exceeds the 1024-model limit".to_string()));
    }
    let mut seen = HashSet::new();
    rows.iter().enumerate().map(|(index, row)| {
        let id = row.get("slug").or_else(|| row.get("id"))
            .and_then(Value::as_str).ok_or_else(invalid)?;
        if id.is_empty() || id.len() > 512 || id.chars().any(char::is_control) || !seen.insert(id) {
            return Err(invalid());
        }
        // Existing full Codex catalogs retain their model metadata contract.
        if let Ok(model) = serde_json::from_value::<ModelInfo>(row.clone()) {
            return Ok(model);
        }
        let mut model = model_info_from_slug(id);
        model.display_name = row.get("display_name").or_else(|| row.get("name"))
            .and_then(Value::as_str).filter(|name| name.len() <= 512)
            .unwrap_or(id).to_string();
        model.description = row.get("description").and_then(Value::as_str)
            .filter(|description| description.len() <= 2048).map(ToOwned::to_owned);
        model.priority = index as i32;
        model.visibility = ModelVisibility::List;
        model.supports_reasoning_summary_parameter = false;
        if let Some(window) = row.get("context_window").and_then(Value::as_i64).filter(|window| *window > 0) {
            model.context_window = Some(window);
            model.max_context_window = Some(window);
        }
        let mut efforts = Vec::new();
        if let Some(levels) = row.get("supported_reasoning_levels").and_then(Value::as_array) {
            if levels.len() > 32 { return Err(invalid()); }
            for level in levels {
                let value = level.get("effort").unwrap_or(level);
                let effort: ReasoningEffort = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
                if effort.as_str().len() > 64 { return Err(invalid()); }
                let description = level.get("description").and_then(Value::as_str)
                    .filter(|description| description.len() <= 256).unwrap_or(effort.as_str()).to_string();
                if !efforts.iter().any(|preset: &ReasoningEffortPreset| preset.effort == effort) {
                    efforts.push(ReasoningEffortPreset { effort, description });
                }
            }
        } else if let Some(options) = row.get("reasoning_options").and_then(Value::as_array) {
            for option in options.iter().filter(|option| option["type"] == "effort") {
                let levels = option.get("values").and_then(Value::as_array).ok_or_else(invalid)?;
                if levels.len() > 32 { return Err(invalid()); }
                for value in levels {
                    let effort: ReasoningEffort = serde_json::from_value(value.clone()).map_err(|_| invalid())?;
                    if effort.as_str().len() > 64 { return Err(invalid()); }
                    if !efforts.iter().any(|preset: &ReasoningEffortPreset| preset.effort == effort) {
                        efforts.push(ReasoningEffortPreset { description: effort.to_string(), effort });
                    }
                }
            }
        }
        model.default_reasoning_level = row.get("default_reasoning_level")
            .and_then(|value| serde_json::from_value::<ReasoningEffort>(value.clone()).ok())
            .filter(|effort| efforts.iter().any(|preset| &preset.effort == effort))
            .or_else(|| efforts.first().map(|preset| preset.effort.clone()));
        model.supported_reasoning_levels = efforts;
        Ok(model)
    }).collect()
}

#[cfg(test)]
#[path = "provider_catalog_tests.rs"]
mod tests;
