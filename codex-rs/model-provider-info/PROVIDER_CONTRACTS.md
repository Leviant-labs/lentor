# Provider Contract Interfaces

The provider foundation freezes these Rust interfaces for the authentication, transport, and account work:

- `codex_protocol::provider::ProviderKind`
- `codex_protocol::provider::AccountServices`
- `codex_protocol::provider::CommandCodeAuthSource`
- `codex_protocol::provider::CommandCodeRoute`
- `codex_protocol::provider::ProviderCapabilities`
- `codex_model_provider_info::ProviderContract`

`ProviderContract` is stored at `ModelProviderCapabilities::contract`. The registry key and `ModelProviderInfo::base_url` provide provider identity and endpoint identity. For `ProviderKind::CommandCode`, the key must be `commandcode`, the base URL must be `https://api.commandcode.ai/provider/v1`, and the capability block must declare one auth source and at least one unique model route.

Example configuration:

```toml
[model_providers.commandcode]
name = "CommandCode"
base_url = "https://api.commandcode.ai/provider/v1"

[model_providers.commandcode.capabilities.contract]
kind = "command_code"

[model_providers.commandcode.capabilities.contract.capabilities]
account_services = "disabled"
command_code_auth_source = "api_key"
command_code_model_routes = ["responses", "chat_completions", "anthropic_messages"]
```

The auth source is non-secret metadata. API keys and OAuth credentials must remain in the provider-scoped keyring store implemented by the authentication layer. Model routes describe the routes discovery may select; live `supported_endpoints` metadata remains authoritative for each model.
