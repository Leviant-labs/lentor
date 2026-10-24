//! Provider identity and capability contract parsed with model-provider configuration.

use crate::ModelProviderInfo;
use codex_protocol::provider::CommandCodeRoute;
use codex_protocol::provider::ProviderCapabilities;
use codex_protocol::provider::ProviderKind;
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

pub const COMMAND_CODE_PROVIDER_ID: &str = "commandcode";
pub const COMMAND_CODE_PROVIDER_BASE_URL: &str = "https://api.commandcode.ai/provider/v1";

/// Explicit provider identity and capabilities consumed by later provider implementations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProviderContract {
    /// Provider family used to select the focused implementation.
    pub kind: ProviderKind,
    /// Optional authentication, routing, and account capabilities.
    pub capabilities: ProviderCapabilities,
}

impl ProviderContract {
    /// Rejects contracts that could send CommandCode credentials or requests outside their boundary.
    pub fn validate(&self, provider: &ModelProviderInfo) -> Result<(), String> {
        match self.kind {
            ProviderKind::CommandCode => {
                if provider.base_url.as_deref() != Some(COMMAND_CODE_PROVIDER_BASE_URL) {
                    return Err(format!(
                        "provider contract base_url must be `{COMMAND_CODE_PROVIDER_BASE_URL}`"
                    ));
                }
                validate_command_code_capabilities(&self.capabilities)?;
                validate_command_code_auth_fields(provider)
            }
            ProviderKind::OpenAi
            | ProviderKind::AmazonBedrock
            | ProviderKind::AmazonBedrockRuntime
            | ProviderKind::Oss
            | ProviderKind::Custom => {
                if self.capabilities.command_code_auth_source.is_some()
                    || !self.capabilities.command_code_model_routes.is_empty()
                {
                    return Err(
                        "command_code capabilities require provider kind `command_code`"
                            .to_string(),
                    );
                }
                Ok(())
            }
        }
    }
}

fn validate_command_code_capabilities(capabilities: &ProviderCapabilities) -> Result<(), String> {
    if capabilities.command_code_auth_source.is_none() {
        return Err("command_code_auth_source must be set".to_string());
    }
    if capabilities.command_code_model_routes.is_empty() {
        return Err("command_code_model_routes must not be empty".to_string());
    }

    let mut routes = capabilities.command_code_model_routes.clone();
    routes.sort_unstable_by_key(|route| match route {
        CommandCodeRoute::Responses => 0,
        CommandCodeRoute::ChatCompletions => 1,
        CommandCodeRoute::AnthropicMessages => 2,
    });
    routes.dedup();
    if routes.len() != capabilities.command_code_model_routes.len() {
        return Err("command_code_model_routes must not contain duplicates".to_string());
    }
    Ok(())
}

fn validate_command_code_auth_fields(provider: &ModelProviderInfo) -> Result<(), String> {
    let mut conflicts = Vec::new();
    if provider.experimental_bearer_token.is_some() {
        conflicts.push("experimental_bearer_token");
    }
    if provider.auth.is_some() {
        conflicts.push("auth");
    }
    if provider.gateway_oauth.is_some() {
        conflicts.push("gateway_oauth");
    }
    if provider.aws.is_some() {
        conflicts.push("aws");
    }
    if provider.requires_openai_auth {
        conflicts.push("requires_openai_auth");
    }

    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "command_code provider cannot be combined with {}",
            conflicts.join(", ")
        ))
    }
}
