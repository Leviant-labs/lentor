## Context

Codex is a large Rust workspace whose default model path, authentication model, account surfaces, and some app integrations assume OpenAI/ChatGPT services. The current local workaround points Codex at an OpenCodex proxy and a generated catalog, but the intended product boundary is direct provider access.

CommandCode exposes a documented Provider API at `https://api.commandcode.ai/provider/v1` with `/models`, `/responses`, `/chat/completions`, and `/messages`. Its CLI OAuth flow is implemented in TypeScript in the OpenCodex reference project. The Codex plugin manifest and marketplace formats already separate package resources from the ChatGPT Apps declared through `paths.apps`, which allows the app runtime to be removed without changing package compatibility.

The repository root is `codex-upstream`, which is already a Git worktree. OpenSpec 1.5.0 is installed and the project-local `spec-driven` schema is initialized. The optional global Codex prompt shortcut could not be installed because it attempted to write outside the workspace; project-local OpenSpec skills were created successfully.

## Goals / Non-Goals

**Goals:**

- Make CommandCode work directly from Rust with API-key and OAuth authentication.
- Remove OpenAI/ChatGPT services from the default runtime dependency path.
- Preserve the plugin package format and independent plugin marketplaces.
- Disable ChatGPT Apps while leaving their manifest syntax compatible and inert.
- Keep Codex/ChatGPT account information opt-in and provider-scoped.
- Preserve a minimal webapp/app-server thread workflow.
- Keep all production additions Rust-first and all behavior traceable to strict OpenSpec requirements.

**Non-Goals:**

- Redesigning the plugin manifest, marketplace manifest, or plugin ID format.
- Building a generic multi-provider codec framework before CommandCode works.
- Reimplementing CommandCode Studio, billing, plan management, or CommandCode's internal account backend.
- Removing optional Codex/ChatGPT account code permanently.
- Supporting every model endpoint or special product such as `/systemone` in the first implementation.
- Rewriting stored rollout or session history formats.

## Decisions

### 1. Use the direct CommandCode Provider API

The provider base URL is `https://api.commandcode.ai/provider/v1`. Codex sends requests directly to that origin. The local OpenCodex proxy is removed from configuration and tests.

Alternatives considered:

- Keep the local proxy: rejected because it adds a second process, a second credential boundary, and an undocumented deployment requirement.
- Embed CommandCode's proprietary `/alpha/generate` CLI route: rejected for the default API integration because the published Provider API exposes standard Responses, Chat Completions, and Anthropic Messages endpoints. The TypeScript `/alpha/generate` adapter remains useful only as edge-case reference, not the target contract.

### 2. Route models from live endpoint metadata

`GET /provider/v1/models` is authoritative. A model route is selected from `supported_endpoints` and expressed as a Rust enum such as `CommandCodeRoute::{Responses, ChatCompletions, AnthropicMessages}`. Unknown or unsupported routes fail before request construction.

Existing Responses and Chat-compatible codecs can be reused behind a small transport trait. Anthropic Messages needs its own request/stream adapter. The implementation should not create a broad provider framework until more than CommandCode requires it.

### 3. Implement authentication in a dedicated Rust crate

Create `codex-command-code-auth`, or a closely scoped `codex-provider-auth` crate if generic provider OAuth primitives are reused immediately. Keep it outside `codex-core`.

Authentication sources are explicit enums, not booleans or positional options:

- `CommandCodeAuthSource::ApiKey`
- `CommandCodeAuthSource::OAuth`
- `CommandCodeAuthSource::ImportedCli`

The OAuth flow preserves the TypeScript behavior: random state, loopback callback on port 5959 with fallback to an ephemeral port, browser URL construction, timeout/abort, manual paste, and `whoami` validation. Secrets are held in `codex-keyring-store`; persisted account metadata contains only user ID, display name, key name, and source.

### 4. Keep plugin format stable and make app execution a capability boundary

`codex-plugin::PluginManifest` remains unchanged, including `PluginManifestPaths.apps`. Existing marketplace parsing and local/Git/npm installation stay active.

ChatGPT Apps are disabled at the runtime/tool boundary: app declarations may parse and serialize, but no connector runtime, app tool provider, app authorization, or ChatGPT Apps discovery is constructed. The TUI and app-server report plugin support and ChatGPT Apps as separate capabilities.

### 5. Model account services as an optional capability

Introduce an explicit account capability such as `AccountServices::{Disabled, Codex}`. Disabled is the default and has no network client construction. Enabled retains login, status, usage, limits, and credits.

This capability is independent of `ModelProvider`. CommandCode authentication never checks the account capability. Enabling it does not enable ChatGPT Apps.

### 6. Preserve core app-server APIs while changing capability responses

Thread start/resume, turns, events, history, model selection, approvals, and local tools remain. ChatGPT App RPCs return a stable disabled capability result. Account RPCs return a stable unsupported result when `AccountServices::Disabled` and operate normally only when enabled.

This keeps the webapp useful without requiring coordinated removal of every v2 payload type in the first stage. Schema fixtures are regenerated after behavior and capability fields change.

### 7. Use OpenSpec as the implementation gate

The `barebones-direct-commandcode` change owns the normative contract. Production code and tests must map to its requirements. Implementation begins only after `openspec validate --all --strict` passes. Each implementation stage must leave the `codex-upstream` Git worktree with reviewable tracked changes and commits.

## Risks / Trade-offs

- [CommandCode model metadata changes] -> Resolve routes live, cap response size/model count, and fail closed for unsupported routes.
- [Three wire formats increase test surface] -> Share internal event mapping and cover each endpoint with captured request/stream fixtures before adding provider breadth.
- [OAuth callback behavior differs across Windows and browsers] -> Bind IPv4 and IPv6 loopback best-effort, support ephemeral-port fallback, and retain manual paste.
- [Secrets leak through legacy OpenAI auth/debug paths] -> Centralize secret types, add redaction tests, and forbid raw credentials in config, logs, rollouts, and app-server responses.
- [Removing app connectors breaks plugins that declare `apps`] -> Preserve manifest fields and continue loading non-app resources; report app declarations as inert.
- [Optional account code still increases binary complexity] -> Construct account clients only from the explicit capability and keep CommandCode tests in a configuration where the capability is disabled.
- [Webapp expects removed ChatGPT App APIs] -> Keep deterministic disabled responses until the webapp can consume the capability model; do not perform hidden fallback calls.
- [Current source contains broad OpenAI/Codex service coupling] -> Land changes in stages, keep diffs under repository size guidance, and avoid extending `codex-core` with new provider/auth concepts.

## Migration Plan

1. Commit the OpenSpec baseline in `codex-upstream` and keep implementation blocked until strict validation passes.
2. Add direct CommandCode configuration and authentication without removing legacy configuration parsing.
3. Add endpoint routing and provider tests, then switch the default provider to `commandcode` only after an end-to-end mock and real API smoke test pass.
4. Disable ChatGPT Apps at runtime and verify plugin compatibility with existing manifests and marketplace fixtures.
5. Gate account services, update CLI/TUI/app-server capability reporting, and regenerate schemas.
6. Remove proxy-only defaults and stale OpenAI fallback behavior after migrated configuration tests pass.
7. Roll back by restoring the previous provider selection; session history remains unchanged and readable throughout.

## Open Questions

No product decisions remain open. Live implementation checks must confirm the current `supported_endpoints` values and streaming event shapes against CommandCode, but those are runtime facts to verify and test rather than design choices.
