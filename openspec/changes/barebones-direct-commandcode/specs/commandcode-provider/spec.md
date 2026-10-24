## ADDED Requirements

### Requirement: Direct CommandCode provider selection
The system SHALL provide a built-in `commandcode` model provider that communicates directly with `https://api.commandcode.ai/provider/v1` and MUST NOT require a local proxy. When the user has not selected a provider, the system SHALL select `commandcode`; an explicit configured provider MUST take precedence.

#### Scenario: Default provider
- **WHEN** configuration contains no `model_provider` value
- **THEN** the active provider is `commandcode` and its base URL is `https://api.commandcode.ai/provider/v1`

#### Scenario: Explicit provider precedence
- **WHEN** configuration selects another valid provider
- **THEN** that provider is used and CommandCode is not contacted

#### Scenario: Proxy independence
- **WHEN** a CommandCode request is built and sent
- **THEN** it is sent directly to `api.commandcode.ai` and no request is sent to `127.0.0.1`, OpenCodex, or LiteLLM

### Requirement: Capability-driven model transport
The system SHALL discover CommandCode models from `GET /provider/v1/models` and SHALL select the request endpoint from each model's `supported_endpoints` metadata. Responses-capable models MUST use `/provider/v1/responses`, Chat Completions-only models MUST use `/provider/v1/chat/completions`, and Anthropic-only models MUST use `/provider/v1/messages`.

#### Scenario: Responses-capable model
- **WHEN** a selected model lists `responses` support
- **THEN** the request is sent to `/provider/v1/responses` using the Responses request shape

#### Scenario: Chat Completions-only model
- **WHEN** a selected model lists `chat/completions` support but no Responses support
- **THEN** the request is sent to `/provider/v1/chat/completions` using the Chat Completions request shape

#### Scenario: Anthropic-only model
- **WHEN** a selected model lists `messages` support only
- **THEN** the request is sent to `/provider/v1/messages` using the Anthropic Messages request shape

#### Scenario: Unsupported model route
- **WHEN** a selected model lists no supported route that Codex implements
- **THEN** the request fails before network execution with an error naming the model and supported endpoints

### Requirement: Streaming and tool execution
The system SHALL translate supported CommandCode streams into Codex's internal response events while preserving text, reasoning, tool calls, finish state, and token usage. Tools executed by Codex MUST be encoded for the selected wire format; remote MCP tools MUST NOT be delegated to CommandCode.

#### Scenario: Text streaming
- **WHEN** a model emits streamed text and terminal usage
- **THEN** Codex emits incremental text events and one terminal completion containing the reported usage

#### Scenario: Client-side tool call
- **WHEN** a model requests a Codex function, custom tool, or local shell tool
- **THEN** Codex receives the complete call, executes it under normal approval and sandbox policy, and sends its result on the next request

#### Scenario: Remote MCP declaration
- **WHEN** the tool catalog contains a remote MCP tool that CommandCode would need to execute
- **THEN** Codex excludes or converts it to client-executed function tools and does not send a delegating `mcp` tool declaration

### Requirement: Provider-scoped failure behavior
The system SHALL map CommandCode authentication, plan, routing, rate-limit, zero-retention, and upstream failures to redacted Codex errors. A failed CommandCode request MUST NOT fall back to OpenAI or another provider.

#### Scenario: Invalid or missing key
- **WHEN** CommandCode returns `401 authentication_error`
- **THEN** Codex reports that the CommandCode credential is missing or invalid and suggests provider-scoped authentication

#### Scenario: Plan lacks Provider API access
- **WHEN** CommandCode returns `403 upgrade_required`
- **THEN** Codex reports that the current CommandCode plan lacks API access and does not change provider or model

#### Scenario: Retryable provider failure
- **WHEN** CommandCode returns `429` or a retryable `5xx` response
- **THEN** Codex applies bounded retry policy and preserves the final provider status and request ID when available
