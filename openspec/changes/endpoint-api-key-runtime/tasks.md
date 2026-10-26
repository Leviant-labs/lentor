## 1. Baseline and scope

- [x] 1.1 Record the user's endpoint/API-key direction and Responses-first selection; supersede the earlier CommandCode-specific plan without deleting its commits.
- [x] 1.2 Inspect existing base URL, environment-key, login-gating, and manually configured model support.
- [ ] 1.3 Select a coherent integration baseline and identify which plugin changes to retain; keep unused CommandCode-specific auth/discovery out of the required runtime.
- [x] 1.4 Validate the new OpenSpec artifacts and record the current branch's integration limits.
  - Evidence: `openspec validate --all --strict` passes both changes. The current PR4 branch contains foundation/auth/discovery work; plugin changes remain on the separate PR2 branch. No combined release or real endpoint smoke test is verified.

## 2. First milestone: configured Responses inference

- [x] 2.0 Record the clarified first-release workflow: in-app endpoint/key setup, generic `/models` discovery, model selection, and supported reasoning controls. Gateway composition is deferred.
- [ ] 2.1 Verify explicit provider, API base URL, key reference, and exact model ID through an agent integration test using the existing provider boundary.
- [ ] 2.2 Verify provider-scoped startup/discovery without mandatory OpenAI/ChatGPT login, first-party model catalogs, or CommandCode identity calls.
- [ ] 2.3 Verify streamed text and terminal events through the configured endpoint.
- [ ] 2.4 Verify a local tool call, tool-result submission, and a continued/resumed turn against the same provider.
- [ ] 2.5 Cover missing key, rejected key, rate-limit, and transport errors with credential-redaction and no-provider-fallback assertions.
- [ ] 2.6 Run a real Responses-compatible endpoint smoke test when its base URL, model ID, and API key are available; record exact evidence or the external blocker.
- [ ] 2.7 Verify that the selected reasoning effort reaches inference, persists when valid, and is revalidated when provider/model changes.

## 3. In-app setup, discovery, and model controls

- [ ] 3.1 Add endpoint/API-key entry and edit controls inside the app/TUI, including unconfigured, connecting, ready, and error states.
- [ ] 3.2 Store entered API keys securely through the existing credential boundary; keep raw keys out of ordinary config, logs, rollouts, and app-server read responses.
- [ ] 3.3 Fetch and refresh authenticated `/models` from the configured API base with size/time/model-count caps; parse sparse `data` or `models` arrays and isolate caches by provider/endpoint/credential identity.
- [ ] 3.4 Expose the same discovered catalog to the TUI `/models` picker and app model interface; reuse existing `/model` behavior where appropriate and preserve selected model IDs exactly.
- [ ] 3.5 Add per-model reasoning controls using advertised capabilities or explicit per-model capability settings; show unknown/unsupported capability states without inventing levels.
- [ ] 3.6 Handle empty catalogs, unavailable discovery, authentication errors, refresh, and provider switches without mixing model lists or silently changing provider/model.
- [ ] 3.7 Add TUI snapshots and app-server integration coverage for setup, secure key entry, discovery, model selection, reasoning selection, and errors; regenerate API schemas if changed.

## 4. Retained plugin and account goals

- [ ] 4.1 Review and integrate the existing plugin boundary; verify manifests, independent resources, and marketplace operations while ChatGPT Apps stay inert.
- [ ] 4.2 Make account services explicitly optional without blocking endpoint inference; verify disabled behavior and enabled account functionality separately.
- [ ] 4.3 Verify thread lifecycle, approvals, local tools, and deterministic disabled-service responses on the combined branch.

## 5. Verification and delivery

- [ ] 5.1 Reconcile retained configuration/session compatibility and regenerate schemas/Bazel locks when the final production diff requires them.
- [ ] 5.2 Run scoped project tests, required format/lint checks, and strict OpenSpec validation; request approval before the complete test suite.
- [ ] 5.3 Publish focused reviewable PRs with appropriate CI and fork-specific setup instructions; record the supported endpoint/protocol and real smoke-test evidence.

Deferred: CommandCode OAuth/import/whoami, provider-specific route selection, multi-gateway composition, and handwritten multi-format adapters. If another inference protocol is needed later, evaluate one embedded implementation independently. Generic `/models` discovery and model/reasoning controls are required now.
