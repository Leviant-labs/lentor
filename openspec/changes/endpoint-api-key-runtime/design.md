## Context

The current fork contains unintegrated branches for provider contracts, plugin isolation, CommandCode authentication, and CommandCode discovery. The active first-release workflow is endpoint/API-key configuration inside the app/TUI, authenticated `/models` discovery, model selection in the TUI/app, and per-model reasoning-level controls using Responses inference.

Source inspection on 2026-10-26 confirms:

- `ModelProviderInfo` already exposes `base_url`, `env_key`, `wire_api`, and `requires_openai_auth` in `codex-rs/model-provider-info/src/lib.rs`.
- `WireApi` currently supports Responses only. Chat Completions requires additional implementation and is deferred by the user's selection.
- `ModelProviderInfo::api_key` reads the configured environment variable and reports a missing credential.
- TUI login gating already checks `requires_openai_auth`; custom providers can skip OpenAI login.
- Unknown model IDs have fallback metadata in `codex-rs/models-manager/src/model_info.rs`. Compatibility of that metadata with the selected endpoint still needs an agent test.
- The effective default provider remains OpenAI in `codex-rs/core/src/config/mod.rs`. An explicit configured provider takes precedence; an unconfigured setup state remains work to verify/design.

## Decisions

### Keep the first release focused on the provider workflow

The user requires one Clodex app/TUI with built-in endpoint/key setup, `/models` discovery, selectable models, and supported reasoning levels. A manually operated gateway, separate server installation, or second application's configuration does not satisfy the product requirement.

Use existing Responses support first. Do not make gateway selection, codec extraction, or multi-project composition a prerequisite. If a later endpoint requires a different protocol, evaluate one reusable in-process translator independently while preserving native Responses traffic.

Reuse configuration, credential storage, models-manager/provider boundaries, model pickers, and reasoning settings. Discovery, selection, and inference must refer to the same provider and credential identity. Verify local function/freeform tools, stable IDs, interrupted streams, reasoning settings, and resumed sessions through the chosen endpoint.

For a later translator, the existing `HttpTransport` boundary in `codex-rs/http-client/src/transport.rs` and generic `ResponsesClient<T>` in `codex-rs/codex-api/src/endpoint/responses.rs` remain candidate seams. This is a deferred design hypothesis, not an implemented integration.

Provider credentials remain managed through Clodex's configured credential boundary. Ordinary configuration holds references rather than raw keys. Keep Codex's approvals, tool execution, and history authoritative; do not import gateway-owned agent loops, conversation stores, MCP execution, or automatic provider failover.

Responses is the first path to verify. Additional upstream formats are deferred.

### Deferred gateway research (2026-10-26; not runtime verification)

The following shortlist records earlier research. It does not define the first-release implementation sequence.

- Respan at `b8575e497c836cbbf0a170db2f7685a5ade753a1` is the first feasibility candidate: its `respan-gateway-conversation-http` crate exposes `Schema::decode_json`, `encode_json`, `decoder`, and `stream_encoder`; the conversation kind is data with no I/O. The workspace declares MIT OR Apache-2.0 and has a Windows CI job. Its Responses parser recognizes function tools but keeps custom items native and refuses unsupported cross-format content. This does not establish Codex freeform-tool compatibility.
- Praxis AI at `3a3704c423444ed8e6a091fbeb31b60e03adf6e0` is a useful reference for rich client-tool lowering/restoration. Its public translator is a filter tied to Praxis runtime state; the finite converters are not a standalone public codec API. Responses is experimental, and native Windows integration was not established.
- Agentgateway at `42c0902ce28eee71d6ba3cf77ef38846cc34fcdf` has an Apache-2.0 `agent-llm` crate, but it is workspace-coupled. The inspected Responses-to-Chat tool converter selects function definitions and drops other tool types; the inspected Responses-to-Anthropic route returns an unsupported conversion error.
- Helicone AI Gateway at `9649b27bdc9fb0907d359e899894102a15f3a085` has a GPL-3.0 LICENSE, despite stale Apache metadata, and the reviewed endpoint mappings do not expose Responses ingress. It is not the preferred reuse candidate.
- Superagent Gateway at `041b08a658ffd0ae587d7065ebb6f4d2ccfc1a18` exposes translation/stream modules but has no explicit license in the reviewed tree; its conversion also lacks Codex custom/freeform-tool support. Do not incorporate its source without resolving permission.

If protocol translation becomes necessary later, Respan's upstream codecs and Praxis's tool-compatibility approach are feasibility candidates. Decoded events could map directly to `codex_api::ResponseEvent` without an internal HTTP server. Keep native-format opaque state on its existing path and report unsupported translated features rather than silently discarding them. No reviewed gateway was built or exercised against Clodex during this source review.

Pinned source references:

- [Respan codec API](https://github.com/respanai/respan-gateway/blob/b8575e497c836cbbf0a170db2f7685a5ade753a1/crates/formats/conversation-http/src/schema/mod.rs), [Responses format limits](https://github.com/respanai/respan-gateway/blob/b8575e497c836cbbf0a170db2f7685a5ade753a1/crates/formats/conversation-http/src/responses/mod.rs).
- [Praxis client-tool compatibility](https://github.com/praxis-proxy/ai/blob/3a3704c423444ed8e6a091fbeb31b60e03adf6e0/docs/filters/openai_client_tool_compat.md).
- [Agentgateway Responses conversion](https://github.com/agentgateway/agentgateway/blob/42c0902ce28eee71d6ba3cf77ef38846cc34fcdf/crates/llm/src/conversion/openai_compat.rs).
- [Helicone license](https://github.com/Helicone/ai-gateway/blob/9649b27bdc9fb0907d359e899894102a15f3a085/LICENSE), [Superagent manifest](https://github.com/superagent-ai/gateway/blob/041b08a658ffd0ae587d7065ebb6f4d2ccfc1a18/Cargo.toml).

### Reuse the existing provider boundary

The first milestone is endpoint/key entry inside Clodex, discovery, model/reasoning selection, and text/local-tool turns through the normal agent. Implement generic discovery at the existing models-manager/provider boundary rather than building provider-specific authentication or catalog services.

Configuration shape, with placeholders only:

```toml
model_provider = "custom-api"
model = "your-model-id"

[model_providers.custom-api]
name = "Custom API"
base_url = "https://your-provider.example/v1"
env_key = "CLODEX_API_KEY"
wire_api = "responses"
requires_openai_auth = false
supports_websockets = false
```

`base_url` is the provider's API base, not the complete `/responses` or `/models` URL. The example shows the existing environment-key path for advanced configuration; the required UI also accepts key entry and stores it securely using a credential reference. Secrets are never committed. This is a configuration illustration, not a claim that an external endpoint has been smoke-tested.

### Discover models from the configured endpoint

Fetch the API base's `/models` endpoint with the configured credential. Bound response size, model count, and request duration. Accept sparse `data` or `models` arrays without requiring Codex's full first-party model metadata. Scope catalog state and caching to provider, endpoint, and credential identity; present refresh, empty, authentication-error, and unavailable states explicitly.

Use one catalog for the TUI `/models` command and the app's model interface, reusing existing `/model` behavior where appropriate. Preserve exact provider model IDs. A manual ID can remain an advanced fallback, but cannot replace required discovery and selection.

### Offer reasoning controls per model

Show the model's supported effort levels and persist a valid selection with its provider/model setting. Apply the selection to inference using the existing reasoning contract. Offer an off/disabled option only where supported. Revalidate effort when switching provider/model and show the resulting setting.

A sparse `/models` response often contains only IDs. Reasoning capabilities therefore come from available provider/model metadata or explicit per-model capability settings. Unknown support must be identified as unknown rather than inventing supported levels. Preserve the selected model's exact capability values, including provider-specific levels when supported by the request contract.

### Make setup and key entry part of the app/TUI

The user enters and edits the endpoint and API key in Clodex, discovers/refreshes models, selects a model, and adjusts its supported reasoning level. Store pasted keys securely through the existing credential boundary; persist references in ordinary configuration and verify secret redaction in logs, rollouts, and API read responses. Verify this complete UI workflow with snapshots and app-server integration tests.

### Preserve useful work without integrating unused infrastructure

Retain the existing branches and commits. Before production implementation, choose a coherent baseline that does not depend on unfinished CommandCode auth/discovery. Review and integrate the independent plugin boundary separately. Account services remain optional under the retained product goal; they must not gate configured endpoint inference.

## Sequence and completion

The tasks file defines five bounded stages: baseline, Responses inference proof, integrated setup/discovery/model controls, plugin/account boundaries, and release verification. The first useful milestone requires endpoint/key entry, authenticated discovery, model/reasoning selection, and successful text/tool/continuation checks without mandatory account login. The full reduced plan additionally requires the retained service boundaries.

## Risks

- Responses compatibility varies: test tool declarations, streaming terminal events, and usage against a real endpoint before claiming support.
- Existing model metadata can advertise unsupported features: exercise a custom model ID and introduce narrowly scoped capability settings only when needed.
- Sparse `/models` rows may not advertise reasoning levels: distinguish unknown capabilities from unsupported ones and support explicit per-model capability settings.
- The prior branches are separate: branch-local tests do not prove the combined release.
- The existing schema was edited manually and dependency manifests changed without a Bazel lock update: reconcile generated files for whichever baseline is actually retained.
