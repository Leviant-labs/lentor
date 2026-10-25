## 1. Specification and Repository Gate

- [x] 1.1 Confirm `git rev-parse --show-toplevel` resolves to `codex-upstream` and record a clean baseline status
- [x] 1.2 Complete all OpenSpec artifacts for `barebones-direct-commandcode`
- [x] 1.3 Run `openspec validate --all --strict` and fix every validation error before implementation
- [x] 1.4 Commit the OpenSpec initialization, project-local skills, and change artifacts as the first reviewable Git commit

## 2. Provider Contract Foundation

- [x] 2.1 [FND] Add explicit `ProviderKind`, `AccountServices`, and provider capability types in focused protocol/provider modules
- [x] 2.2 [FND] Add `CommandCodeAuthSource` and `CommandCodeRoute` enums without implementing network behavior
- [x] 2.3 [FND] Extend provider configuration parsing for direct CommandCode identity, base URL, auth source, and model routing
- [x] 2.4 [FND] Freeze public type signatures and publish an interface note consumed by AUTH, TRANSPORT, and ACCOUNT
- [x] 2.5 [FND] Add whole-object config/type round-trip tests and reserved-provider validation tests
- [ ] 2.6 [FND] Run `just write-config-schema` and scoped provider/config tests; commit only foundation changes
  - Partial: `codex-rs/core/config.schema.json` was updated by hand in `c0d4bfc`, but `just write-config-schema` and the scoped provider/config test run are not verified in this environment.

## 3. CommandCode Authentication

- [x] 3.1 [AUTH] Create a focused Rust auth crate with API-key, OAuth, and imported-CLI source types
- [x] 3.2 [AUTH] Implement keyring-backed provider credential storage and non-secret `CommandCodeIdentity` metadata
- [x] 3.3 [AUTH] Implement bounded `GET https://api.commandcode.ai/alpha/whoami` validation and error redaction
- [x] 3.4 [AUTH] Implement random state generation and state-checked CommandCode callback payload parsing
- [x] 3.5 [AUTH] Implement IPv4/IPv6 loopback callback binding, ephemeral-port fallback, timeout, and abort handling
- [x] 3.6 [AUTH] Implement browser authorization URL construction and manual callback/URL/key paste fallback
- [ ] 3.7 [AUTH] Implement optional local CommandCode CLI credential import without retaining the source file
  - Blocked on wording: the implementation follows the spec scenario (key is validated and stored, and no copy of the source auth file is persisted), but it deliberately leaves the user's CLI source file in place. The task text says "without retaining the source file".
- [x] 3.8 [AUTH] Add mock-server tests for success, invalid key, state mismatch, timeout, cancellation, and provider isolation
- [ ] 3.9 [AUTH] Add secret-redaction tests across errors, logs, config output, and serialized diagnostic records
  - Partial: error text/`Debug`, `ApiKey` formatting, and serialized identity metadata are covered. Config output, logs, rollouts, and app-server diagnostic records are not yet tested.
- [ ] 3.10 [AUTH] Treat an environment-variable key reference as its own credential source with distinct identity provenance, and test a populated variable through `whoami` validation and storage

## 4. CommandCode Discovery and Routing

- [x] 4.1 [TRANSPORT] Register the direct `commandcode` provider with base URL `https://api.commandcode.ai/provider/v1`
- [x] 4.2 [TRANSPORT] Implement bounded `/provider/v1/models` retrieval with response-size and model-count caps
- [x] 4.3 [TRANSPORT] Parse sparse model metadata and normalize `supported_endpoints` into `CommandCodeRoute`
- [x] 4.4 [TRANSPORT] Reject unknown, empty, or mutually unsupported route sets before request construction
- [x] 4.5 [TRANSPORT] Add discovery fixtures for complete, sparse, oversized, malformed, and unavailable responses
- [x] 4.6 [TRANSPORT] Expose selected provider, model, route, and catalog state through the existing model-provider boundary
- [ ] 4.7 [TRANSPORT] Select `commandcode` when no `model_provider` is configured, keep an explicit provider authoritative without contacting CommandCode, and test both precedence paths
  - Partial: `select_provider` implements and tests both precedence paths, and discovery only runs through the explicit CommandCode boundary. The effective default provider is still `openai` until task 10.3 wires the config default.

## 5. CommandCode Wire Adapters

- [ ] 5.1 [TRANSPORT] Define shared CommandCode request context and internal stream-event mapping types
- [ ] 5.2 [TRANSPORT] Implement Responses request construction for Responses-capable models
- [ ] 5.3 [TRANSPORT] Implement Responses stream decoding for text, reasoning, tool calls, usage, and finish state
- [ ] 5.4 [TRANSPORT] Implement Chat Completions request construction for Chat Completions-only models
- [ ] 5.5 [TRANSPORT] Implement indexed Chat Completions tool-call assembly, usage, and finish-reason mapping
- [ ] 5.6 [TRANSPORT] Implement Anthropic Messages request construction for Anthropic-only models
- [ ] 5.7 [TRANSPORT] Implement Anthropic Messages stream decoding and tool-result round trips
- [ ] 5.8 [TRANSPORT] Preserve client-side tool execution and exclude remote MCP delegating declarations
- [ ] 5.9 [TRANSPORT] Map 401, 403, 422, 429, and retryable 5xx responses into bounded provider errors
- [ ] 5.10 [TRANSPORT] Add captured request/stream fixtures for every route and terminal failure class
- [ ] 5.11 [TRANSPORT] Add direct-network integration tests proving no proxy or OpenAI fallback route is used
- [ ] 5.12 [TRANSPORT] Preserve the final provider status and request ID on terminal failures, and map zero-retention rejections to a distinct redacted error

## 6. Plugin Preservation and ChatGPT App Boundary

- [ ] 6.1 [PLUGINS] Add compatibility tests locking `PluginManifest`, `PluginId`, `paths.apps`, and marketplace parsing
- [ ] 6.2 [PLUGINS] Add round-trip fixtures for local, Git, and npm marketplace plugin entries
- [ ] 6.3 [PLUGINS] Keep plugin skills, MCP declarations, hooks, and package data active without account services
- [ ] 6.4 [PLUGINS] Disable ChatGPT App connector construction, authorization, discovery, and app-tool injection
- [ ] 6.5 [PLUGINS] Make `paths.apps` parse and serialize while remaining inert at runtime
- [ ] 6.6 [PLUGINS] Produce capability metadata separating available plugin support from disabled ChatGPT Apps
- [ ] 6.7 [PLUGINS] Add regressions proving an app-declaring plugin still loads all non-app resources
- [ ] 6.8 [PLUGINS] Test marketplace add, list, install, update, and remove operations without ChatGPT App service traffic

## 7. Optional Account Services

- [ ] 7.1 [ACCOUNT] Wire `AccountServices::Disabled` as the default and avoid account client construction in that mode
- [ ] 7.2 [ACCOUNT] Preserve Codex/ChatGPT login behavior behind `AccountServices::Codex`
- [ ] 7.3 [ACCOUNT] Preserve account status, usage, limits, and credits only in enabled mode
- [ ] 7.4 [ACCOUNT] Keep CommandCode auth and transport independent of account capability state
- [ ] 7.5 [ACCOUNT] Keep ChatGPT Apps disabled even when account services are enabled
- [ ] 7.6 [ACCOUNT] Add no-network tests for disabled mode, full account reporting tests for enabled mode, and a key-only CommandCode session that discovers models and answers a turn with no Codex or ChatGPT login
- [ ] 7.7 [ACCOUNT] Add account-outage isolation tests with CommandCode inference active

## 8. CLI and TUI Integration

- [ ] 8.1 [CLI-TUI] Replace blocking default login with optional CommandCode provider setup states
- [ ] 8.2 [CLI-TUI] Add CommandCode API-key entry and OAuth/manual-login screens using AUTH interfaces
- [ ] 8.3 [CLI-TUI] Show provider ID, endpoint health, model catalog state, selected model, and supported route
- [ ] 8.4 [CLI-TUI] Render provider-scoped authentication, plan, rate-limit, and upstream errors
- [ ] 8.5 [CLI-TUI] Keep independent plugin management usable and report ChatGPT Apps separately as disabled
- [ ] 8.6 [CLI-TUI] Show account status/usage/limits/credits only when the account capability is enabled
- [ ] 8.7 [CLI-TUI] Add TUI snapshots for setup, auth, model picker, status, plugins, disabled apps, account states, and errors
- [ ] 8.8 [CLI-TUI] Run scoped CLI/TUI tests and review every updated snapshot as a visual diff

## 9. App-Server and Webapp Integration

- [ ] 9.1 [APP] Preserve thread start/resume, turn submission, events, history, model selection, approvals, and local tools
- [ ] 9.2 [APP] Expose provider ID, readiness, model catalog, selected model, and supported route to the webapp
- [ ] 9.3 [APP] Return deterministic disabled responses for ChatGPT App RPCs without network traffic
- [ ] 9.4 [APP] Capability-gate account RPCs and preserve account reporting only when enabled
- [ ] 9.5 [APP] Preserve independent plugin management RPCs through the unchanged plugin format
- [ ] 9.6 [APP] Add JSON-RPC integration tests for thread lifecycle and capability states
- [ ] 9.7 [APP] Add app-server schema tests and regenerate fixtures with `just write-app-server-schema`
- [ ] 9.8 [APP] Run scoped app-server and protocol tests; commit protocol and handler changes together

## 10. Migration and Cleanup

- [ ] 10.1 [INT] Add migration from local-proxy/generated-catalog configuration to direct CommandCode configuration
- [ ] 10.2 [INT] Add session/provider-ID compatibility tests for legacy rollout data
- [ ] 10.3 [INT] Remove implicit OpenAI provider fallback and default OpenAI/ChatGPT service traffic
- [ ] 10.4 [INT] Remove obsolete proxy-only fields after migration tests pass
- [ ] 10.5 [INT] Reconcile shared files across feature branches without changing frozen interfaces
- [ ] 10.6 [INT] Run `just write-config-schema`, `just write-app-server-schema`, and `just bazel-lock-update` when applicable
- [ ] 10.7 [INT] Add source/network audits for proxy endpoints and default OpenAI service hosts

## 11. Independent Verification and Completion

- [ ] 11.1 [VERIFY] Map every OpenSpec requirement and scenario to a test or recorded manual smoke result
- [ ] 11.2 [VERIFY] Review credential handling for secret leakage in config, logs, rollouts, errors, and app-server output
- [ ] 11.3 [VERIFY] Review plugin compatibility and prove ChatGPT Apps cannot execute or trigger authorization
- [ ] 11.4 [VERIFY] Run scoped tests for all changed projects and `just fmt`/scoped `just fix`
- [ ] 11.5 [VERIFY] Run mocked CommandCode text and tool turns through CLI and app-server for all applicable routes
- [ ] 11.6 [VERIFY] Run a real CommandCode smoke test or record the exact external plan/key blocker
- [ ] 11.7 [VERIFY] Re-run `openspec validate --all --strict`, inspect the full diff/history, and approve or reject completion
