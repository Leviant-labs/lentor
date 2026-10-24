//! Provider identities and optional runtime capabilities shared across crates.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;

/// Built-in provider family represented by a model-provider registry entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    CommandCode,
    #[serde(rename = "openai")]
    OpenAi,
    AmazonBedrock,
    AmazonBedrockRuntime,
    Oss,
    #[default]
    Custom,
}

/// Optional Codex or ChatGPT account functionality selected independently of model providers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AccountServices {
    #[default]
    Disabled,
    Codex,
}

/// Credential source selected for CommandCode provider authentication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandCodeAuthSource {
    ApiKey,
    OAuth,
    ImportedCli,
}

/// CommandCode request family selected from a model's supported endpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CommandCodeRoute {
    Responses,
    ChatCompletions,
    AnthropicMessages,
}

/// Runtime capabilities declared by a provider without implementing their behavior.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct ProviderCapabilities {
    /// Account services remain disabled unless explicitly selected.
    #[serde(default)]
    pub account_services: AccountServices,
    /// Credential source used by the CommandCode authentication implementation.
    pub command_code_auth_source: Option<CommandCodeAuthSource>,
    /// Routes that CommandCode discovery may select for the provider's models.
    #[serde(default)]
    pub command_code_model_routes: Vec<CommandCodeRoute>,
}
