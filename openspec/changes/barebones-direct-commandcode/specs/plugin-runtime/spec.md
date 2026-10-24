## ADDED Requirements

### Requirement: Plugin package format compatibility
The system SHALL preserve the existing plugin manifest and marketplace formats, including namespaced plugin IDs, local/Git/npm sources, skills, MCP declarations, hooks, interface metadata, and the `paths.apps` field. This change MUST NOT rename or remove manifest fields.

#### Scenario: Existing manifest loads
- **WHEN** an existing plugin manifest is parsed and installed
- **THEN** its name, version, resources, interface metadata, and `paths.apps` value round-trip without migration

#### Scenario: Marketplace source remains supported
- **WHEN** a marketplace lists a local, Git, or npm plugin
- **THEN** Codex can list and install it through the existing marketplace policy

### Requirement: Independent plugin runtime
The system SHALL continue loading plugin-provided skills, MCP servers, hooks, and other package resources without Codex or ChatGPT account authentication. Independent plugin marketplaces MUST remain usable when ChatGPT Apps are disabled.

#### Scenario: Plugin without account services
- **WHEN** a plugin provides skills, MCP servers, or hooks and no account capability is enabled
- **THEN** those resources are available under existing trust and permission policy

#### Scenario: Marketplace operation
- **WHEN** a user adds, lists, installs, updates, or removes an independent plugin marketplace entry
- **THEN** the operation works without contacting a ChatGPT Apps service

### Requirement: ChatGPT Apps are inert
The system SHALL preserve `paths.apps` during parsing and serialization but MUST NOT execute ChatGPT App connectors, inject app tools, authorize apps through a ChatGPT account, or discover ChatGPT Apps at startup.

#### Scenario: Plugin declares apps
- **WHEN** a plugin manifest contains `paths.apps`
- **THEN** the plugin's non-app resources remain active and the apps declaration remains stored but produces no app tools or network calls

#### Scenario: Explicit app invocation
- **WHEN** user input requests a ChatGPT App or connector
- **THEN** Codex reports that ChatGPT Apps are unavailable in this build and does not start app authorization

#### Scenario: Capability reporting
- **WHEN** the TUI, CLI, or app-server reports plugin capabilities
- **THEN** plugin support is reported as available and ChatGPT Apps are reported separately as disabled
