## Why

Codex currently treats OpenAI/ChatGPT services and identity as part of its default runtime, while the intended baseline is a provider-independent Rust agent that can use CommandCode directly. The project also needs a durable, reviewable specification and a clear boundary between the stable plugin package format, independent plugin marketplaces, ChatGPT Apps, and optional Codex account services.

## What Changes

- Add a direct Rust CommandCode provider for the documented Provider API and CommandCode OAuth/API-key authentication. No local OpenCodex, LiteLLM, or other proxy is required.
- Select model transport from CommandCode model capabilities: OpenAI Responses, OpenAI Chat Completions, or Anthropic Messages.
- Make CommandCode the default provider only when no explicit provider is configured; remove implicit OpenAI fallback and default OpenAI service traffic.
- Preserve the Codex plugin manifest/package format and independent plugin marketplaces unchanged. Remove ChatGPT Apps/connector execution while keeping `paths.apps` syntax compatible and inert.
- Keep Codex/ChatGPT login, usage, limits, and credits as optional provider-scoped account services. They are disabled unless explicitly selected and never block CommandCode operation.
- Preserve the core webapp/app-server thread workflow while removing ChatGPT Apps and making optional account services capability-gated.
- Initialize and complete a strict OpenSpec change in the Git repository before Rust implementation begins.

## Capabilities

### New Capabilities

- `commandcode-provider`: Direct CommandCode discovery, authentication, transport selection, streaming, tools, usage, and errors without a proxy.
- `provider-auth`: Provider-scoped API-key and OAuth credential handling with safe storage and validation.
- `plugin-runtime`: Stable plugin format/marketplace behavior with ChatGPT Apps disabled.
- `optional-account-services`: Opt-in Codex/ChatGPT account, usage, limits, and credits services that are absent from the default runtime path.
- `webapp-runtime`: Minimal app-server/thread behavior for webapp use without ChatGPT Apps or mandatory account services.
- `rust-project-governance`: Git-backed, OpenSpec-validated, Rust-first implementation governance for the fork.

### Modified Capabilities

None. This is the initial OpenSpec baseline for the project.

## Impact

- Rust crates for provider configuration, model transport, authentication, plugin loading, app-server, CLI, and TUI.
- Provider configuration and schema: `model_provider`, `model_providers`, model discovery, and account capability settings.
- Wire adapters and request/stream types for CommandCode's `/provider/v1` endpoints and `/alpha/whoami` authentication validation.
- Plugin manifest and marketplace implementation remains structurally compatible; connector/app execution paths become inert.
- App-server v2 capability responses and schema fixtures change where ChatGPT Apps or account services are exposed.
- The repository receives OpenSpec artifacts, Rust-facing acceptance tests, and Git-tracked implementation work under `codex-upstream`.
