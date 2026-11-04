mod messages;
mod output;

use crate::AnthropicNativeState;
use crate::ChatNormalizer;
use crate::CodecError;
use crate::EncodedRequest;
use crate::NativeRecord;
use crate::Protocol;
use bytes::Bytes;
use codex_api::ResponseEvent;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use respan_gateway_conversation::Block;
use respan_gateway_conversation::Finish;
use respan_gateway_conversation::Usage;
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_http::Decoder;
use respan_gateway_sse::Parser;
use serde::Deserialize;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde::de::{self};
use serde_json::Value;
use serde_json::json;
use std::collections::BTreeMap;
use std::collections::HashSet;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_EVENT: usize = 2 * 1024 * 1024;
const MAX_ARGS: usize = 1024 * 1024;
const MAX_NATIVE: usize = 1024 * 1024;

/// Events and private provider state produced by one transport operation.
/// Store all native records before delivering the events in the same batch.
#[derive(Default)]
pub struct StreamBatch {
    pub events: Vec<ResponseEvent>,
    pub native_state: Vec<NativeRecord>,
}

struct Item {
    id: String,
    kind: Block,
    content: String,
    ended: bool,
    deferred: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MessageBlock {
    Text,
    Tool,
    Thinking,
    Redacted,
}

/// Incremental translator for a single Chat or Messages response.
pub struct CodecStream {
    protocol: Protocol,
    provider: String,
    request: EncodedRequest,
    chat: ChatNormalizer,
    parser: Parser,
    decoder: <AnthropicMessages as Schema>::Decoding,
    bytes: usize,
    last: Option<u8>,
    stop: bool,
    message_started: bool,
    message_finished: bool,
    message_blocks: BTreeMap<u32, (MessageBlock, bool)>,
    failed: bool,
    ended: bool,
    response_id: Option<String>,
    finish: Option<Finish>,
    usage: Usage,
    has_usage: bool,
    items: BTreeMap<u32, Item>,
    native: BTreeMap<u32, AnthropicNativeState>,
    redacted: HashSet<u32>,
    calls: HashSet<String>,
    defer_output: bool,
}

impl CodecStream {
    pub fn new(protocol: Protocol, provider_identity: &str, request: &EncodedRequest) -> Self {
        Self {
            protocol,
            provider: provider_identity.to_owned(),
            request: request.clone(),
            chat: ChatNormalizer::default(),
            parser: Parser::new(Some(MAX_EVENT)),
            decoder: AnthropicMessages.decoder(),
            bytes: 0,
            last: None,
            stop: false,
            message_started: false,
            message_finished: false,
            message_blocks: BTreeMap::new(),
            failed: false,
            ended: false,
            response_id: None,
            finish: None,
            usage: Usage::default(),
            has_usage: false,
            items: BTreeMap::new(),
            native: BTreeMap::new(),
            redacted: HashSet::new(),
            calls: HashSet::new(),
            defer_output: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<StreamBatch, CodecError> {
        if self.failed || self.ended {
            return Err(err("stream is closed"));
        }
        let result = self.push_inner(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn push_inner(&mut self, bytes: &[u8]) -> Result<StreamBatch, CodecError> {
        if self.request.protocol != self.protocol || self.request.provider_identity != self.provider
        {
            return Err(err("stream request provider mismatch"));
        }
        if self.provider.len() != 64 || !self.provider.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(err("invalid provider identity"));
        }
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| err("response size overflow"))?;
        if self.bytes > MAX_RESPONSE {
            return Err(err("response exceeds limit"));
        }
        if let Some(last) = bytes.last() {
            self.last = Some(*last);
        }
        let outputs = match self.protocol {
            Protocol::Chat => self
                .chat
                .push(bytes)
                .map_err(|_| err("invalid Chat stream"))?,
            Protocol::Messages => {
                let frames = self
                    .parser
                    .push(Bytes::copy_from_slice(bytes))
                    .map_err(|_| err("invalid SSE framing"))?;
                let mut outputs = Vec::new();
                for frame in frames {
                    self.message_frame(frame, &mut outputs)?;
                }
                outputs
            }
        };
        let mut batch = StreamBatch::default();
        for output in outputs {
            self.map(output, &mut batch)?;
        }
        Ok(batch)
    }

    pub fn finish(&mut self) -> Result<StreamBatch, CodecError> {
        if self.failed || self.ended {
            return Err(err("stream is closed"));
        }
        let result = self.finish_inner();
        if result.is_err() {
            self.failed = true;
        } else {
            self.ended = true;
        }
        result
    }

    fn finish_inner(&mut self) -> Result<StreamBatch, CodecError> {
        if !matches!(self.last, Some(b'\n' | b'\r')) {
            return Err(err("truncated SSE stream"));
        }
        let outputs = match self.protocol {
            Protocol::Chat => self.chat.end().map_err(|_| err("invalid Chat stream"))?,
            Protocol::Messages => {
                let mut outputs = Vec::new();
                for frame in self.parser.finish() {
                    self.message_frame(frame, &mut outputs)?;
                }
                if !self.stop {
                    return Err(err("missing message stop"));
                }
                self.decoder
                    .end(Ok(()), &mut outputs)
                    .map_err(|_| err("invalid Messages stream"))?;
                outputs
            }
        };
        let mut batch = StreamBatch::default();
        for output in outputs {
            self.map(output, &mut batch)?;
        }
        let response_id = self
            .response_id
            .clone()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| err("missing response identity"))?;
        let finish = self
            .finish
            .as_ref()
            .ok_or_else(|| err("missing finish reason"))?;
        if self.items.values().any(|i| !i.ended) {
            return Err(err("unclosed content block"));
        }
        let is_tool = !self.calls.is_empty();
        if matches!(finish, Finish::ToolUse) != is_tool {
            return Err(err("finish reason does not match tools"));
        }
        if !matches!(finish, Finish::Stop | Finish::ToolUse) {
            return Err(err("unsupported finish reason"));
        }
        let mut native_bytes = 0usize;
        for (index, state) in std::mem::take(&mut self.native) {
            let blocks = state
                .finish()
                .map_err(|_| err("invalid native reasoning"))?;
            let item = self
                .items
                .get(&index)
                .ok_or_else(|| err("native reasoning item missing"))?;
            let payload =
                json!({"protocol":"messages", "endpoint_hash":self.provider, "blocks":blocks});
            let size = serde_json::to_vec(&payload)
                .map_err(|_| err("invalid native reasoning"))?
                .len();
            native_bytes = native_bytes
                .checked_add(size)
                .ok_or_else(|| err("native state exceeds limit"))?;
            if size > 32 * 1024 || native_bytes > MAX_NATIVE {
                return Err(err("native state exceeds limit"));
            }
            batch.native_state.push(NativeRecord {
                item_id: item.id.clone(),
                payload,
            });
        }
        for item in self.items.values() {
            match &item.kind {
                Block::ToolCall { id, name } => {
                    let arguments =
                        if self.protocol == Protocol::Messages && item.content.is_empty() {
                            "{}"
                        } else {
                            &item.content
                        };
                    let restored = self.request.restore_tool_call(name, arguments)?;
                    let value = if restored.freeform {
                        json!({"type":"custom_tool_call", "id":item.id, "call_id":id, "name":restored.name, "namespace":restored.namespace, "input":restored.input})
                    } else {
                        json!({"type":"function_call", "id":item.id, "call_id":id, "name":restored.name, "namespace":restored.namespace, "arguments":restored.input})
                    };
                    let done = parse_item(value)?;
                    batch
                        .events
                        .push(ResponseEvent::OutputItemAdded(done.clone()));
                    batch.events.push(ResponseEvent::OutputItemDone(done));
                }
                Block::Reasoning => {
                    if self.protocol == Protocol::Chat {
                        let payload = json!({"protocol":"chat","endpoint_hash":self.provider,"field":"reasoning_content"});
                        let size = serde_json::to_vec(&payload)
                            .map_err(|_| err("invalid native reasoning"))?
                            .len();
                        native_bytes = native_bytes
                            .checked_add(size)
                            .ok_or_else(|| err("native state exceeds limit"))?;
                        if size > 32 * 1024 || native_bytes > MAX_NATIVE {
                            return Err(err("native state exceeds limit"));
                        }
                        batch.native_state.push(NativeRecord {
                            item_id: item.id.clone(),
                            payload,
                        });
                    }
                    batch
                        .events
                        .push(ResponseEvent::OutputItemAdded(reasoning_item(item, "")?));
                    if !item.content.is_empty() {
                        batch.events.push(ResponseEvent::ReasoningContentDelta {
                            delta: item.content.clone(),
                            content_index: 0,
                        });
                    }
                    batch
                        .events
                        .push(ResponseEvent::OutputItemDone(reasoning_item(
                            item,
                            &item.content,
                        )?));
                }
                Block::Text => {
                    if item.deferred {
                        batch
                            .events
                            .push(ResponseEvent::OutputItemAdded(message_item(item, "")?));
                        if !item.content.is_empty() {
                            batch
                                .events
                                .push(ResponseEvent::OutputTextDelta(item.content.clone()));
                        }
                        batch
                            .events
                            .push(ResponseEvent::OutputItemDone(message_item(
                                item,
                                &item.content,
                            )?));
                    }
                }
                _ => return Err(err("unsupported block")),
            }
        }
        let usage = self.has_usage.then(|| TokenUsage {
            input_tokens: count(self.usage.input_tokens),
            cached_input_tokens: count(self.usage.cached_input_tokens),
            cache_write_input_tokens: count(self.usage.cache_write_input_tokens),
            output_tokens: count(self.usage.output_tokens),
            reasoning_output_tokens: count(self.usage.reasoning_tokens),
            total_tokens: count(
                self.usage
                    .input_tokens
                    .saturating_add(self.usage.output_tokens),
            ),
            codex_rollout_budget_units: None,
        });
        batch.events.push(ResponseEvent::Completed {
            response_id,
            token_usage: usage,
            usage_metadata: None,
            end_turn: Some(!is_tool),
        });
        Ok(batch)
    }
}

fn err(message: &str) -> CodecError {
    CodecError::new(message)
}
fn count(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}
fn parse_item(value: Value) -> Result<ResponseItem, CodecError> {
    serde_json::from_value(value).map_err(|_| err("invalid canonical item"))
}
fn message_item(item: &Item, text: &str) -> Result<ResponseItem, CodecError> {
    parse_item(
        json!({"type":"message", "id":item.id, "role":"assistant", "content":[{"type":"output_text", "text":text}]}),
    )
}
fn reasoning_item(item: &Item, text: &str) -> Result<ResponseItem, CodecError> {
    parse_item(
        json!({"type":"reasoning", "id":item.id, "summary":[], "content":[{"type":"reasoning_text", "text":text}], "encrypted_content":null}),
    )
}
struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;

            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate object keys")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|v| UniqueJson(v.into()))
                    .ok_or_else(|| E::custom("invalid number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJson(value.into()))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueJson(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueJson(values.into()))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueJson(value))) = map.next_entry::<String, UniqueJson>()? {
                    if values.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate key"));
                    }
                }
                Ok(UniqueJson(values.into()))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
