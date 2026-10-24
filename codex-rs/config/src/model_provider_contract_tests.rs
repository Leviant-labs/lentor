use super::*;
use codex_model_provider_info::AccountServices;
use codex_model_provider_info::COMMAND_CODE_PROVIDER_BASE_URL;
use codex_model_provider_info::CommandCodeAuthSource;
use codex_model_provider_info::CommandCodeRoute;
use codex_model_provider_info::ModelProviderCapabilities;
use codex_model_provider_info::ProviderCapabilities;
use codex_model_provider_info::ProviderContract;
use codex_model_provider_info::ProviderKind;
use pretty_assertions::assert_eq;

fn command_code_provider() -> ModelProviderInfo {
    ModelProviderInfo {
        name: "CommandCode".to_string(),
        base_url: Some(COMMAND_CODE_PROVIDER_BASE_URL.to_string()),
        capabilities: Some(ModelProviderCapabilities {
            contract: Some(ProviderContract {
                kind: ProviderKind::CommandCode,
                capabilities: ProviderCapabilities {
                    account_services: AccountServices::Disabled,
                    command_code_auth_source: Some(CommandCodeAuthSource::ApiKey),
                    command_code_model_routes: vec![
                        CommandCodeRoute::Responses,
                        CommandCodeRoute::ChatCompletions,
                        CommandCodeRoute::AnthropicMessages,
                    ],
                },
            }),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn command_code_provider_config_round_trips_as_whole_config() {
    let input = r#"
model_provider = "commandcode"

[model_providers.commandcode]
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"

[model_providers.commandcode.capabilities.contract]
kind = "command_code"

[model_providers.commandcode.capabilities.contract.capabilities]
account_services = "disabled"
command_code_auth_source = "api_key"
command_code_model_routes = ["responses", "chat_completions", "anthropic_messages"]
"#;

    let parsed = toml::from_str::<ConfigToml>(input).expect("provider config should parse");
    assert_eq!(
        parsed.model_provider.as_deref(),
        Some(COMMAND_CODE_PROVIDER_ID)
    );
    assert_eq!(
        parsed.model_providers.get(COMMAND_CODE_PROVIDER_ID),
        Some(&command_code_provider())
    );

    let encoded = toml::to_string(&parsed).expect("provider config should serialize");
    assert_eq!(
        toml::from_str::<ConfigToml>(&encoded).expect("provider config should round-trip"),
        parsed
    );
}

#[test]
fn reserved_provider_ids_cannot_be_overridden() {
    for provider_id in RESERVED_MODEL_PROVIDER_IDS
        .into_iter()
        .filter(|provider_id| {
            !matches!(
                *provider_id,
                AMAZON_BEDROCK_PROVIDER_ID | AMAZON_BEDROCK_RUNTIME_PROVIDER_ID
            )
        })
    {
        let input = format!(
            r#"
[model_providers.{provider_id}]
name = "Override"
base_url = "https://example.invalid/v1"
"#
        );
        let error = toml::from_str::<ConfigToml>(&input)
            .expect_err("reserved provider override should be rejected");
        assert!(error.to_string().contains(&format!("`{provider_id}`")));
    }
}

#[test]
fn command_code_contract_rejects_non_direct_base_url() {
    let mut provider = command_code_provider();
    provider.base_url = Some("https://example.invalid/provider/v1".to_string());

    let error = validate_model_providers(
        &[(COMMAND_CODE_PROVIDER_ID.to_string(), provider)]
            .into_iter()
            .collect(),
    )
    .expect_err("non-direct base URL should be rejected");
    assert_eq!(
        error,
        "model_providers.commandcode: provider contract base_url must be \
`https://api.commandcode.ai/provider/v1`"
    );
}

#[test]
fn command_code_contract_rejects_duplicate_routes() {
    let mut provider = command_code_provider();
    let Some(capabilities) = provider.capabilities.as_mut() else {
        panic!("provider should have capabilities");
    };
    let Some(contract) = capabilities.contract.as_mut() else {
        panic!("provider should have a contract");
    };
    contract
        .capabilities
        .command_code_model_routes
        .push(CommandCodeRoute::Responses);

    let error = validate_model_providers(
        &[(COMMAND_CODE_PROVIDER_ID.to_string(), provider)]
            .into_iter()
            .collect(),
    )
    .expect_err("duplicate route should be rejected");
    assert_eq!(
        error,
        "model_providers.commandcode: command_code_model_routes must not contain duplicates"
    );
}
