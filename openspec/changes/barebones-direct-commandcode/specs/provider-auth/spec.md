## ADDED Requirements

### Requirement: Provider-scoped credentials
The system SHALL associate credentials with an explicit provider ID and MUST NOT use a credential for another provider. CommandCode credentials MUST be usable without any Codex or ChatGPT account credential.

#### Scenario: Isolated providers
- **WHEN** credentials exist for `commandcode` and another provider
- **THEN** each request uses only the credential stored for its active provider ID

#### Scenario: CommandCode-only setup
- **WHEN** only a CommandCode credential exists
- **THEN** model discovery and inference complete without a Codex or ChatGPT login

### Requirement: CommandCode API-key authentication
The system SHALL accept a CommandCode API key through explicit key entry or an environment-variable reference and SHALL validate it with `GET https://api.commandcode.ai/alpha/whoami` before marking it connected. The key MUST be stored through the existing keyring-backed secret store.

#### Scenario: Successful key validation
- **WHEN** a user submits a valid CommandCode API key
- **THEN** Codex stores the secret and records only the validated user ID and display name as non-secret metadata

#### Scenario: Invalid key
- **WHEN** `whoami` rejects the submitted key
- **THEN** Codex does not persist the key and reports authentication failure without echoing it

### Requirement: CommandCode OAuth login
The system SHALL implement CommandCode login in Rust using a random state value, loopback callback listener, browser authorization URL, timeout, abort handling, and manual callback/key paste fallback. The callback state MUST match before credentials are accepted.

#### Scenario: Successful callback
- **WHEN** CommandCode Studio posts a complete callback with the matching state and API key
- **THEN** Codex validates the key with `whoami`, stores it, and reports the authenticated user

#### Scenario: State mismatch
- **WHEN** a callback or pasted authorization response carries a different state
- **THEN** Codex rejects it and continues waiting for a valid response or cancellation

#### Scenario: Manual fallback
- **WHEN** automatic callback delivery is unavailable but the user pastes a valid callback payload, authorization URL, or API key
- **THEN** Codex validates and accepts it only after the applicable state check and `whoami` validation

### Requirement: Credential safety
The system MUST NOT log, display, transmit to another service, or copy provider secrets into configuration or rollout files. Importing an existing local CommandCode CLI credential, when enabled, MUST read only the credential needed for provider authentication and retain only non-secret identity metadata beyond the keyring.

#### Scenario: Redacted diagnostics
- **WHEN** authentication configuration or errors are rendered in CLI, TUI, app-server, logs, or rollouts
- **THEN** secret values are redacted and only provider ID, account display metadata, and credential source are visible

#### Scenario: Local CLI import
- **WHEN** local credential import succeeds
- **THEN** the imported key is validated and stored in Codex's keyring-backed store without persisting a copy of the source auth file
