## 1. Specification and Repository Gate

- [x] 1.1 Confirm `git rev-parse --show-toplevel` resolves to `codex-upstream` and record a clean baseline status
- [x] 1.2 Complete all OpenSpec artifacts for `barebones-direct-commandcode`
- [x] 1.3 Run `openspec validate --all --strict` and fix every validation error before implementation
- [x] 1.4 Commit the OpenSpec initialization, project-local skills, and change artifacts as the first reviewable Git commit

## 2. Provider Authentication

- [ ] 2.1 Create a Rust provider-auth crate with explicit CommandCode API-key, OAuth, and imported-CLI source enums
- [ ] 2.2 Implement keyring-backed provider credential storage and non-secret account metadata
- [ ] 2.3 Implement `https://api.commandcode.ai/alpha/whoami` validation with bounded timeout and redacted errors
- [ ] 2.4 Port the CommandCode loopback OAuth flow to Rust with state validation, IPv4/IPv6 binding, ephemeral-port fallback, abort, and timeout
- [ ] 2.5 Implement manual callback payload, authorization URL, and API-key paste fallback
- [ ] 2.6 Add optional local CommandCode CLI credential import without persisting a copy of the source auth file
- [ ] 2.7 Add authentication tests for success, invalid key, state mismatch, timeout, cancellation, redaction, and provider isolation

## 3. Direct CommandCode Provider

- [ ] 3.1 Add the `commandcode` provider configuration with direct base URL `https://api.commandcode.ai/provider/v1`
- [ ] 3.2 Implement bounded `/provider/v1/models` discovery and sparse model metadata parsing
- [ ] 3.3 Add a Rust model route enum derived from `supported_endpoints`
- [ ] 3.4 Implement direct Responses requests and stream mapping for Responses-capable models
- [ ] 3.5 Implement direct Chat Completions requests and stream mapping for Chat Completions-only models
- [ ] 3.6 Implement Anthropic Messages requests and stream mapping for Anthropic-only models
- [ ] 3.7 Map text, reasoning, tool calls, finish state, usage, and provider errors into existing Codex response events
- [ ] 3.8 Ensure tools remain client-executed and never emit delegating remote MCP declarations to CommandCode
- [ ] 3.9 Add captured-fixture tests for all three transports, unsupported routes, and 401/403/422/429/5xx responses
- [ ] 3.10 Add an integration test proving direct CommandCode requests contain no proxy or OpenAI fallback traffic

## 4. Plugin Runtime Boundary

- [ ] 4.1 Add compatibility tests locking the existing plugin manifest and marketplace formats, including `paths.apps`
- [ ] 4.2 Keep independent plugin marketplace list/add/install/update/remove behavior active without account services
- [ ] 4.3 Disable ChatGPT App connector runtime, authorization, discovery, and app-tool injection
- [ ] 4.4 Make `paths.apps` declarations parse, round-trip, and remain inert while plugin skills, MCP, and hooks stay active
- [ ] 4.5 Add CLI, TUI, and app-server capability reporting that separates plugin support from disabled ChatGPT Apps
- [ ] 4.6 Add regression tests for existing app-declaring plugins and independent marketplace plugins

## 5. Optional Account Services

- [ ] 5.1 Add an explicit account-services capability with disabled default and optional Codex/ChatGPT mode
- [ ] 5.2 Prevent account client construction and network calls while the capability is disabled
- [ ] 5.3 Preserve account login, status, usage, limits, and credits only when the capability is enabled
- [ ] 5.4 Keep CommandCode provider authentication and routing independent of account-services state
- [ ] 5.5 Ensure enabling account services does not enable ChatGPT Apps
- [ ] 5.6 Add tests for disabled startup, enabled account reporting, account outage isolation, and CommandCode coexistence

## 6. CLI, TUI, and Webapp Integration

- [ ] 6.1 Add CLI/TUI setup for CommandCode API-key and OAuth authentication without a blocking default login
- [ ] 6.2 Show provider ID, endpoint health, supported model routes, and provider-scoped failures in CLI/TUI
- [ ] 6.3 Preserve webapp thread start/resume, turns, events, history, model selection, approvals, and local tools
- [ ] 6.4 Return deterministic disabled responses for ChatGPT App app-server RPCs without network traffic
- [ ] 6.5 Capability-gate account app-server APIs and preserve supported account reporting when enabled
- [ ] 6.6 Update TUI snapshots for provider setup, model selection, plugins, disabled apps, optional account state, and provider errors
- [ ] 6.7 Regenerate and verify app-server schema fixtures

## 7. Migration and Cleanup

- [ ] 7.1 Migrate local-proxy and generated-catalog configuration to direct CommandCode provider configuration
- [ ] 7.2 Remove the implicit OpenAI provider fallback and default OpenAI/ChatGPT service traffic
- [ ] 7.3 Keep legacy session history readable and test provider-ID migration behavior
- [ ] 7.4 Remove obsolete proxy-only configuration fields after compatibility tests pass
- [ ] 7.5 Run `just write-config-schema` and verify the generated config schema
- [ ] 7.6 Run `just bazel-lock-update` if Rust dependencies changed and include lockfile updates in the commit

## 8. Verification and Completion

- [ ] 8.1 Run `just fmt` and scoped `just fix` for every changed Rust project
- [ ] 8.2 Run project tests for provider, auth, plugins, core, TUI, and app-server changes
- [ ] 8.3 Run a mocked end-to-end CommandCode text turn and tool-calling turn through CLI and app-server
- [ ] 8.4 Run a real API smoke test with an authorized CommandCode key for `commandcode/xiaomi-mimo-v2.6-pro`, or record the precise plan/endpoint blocker
- [ ] 8.5 Verify no production OpenAI fallback or local proxy path remains in the default runtime
- [ ] 8.6 Re-run `openspec validate --all --strict`, inspect Git status/diff, and commit the completed implementation
