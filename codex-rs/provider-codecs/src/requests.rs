use crate::tools::RestoredToolCall;
use crate::tools::ToolRegistry;
use codex_api::ResponsesApiRequest;
use respan_gateway_conversation::Content;
use respan_gateway_conversation::Input;
use respan_gateway_conversation::Message;
use respan_gateway_conversation::Role;
use respan_gateway_conversation::ToolChoice;
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_conversation_http::responses::OpenAiResponses;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;

/// Translated generation protocol. Native Responses bypasses this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Chat,
    Messages,
}

/// Opaque provider state belonging to one canonical response item.
#[derive(Clone, PartialEq)]
pub struct NativeRecord {
    pub item_id: String,
    pub payload: Value,
}

/// A request conversion or provider stream failure.
#[derive(Debug)]
pub struct CodecError(String);
impl CodecError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for CodecError {}

/// Provider body and the private registry needed to restore tool calls.
#[derive(Clone)]
pub struct EncodedRequest {
    pub body: Value,
    registry: ToolRegistry,
    pub(crate) protocol: Protocol,
    pub(crate) provider_identity: String,
}
impl fmt::Debug for EncodedRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncodedRequest")
            .field("body", &"[redacted]")
            .finish_non_exhaustive()
    }
}
impl EncodedRequest {
    /// Restore the exact local tool identity and unwrap freeform input.
    pub fn restore_tool_call(
        &self,
        wire_name: &str,
        arguments: &str,
    ) -> Result<RestoredToolCall, CodecError> {
        self.registry.restore(wire_name, arguments)
    }
}

/// Encode one canonical request for Chat or Messages, without making HTTP calls.
/// `provider_identity` is the host-computed 64-character SHA-256 endpoint identity.
/// Native payloads must be `{protocol, endpoint_hash, blocks}` and are private.
/// Chat preserves prompt-cache affinity. Messages has no equivalent cache key;
/// that routing hint is intentionally omitted. Internal Responses client metadata
/// remains local to tracing and is never sent as provider message content.
/// Messages projects all system/developer updates into the top-level system
/// prefix, preserving instruction order and conversation order separately. A new
/// update changes that prefix and may invalidate the provider's conversation cache;
/// the canonical request retains the original chronological instruction placement.
pub fn encode_request(
    protocol: Protocol,
    provider_identity: &str,
    request: &ResponsesApiRequest,
    native_states: &[NativeRecord],
) -> Result<EncodedRequest, CodecError> {
    if provider_identity.len() != 64 || !provider_identity.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(CodecError::new("invalid provider identity"));
    }
    let mut native = HashMap::new();
    let mut native_bytes = 0usize;
    for record in native_states {
        if record.item_id.is_empty() || record.item_id.len() > 512 {
            return Err(CodecError::new("invalid native item ID"));
        }
        let size = serde_json::to_vec(&record.payload)
            .map_err(|_| CodecError::new("invalid native payload"))?
            .len();
        native_bytes = native_bytes
            .checked_add(size)
            .ok_or_else(|| CodecError::new("native state overflow"))?;
        if size > 32 * 1024 || native_bytes > 1024 * 1024 {
            return Err(CodecError::new("native state exceeds limit"));
        }
        let expected = match protocol {
            Protocol::Chat => "chat",
            Protocol::Messages => "messages",
        };
        if record.payload.as_object().is_none_or(|object| {
            object.keys().any(|key| !match protocol {
                Protocol::Chat => matches!(key.as_str(), "protocol" | "endpoint_hash" | "field"),
                Protocol::Messages => {
                    matches!(key.as_str(), "protocol" | "endpoint_hash" | "blocks")
                }
            })
        }) {
            return Err(CodecError::new("invalid native state envelope"));
        }
        if record.payload.get("protocol").and_then(Value::as_str) != Some(expected)
            || record.payload.get("endpoint_hash").and_then(Value::as_str)
                != Some(provider_identity)
        {
            return Err(CodecError::new("native state provider mismatch"));
        }
        if native
            .insert(record.item_id.as_str(), &record.payload)
            .is_some()
        {
            return Err(CodecError::new("duplicate native item ID"));
        }
    }
    let raw = serde_json::to_value(request).map_err(|e| CodecError::new(e.to_string()))?;
    if request.service_tier.is_some()
        || request.access_programs.is_some()
        || request.stream_options.is_some()
        || request.store
    {
        return Err(CodecError::new("unsupported provider request control"));
    }
    if request
        .include
        .iter()
        .any(|entry| entry != "reasoning.encrypted_content")
    {
        return Err(CodecError::new("unsupported include field"));
    }
    let mut registry = ToolRegistry::default();
    let mut chat = Input::default();
    chat.model = Some(request.model.clone());
    if let Some(tools) = raw.get("tools").filter(|v| !v.is_null()) {
        registry.add(
            tools
                .as_array()
                .ok_or_else(|| CodecError::new("tools must be an array"))?,
            &mut chat.tools,
            None,
        )?;
    }
    for item in &request.input {
        if let codex_protocol::models::ResponseItem::AdditionalTools { role, tools, .. } = item {
            if role != "developer" && role != "system" {
                return Err(CodecError::new("unsupported additional tools role"));
            }
            registry.add(tools, &mut chat.tools, None)?;
        }
    }
    let choice = match request.tool_choice.as_str() {
        "auto" => ToolChoice::Auto,
        "none" => ToolChoice::None,
        "required" => ToolChoice::Required,
        _ => return Err(CodecError::new("unsupported tool choice")),
    };
    if chat.tools.is_empty() {
        if matches!(choice, ToolChoice::Required) {
            return Err(CodecError::new("tool choice requires a declared tool"));
        }
    } else {
        chat.options.parallel_tool_calls = Some(request.parallel_tool_calls);
        chat.options.tool_choice = Some(choice);
    }
    let mut effective_reasoning = raw.get("reasoning").cloned().unwrap_or(Value::Null);
    let mut used = HashSet::new();
    for item in &request.input {
        let mut value = serde_json::to_value(item).map_err(|e| CodecError::new(e.to_string()))?;
        let kind = value["type"].as_str().unwrap_or("").to_owned();
        if kind == "additional_tools" {
            continue;
        }
        if kind == "configuration_update" {
            if !effective_reasoning.is_object() {
                effective_reasoning = json!({});
            }
            effective_reasoning["effort"] = value["reasoning"]["effort"].clone();
            continue;
        }
        if kind == "reasoning" && protocol == Protocol::Chat {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| CodecError::new("Chat reasoning requires item identity"))?;
            let state = native
                .get(id)
                .ok_or_else(|| CodecError::new("Chat reasoning provider state missing"))?;
            if state.get("field").and_then(Value::as_str) != Some("reasoning_content")
                || !used.insert(id.to_owned())
            {
                return Err(CodecError::new(
                    "unsupported or duplicate Chat reasoning state",
                ));
            }
            append_message(
                &mut chat.messages,
                crate::chat_history::reasoning_message(&value)?,
            );
            continue;
        }
        if kind == "reasoning" && protocol == Protocol::Messages {
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| CodecError::new("native reasoning requires item ID"))?;
            let state = native
                .get(id)
                .ok_or_else(|| CodecError::new("native reasoning state missing"))?;
            if !used.insert(id.to_owned()) {
                return Err(CodecError::new(
                    "native reasoning item appears more than once",
                ));
            }
            let blocks = state
                .get("blocks")
                .and_then(Value::as_array)
                .filter(|b| !b.is_empty())
                .ok_or_else(|| CodecError::new("invalid native reasoning blocks"))?;
            if blocks.iter().any(|block| {
                !matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                )
            }) {
                return Err(CodecError::new("unsupported native reasoning block"));
            }
            if blocks.iter().any(|block| match block["type"].as_str() {
                Some("thinking") => {
                    block["thinking"].as_str().is_none()
                        || block
                            .get("signature")
                            .is_some_and(|signature| signature.as_str().is_none_or(str::is_empty))
                }
                Some("redacted_thinking") => block["data"].as_str().is_none(),
                _ => true,
            }) {
                return Err(CodecError::new("invalid native reasoning block"));
            }
            for block in blocks {
                // Native blocks preserve unsigned gateway thinking too. The
                // pinned codec's generic reasoning projection drops unsigned
                // thinking when preparing an Anthropic request.
                append_message(
                    &mut chat.messages,
                    Message::new(
                        Role::Assistant,
                        Content::native("anthropic-messages", block.clone()),
                    ),
                );
            }
            continue;
        }
        if !matches!(
            kind.as_str(),
            "message"
                | "function_call"
                | "function_call_output"
                | "custom_tool_call"
                | "custom_tool_call_output"
                | "reasoning"
        ) {
            return Err(CodecError::new(format!("unsupported input item {kind}")));
        }
        let object = value
            .as_object_mut()
            .ok_or_else(|| CodecError::new("invalid input item"))?;
        if kind == "message"
            && let Some(parts) = object.get_mut("content").and_then(Value::as_array_mut)
        {
            for part in parts {
                if let Some(part) = part.as_object_mut() {
                    for key in ["annotations", "logprobs"] {
                        if part
                            .get(key)
                            .and_then(Value::as_array)
                            .is_some_and(Vec::is_empty)
                        {
                            part.remove(key);
                        }
                    }
                }
            }
        }
        if object
            .get("encrypted_content")
            .is_some_and(|v| !v.is_null())
            || object
                .get("encrypted_function_args")
                .is_some_and(|v| !v.is_null())
        {
            return Err(CodecError::new("encrypted input cannot be translated"));
        }
        for key in [
            "id",
            "status",
            "phase",
            "encrypted_content",
            "encrypted_function_args",
            "internal_chat_message_metadata_passthrough",
        ] {
            object.remove(key);
        }
        if kind == "function_call" || kind == "custom_tool_call" {
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| CodecError::new("tool call name missing"))?;
            let namespace = object.get("namespace").and_then(Value::as_str);
            let wire = registry.history_wire_name(name, namespace)?;
            object.insert("name".into(), wire.into());
            object.remove("namespace");
            if kind == "custom_tool_call" {
                let input = object
                    .remove("input")
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .ok_or_else(|| CodecError::new("custom call input missing"))?;
                object.insert("type".into(), "function_call".into());
                object.insert(
                    "arguments".into(),
                    json!({"input":input}).to_string().into(),
                );
            }
        } else if kind == "function_call_output" || kind == "custom_tool_call_output" {
            object.remove("name");
            object.remove("namespace");
            if kind == "custom_tool_call_output" {
                object.insert("type".into(), "function_call_output".into());
            }
        }
        let decoded = OpenAiResponses.decode_json(&json!({"input":[value]}));
        for message in decoded.messages {
            append_message(&mut chat.messages, message);
        }
    }
    if used.len() != native.len() {
        return Err(CodecError::new("native state does not match input"));
    }
    let budget = effective_reasoning.get("effort").and_then(|value| {
        value
            .as_u64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
    });
    if budget.is_some()
        && let Some(reasoning) = effective_reasoning.as_object_mut()
    {
        reasoning.remove("effort");
    }
    if !effective_reasoning.is_null() || raw.get("text").is_some() {
        let mut options = json!({});
        if effective_reasoning
            .as_object()
            .is_some_and(|object| !object.is_empty())
        {
            options["reasoning"] = effective_reasoning;
        }
        if let Some(text) = raw.get("text") {
            options["text"] = text.clone();
        }
        let decoded = OpenAiResponses.decode_json(&options);
        chat.options.reasoning = decoded.options.reasoning;
        chat.options.response_format = decoded.options.response_format;
        if decoded.native.is_some() {
            return Err(CodecError::new("unsupported reasoning or text control"));
        }
    }
    if let Some(budget) = budget {
        if protocol == Protocol::Messages && budget >= 4096 {
            return Err(CodecError::new(
                "reasoning budget must be below the 4096 output limit",
            ));
        }
        let reasoning = chat.options.reasoning.get_or_insert_with(Default::default);
        reasoning.budget_tokens = Some(budget);
    }
    if protocol == Protocol::Messages {
        chat.options.max_output_tokens = Some(4096);
        // Anthropic has one system field, with no slot for later privileged
        // messages. Move complete decoded messages only in this wire projection:
        // retain their roles, native fields and relative order for codec validation.
        let (mut instructions, messages): (Vec<_>, Vec<_>) = std::mem::take(&mut chat.messages)
            .into_iter()
            .partition(|message| matches!(message.role, Role::System | Role::Developer));
        instructions.extend(messages);
        chat.messages = instructions;
    }
    let mut body = match protocol {
        Protocol::Chat => OpenAiChat.encode_request(&chat),
        Protocol::Messages => AnthropicMessages.encode_request(&chat),
    }
    .map_err(|e| CodecError::new(e.to_string()))?;
    if let Some(tools) = body.get("tools").and_then(Value::as_array) {
        let mut bytes = 0usize;
        for tool in tools {
            let size = serde_json::to_vec(tool)
                .map_err(|_| CodecError::new("invalid encoded tool"))?
                .len();
            bytes = bytes.saturating_add(size);
            if size > crate::tools::MAX_TOOL_BYTES || bytes > crate::tools::MAX_TOOLS_BYTES {
                return Err(CodecError::new("encoded tools exceed context limit"));
            }
        }
    }
    if protocol == Protocol::Chat
        && let Some(key) = &request.prompt_cache_key
    {
        body.insert("prompt_cache_key".into(), key.clone().into());
    }
    body.insert("stream".into(), request.stream.into());
    if protocol == Protocol::Chat && request.stream {
        body.insert("stream_options".into(), json!({"include_usage": true}));
    }
    Ok(EncodedRequest {
        body: Value::Object(body),
        registry,
        protocol,
        provider_identity: provider_identity.to_owned(),
    })
}

fn append_message(messages: &mut Vec<Message>, message: Message) {
    if let Some(last) = messages.last_mut()
        && last.role == message.role
        && ((last.role == Role::Assistant
            && (!last
                .content
                .iter()
                .any(|part| matches!(part, Content::ToolCall { .. }))
                || message
                    .content
                    .iter()
                    .all(|part| matches!(part, Content::ToolCall { .. }))))
            || (last.role == Role::User
                && last
                    .content
                    .iter()
                    .all(|part| matches!(part, Content::ToolResult { .. }))
                && message
                    .content
                    .iter()
                    .all(|part| matches!(part, Content::ToolResult { .. }))))
        && (last.native.is_none() || crate::chat_history::has_reasoning_field(last))
        && (message.native.is_none() || crate::chat_history::has_reasoning_field(&message))
    {
        last.content.extend(message.content);
        if let Some(incoming) = message.native {
            if let Some(existing) = last.native.as_mut() {
                let left = existing.fields["reasoning_content"]
                    .as_str()
                    .unwrap_or_default();
                let right = incoming.fields["reasoning_content"]
                    .as_str()
                    .unwrap_or_default();
                existing
                    .fields
                    .insert("reasoning_content".into(), format!("{left}{right}").into());
            } else {
                last.native = Some(incoming);
            }
        }
    } else {
        messages.push(message);
    }
}
