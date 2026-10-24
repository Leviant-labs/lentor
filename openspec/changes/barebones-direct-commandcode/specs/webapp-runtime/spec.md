## ADDED Requirements

### Requirement: Core webapp thread workflow
The app-server SHALL support starting and resuming threads, submitting turns, streaming model/tool events, reading history, selecting models, applying approvals, and executing local tools without ChatGPT Apps or mandatory account services.

#### Scenario: New webapp thread
- **WHEN** the webapp starts a thread with the default CommandCode provider
- **THEN** the thread becomes ready without account login and accepts a user turn

#### Scenario: Resume and history
- **WHEN** the webapp resumes an existing thread
- **THEN** prior history is readable and the next turn continues with the configured provider and model

### Requirement: Capability-gated account APIs
App-server account APIs SHALL be present only when the optional account capability is enabled. Disabled account APIs MUST return deterministic unsupported responses and MUST NOT initiate account-service network traffic.

#### Scenario: Disabled account API
- **WHEN** the webapp requests account status while account services are disabled
- **THEN** app-server returns a stable unsupported response without calling a Codex or ChatGPT service

#### Scenario: Enabled account API
- **WHEN** account services are enabled and authenticated
- **THEN** app-server exposes the supported account status, usage, limits, and credits responses

### Requirement: ChatGPT Apps unavailable
App-server MUST NOT expose functional ChatGPT App discovery, authorization, or connector execution. Plugin management APIs for independent plugins MUST continue to work.

#### Scenario: App request
- **WHEN** the webapp requests ChatGPT App discovery or execution
- **THEN** app-server reports the capability as disabled and performs no network call

#### Scenario: Plugin request
- **WHEN** the webapp lists or manages independent plugins
- **THEN** the request follows the preserved plugin marketplace and manifest behavior

### Requirement: Webapp provider visibility
The webapp SHALL receive provider ID, endpoint health, model catalog state, selected model, and supported transport for the active provider. Errors MUST identify the provider and MUST NOT imply OpenAI is the default backend.

#### Scenario: Provider readiness
- **WHEN** the webapp opens provider or model settings
- **THEN** it shows CommandCode connectivity, discovered models, and each model's supported endpoints

#### Scenario: Provider failure
- **WHEN** CommandCode authentication or inference fails
- **THEN** the webapp shows a provider-scoped error with retry or authentication guidance
