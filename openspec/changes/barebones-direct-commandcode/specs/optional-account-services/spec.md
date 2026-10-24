## ADDED Requirements

### Requirement: Account services disabled by default
The default runtime MUST NOT depend on Codex or ChatGPT account authentication, backend account APIs, billing, usage, rate limits, credits, or entitlements. Startup and CommandCode inference MUST complete when no account capability is configured.

#### Scenario: Default startup
- **WHEN** Codex starts with a CommandCode provider and no account capability
- **THEN** no Codex or ChatGPT account service is contacted and the session is ready after provider readiness checks

#### Scenario: Account outage
- **WHEN** optional account services are disabled and an account service is unavailable
- **THEN** CommandCode inference and local tools are unaffected

### Requirement: Explicit account capability
The system SHALL allow a user to enable Codex/ChatGPT account services explicitly. Once enabled, it MAY provide login, account status, usage, limits, and credits through the provider-scoped account service.

#### Scenario: Enabled account status
- **WHEN** the account capability is enabled and login succeeds
- **THEN** CLI, TUI, and app-server may show account status, usage, limits, and credits for that account

#### Scenario: Disabled account surface
- **WHEN** the account capability is disabled
- **THEN** account login, usage, limits, and credits are absent or clearly unavailable and no login prompt blocks startup

### Requirement: Account services do not alter plugin or provider defaults
Enabling account services MUST NOT enable ChatGPT Apps, change the active model provider, or become required for CommandCode OAuth/API-key authentication.

#### Scenario: Account enabled with CommandCode
- **WHEN** account services are enabled while CommandCode is the active provider
- **THEN** inference continues through CommandCode and account usage is presented only as separate account information

#### Scenario: Apps remain disabled
- **WHEN** account services are enabled
- **THEN** ChatGPT App connector execution remains unavailable
