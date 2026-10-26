## Why

The active goal is provider setup inside the app/TUI: enter an API base URL and API key, discover models from that provider's `/models` endpoint, select a model in the TUI or app model interface, and change its supported reasoning level. Responses is the first inference protocol. Gateway/library research is optional follow-up work rather than a prerequisite for this workflow. No separately operated server or second app is required.

## What Changes

- Configure and edit the API base URL and API key inside the app/TUI. Store entered keys securely; ordinary configuration contains credential references.
- Fetch and refresh the configured provider's `/models` catalog using its credential, accepting sparse OpenAI-compatible model rows.
- Populate the TUI `/models` picker and app model-selection interface from the same provider-scoped catalog. Reuse the existing `/model` interface where appropriate and preserve compatibility.
- Expose supported reasoning levels per model, apply the selection to inference, and restore valid settings across sessions. Unknown capabilities must not be presented as confirmed support.
- Support Responses inference first. Add an embedded translator only if a later selected endpoint requires another protocol; combining several gateways is not an initial milestone.
- Keep setup, discovery, selection, and inference in Clodex. Users must not start a gateway service or manage another application's configuration.
- Keep Codex in charge of tools and session history. Use a fixed upstream route for the initial proof, without gateway-owned tool execution or automatic model/provider failover.
- Verify text streaming, local tool execution, session continuation, credential redaction, and failure behavior through the existing agent path.
- Verify the complete UI workflow from entering endpoint/key through discovery, model/reasoning selection, and an agent turn.
- Preserve independent plugins and retain the prior goals of inert ChatGPT Apps and optional account services as a separate follow-up stage.
- Defer native CommandCode OAuth, CLI credential import, identity validation, endpoint auto-routing, and multi-format gateway composition. Generic `/models` discovery is required in the first version.
- Preserve existing implementation commits for reference. Select a coherent integration baseline before making production changes.

## Capabilities

### New Capabilities

- `endpoint-key-runtime`: In-app provider setup, credential-safe `/models` discovery, model/reasoning selection, Responses inference, and observable failure behavior.

### Modified Capabilities

None. This change supersedes the unintegrated `barebones-direct-commandcode` plan rather than claiming its proposed capabilities have shipped.

## Impact

Reuse existing configuration, credential storage, models-manager/provider boundaries, CLI/TUI model controls, and app-server model/config APIs where possible. Extend app-server v2 and schemas only for missing behavior. Provider-specific auth crates and gateway composition are not prerequisites. Plugin/account work remains separately scoped and must be verified on the eventual combined branch.
