## Why

Lentor needs a reviewable alpha candidate that can coexist with Codex and use
configured providers. This milestone supersedes the Responses-only scope of
`endpoint-api-key-runtime`; that earlier change and its evidence remain historical
records, not proof that the expanded milestone is complete.

## What Changes

- Complete the independent Lentor CLI, isolated profile, setup and provider
  integration with the selected Responses, Chat Completions and Anthropic
  Messages protocols.
- Assemble a Windows x64 archive from explicit prebuilt inputs, with helpers,
  resources, license, setup information and integrity metadata.
- Use GitHub-hosted CI with scoped tests, schema and lock checks and an offline
  Windows package smoke. Keep live provider runs separate and explicitly initiated.
- Gate release on refreshed name checks and recorded end-to-end evidence across
  all three protocols. Source tests and raw API calls alone are not that evidence.

## Impact

Runtime, codec, setup, TUI and packaging work are dependent on the shared
integration branch. No tag, release or publication is implied by this proposal.
