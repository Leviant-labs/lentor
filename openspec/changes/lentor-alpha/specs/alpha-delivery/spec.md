## ADDED Requirements

### Requirement: Prebuilt Windows alpha package

The packaging entrypoint SHALL assemble a Windows x64 archive using only
explicitly supplied prebuilt binaries, and SHALL include package integrity
metadata, required resources, license notices and setup instructions.

#### Scenario: A required binary is missing

- **WHEN** a required prebuilt binary is absent
- **THEN** assembly fails without starting a build or downloading a binary

### Requirement: Evidence-gated alpha release

The project SHALL require end-to-end evidence for the Responses, Chat
Completions and Anthropic Messages protocols, package verification and a fresh
name-conflict review before making an alpha release decision.

#### Scenario: Only direct API smoke evidence exists

- **WHEN** direct API probes or source tests pass without integrated provider
  setup, discovery, inference, tool execution and session continuation evidence
- **THEN** the relevant protocol remains unverified for release readiness

### Requirement: Safe CI execution

Pull-request CI SHALL use clean fixtures without provider keys, and live
provider smoke checks SHALL require a separate explicit trusted invocation.

#### Scenario: A pull request targets main or dev

- **WHEN** the pull request CI runs
- **THEN** it does not run live provider smoke checks or use provider secrets
