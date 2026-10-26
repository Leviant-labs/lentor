use crate::JsonSchema;
use crate::TS;
use codex_utils_redacted_string::RedactedString;
use serde::Deserialize;
use serde::Serialize;

/// Configures an API-key provider without persisting its secret in config.toml.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ModelProviderConfigureParams {
    pub provider_id: String,
    pub name: String,
    pub base_url: String,
    #[ts(type = "string")]
    pub api_key: RedactedString,
    #[ts(optional = nullable)]
    pub model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct ModelProviderConfigureResponse {
    pub provider_id: String,
    pub base_url: String,
    pub model: Option<String>,
}

#[cfg(test)]
#[path = "model_provider_tests.rs"]
mod tests;
