## ADDED Requirements

### Requirement: Explicit endpoint and model configuration
The system SHALL support entry and editing of an API base URL and API key inside the app/TUI, with model selection from the configured provider's discovered catalog. Entered keys SHALL be stored securely; ordinary configuration SHALL contain credential references. The first version SHALL use the Responses API.

#### Scenario: Configured Responses provider
- **WHEN** a user configures an endpoint/key and selects a discovered model
- **THEN** inference uses that provider's API base URL, bearer credential, and exact model ID

#### Scenario: Missing credential
- **WHEN** the configured credential is absent or empty
- **THEN** the system reports the missing provider credential without sending an inference request to another provider

### Requirement: Integrated provider workflow
The system SHALL perform setup, discovery, model/reasoning selection, and Responses inference within the Clodex app/TUI without requiring a separately operated gateway server or second app. Codex SHALL retain local tool execution and session history ownership. Protocol translation beyond Responses is not required for the first version.

#### Scenario: Single application setup
- **WHEN** the user enters a supported endpoint and API key in Clodex
- **THEN** Clodex discovers models and exposes selection controls without another application to be started or configured

### Requirement: Provider-scoped model discovery
The system SHALL discover models from the configured API base URL's `/models` endpoint using the configured provider credential. Discovery SHALL be bounded and accept sparse model rows in OpenAI-compatible `data` arrays or `models` arrays. The TUI `/models` picker and app model interface SHALL consume the same provider-scoped catalog.

#### Scenario: Sparse discovered models
- **WHEN** `/models` returns model rows containing IDs but no optional display or reasoning metadata
- **THEN** those models are selectable by their exact IDs and missing capabilities remain unknown

#### Scenario: Provider switch
- **WHEN** the user selects a different provider or changes the endpoint/credential
- **THEN** discovery and selection use the corresponding catalog without leaking models from the previous provider

#### Scenario: Discovery failure
- **WHEN** discovery fails or returns an empty catalog
- **THEN** the interface shows the provider-scoped error or empty state and offers refresh without silently choosing another provider/model

### Requirement: Model-specific reasoning controls
The system SHALL expose reasoning-level selection in the model controls for models with supported levels, apply the selection to inference, and persist it when valid. Supported levels SHALL come from provider/model capability metadata or explicit per-model capability settings; missing metadata SHALL NOT be treated as confirmed support. An off/disabled level SHALL be offered only when supported.

#### Scenario: Supported reasoning selection
- **WHEN** a user selects an advertised or explicitly configured reasoning level for the active model
- **THEN** the next inference request uses that level and the UI displays the selected setting

#### Scenario: Model without known reasoning support
- **WHEN** a selected model's reasoning capability is unsupported or unknown
- **THEN** the interface identifies that state without presenting arbitrary levels as supported

#### Scenario: Model change
- **WHEN** the user switches to a model that does not support the saved reasoning level
- **THEN** Clodex revalidates the setting and displays the resulting valid/default selection before inference

### Requirement: Account-independent startup and model selection
The system SHALL allow configured endpoint discovery and inference without mandatory OpenAI/ChatGPT login, CommandCode identity validation, or a first-party model catalog.

#### Scenario: Custom provider
- **WHEN** a configured custom provider starts with a valid credential
- **THEN** discovery and inference use that provider without authenticating an unrelated account

### Requirement: Responses agent behavior
The system SHALL preserve text streaming, local tool execution under existing approvals and sandbox policy, tool-result submission, and session continuation through the configured Responses endpoint.

#### Scenario: Local tool round trip
- **WHEN** the endpoint returns a supported local tool call
- **THEN** Codex executes it through its normal tool boundary and submits the result to the same configured provider

#### Scenario: Continued session
- **WHEN** a user continues or resumes a session using the configured provider
- **THEN** the next turn retains that provider and model selection without rewriting existing history

### Requirement: Credential-safe provider failures
The system SHALL report missing credentials, HTTP authentication failures, rate limits, and terminal transport failures without exposing API keys. Failed inference MUST NOT silently select another provider.

#### Scenario: Rejected API key
- **WHEN** the configured endpoint rejects the API key
- **THEN** the user receives a redacted provider error and inference does not fall back to OpenAI or another endpoint

### Requirement: Generic setup and readiness
The system SHALL expose setup and readiness for the configured endpoint, discovered catalog, selected model/reasoning level, and credential reference without requiring provider-specific OAuth.

#### Scenario: Endpoint setup
- **WHEN** the user configures a Responses endpoint/key and selects a discovered model and supported reasoning level
- **THEN** the saved configuration selects that provider/model/reasoning setting, references its credential, and does not contain the raw key
