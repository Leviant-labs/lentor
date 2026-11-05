use std::collections::BTreeMap;
use std::collections::HashSet;
use std::fmt;

use bytes::Bytes;
use respan_gateway_conversation::Output;
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::openai::OpenAiChat;
use respan_gateway_http::Decoder;
use respan_gateway_sse::Parser;
use respan_gateway_sse::event;
use respan_gateway_sse::event_data;
use serde::Deserialize;
use serde::de::MapAccess;
use serde::de::SeqAccess;
use serde::de::Visitor;
use serde::de::{self};
use serde_json::Value;
use serde_json::json;

const MAX_RESPONSE: usize = 16 * 1024 * 1024;
const MAX_EVENT: usize = 2 * 1024 * 1024;
const MAX_ARGUMENTS: usize = 1024 * 1024;
const MAX_IDENTITY: usize = 128;
const MAX_CALLS: usize = 64;

/// A malformed, truncated, or oversized provider stream.
#[derive(Debug)]
pub struct NormalizeError(String);

impl NormalizeError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for NormalizeError {}

fn invalid(message: impl Into<String>) -> NormalizeError {
    NormalizeError(message.into())
}

#[derive(Default)]
struct Call {
    id: String,
    name: String,
    arguments: String,
    last_id: String,
    last_name: String,
}

/// Bounded Chat SSE normalizer feeding Respan's public decoder.
///
/// Tool events are withheld until clean transport EOF, while text deltas are
/// forwarded immediately. A failed stream must not be reused.
pub struct ChatNormalizer {
    parser: Parser,
    decoder: <OpenAiChat as Schema>::Decoding,
    calls: BTreeMap<u64, Call>,
    bytes: usize,
    last_byte: Option<u8>,
    finished: bool,
    done: bool,
    failed: bool,
    ended: bool,
    staged: Vec<Output>,
    staging: bool,
    unsupported_choice_fields: bool,
}

impl Default for ChatNormalizer {
    fn default() -> Self {
        Self {
            parser: Parser::new(Some(MAX_EVENT)),
            decoder: OpenAiChat.decoder(),
            calls: BTreeMap::new(),
            bytes: 0,
            last_byte: None,
            finished: false,
            done: false,
            failed: false,
            ended: false,
            staged: Vec::new(),
            staging: false,
            unsupported_choice_fields: false,
        }
    }
}

impl ChatNormalizer {
    /// Consume one arbitrary transport chunk, yielding decoded Respan events.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Output>, NormalizeError> {
        if self.failed || self.ended {
            return Err(invalid("stream already failed"));
        }
        let result = self.push_inner(bytes);
        self.failed |= result.is_err();
        result
    }

    fn push_inner(&mut self, bytes: &[u8]) -> Result<Vec<Output>, NormalizeError> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("response size overflow"))?;
        if self.bytes > MAX_RESPONSE {
            return Err(invalid("response exceeds 16 MiB"));
        }
        if let Some(last) = bytes.last() {
            self.last_byte = Some(*last);
        }
        let frames = self
            .parser
            .push(Bytes::copy_from_slice(bytes))
            .map_err(|e| invalid(e.to_string()))?;
        let mut output = Vec::new();
        for frame in frames {
            let mut events = Vec::new();
            self.frame(frame, &mut events)?;
            if self.staging {
                self.staged.extend(events);
            } else {
                output.extend(events);
            }
        }
        Ok(output)
    }

    /// Finish the transport and reject truncated or unfinished calls.
    pub fn end(&mut self) -> Result<Vec<Output>, NormalizeError> {
        if self.failed || self.ended {
            return Err(invalid("stream already failed"));
        }
        let result = self.end_inner();
        self.failed |= result.is_err();
        self.ended |= result.is_ok();
        result
    }

    fn end_inner(&mut self) -> Result<Vec<Output>, NormalizeError> {
        if !matches!(self.last_byte, Some(b'\n' | b'\r')) {
            return Err(invalid("truncated SSE line"));
        }
        let mut output = Vec::new();
        for frame in self.parser.finish() {
            let mut events = Vec::new();
            self.frame(frame, &mut events)?;
            if self.staging {
                self.staged.extend(events);
            } else {
                output.extend(events);
            }
        }
        if !self.calls.is_empty() {
            return Err(invalid("tool calls ended without finish reason"));
        }
        let mut end_events = Vec::new();
        self.decoder
            .end(Ok(()), &mut end_events)
            .map_err(|e| invalid(e.to_string()))?;
        output.append(&mut self.staged);
        output.extend(end_events);
        Ok(output)
    }

    fn frame(&mut self, frame: Bytes, output: &mut Vec<Output>) -> Result<(), NormalizeError> {
        if frame.len() > MAX_EVENT {
            return Err(invalid("SSE event exceeds 2 MiB"));
        }
        let raw = std::str::from_utf8(&frame).map_err(|_| invalid("invalid SSE UTF-8"))?;
        if raw.split(['\r', '\n']).any(|line| {
            !line.is_empty()
                && !line.starts_with(':')
                && line != "data"
                && !line.starts_with("data:")
        }) {
            return Err(invalid("unsupported Chat SSE field"));
        }
        let Some(data) = event_data(&frame) else {
            return Ok(());
        };
        if self.done {
            return Err(invalid("event after DONE"));
        }
        if data.trim() == "[DONE]" {
            if !self.calls.is_empty() || !self.finished {
                return Err(invalid("DONE before finish reason"));
            }
            self.done = true;
            return self
                .decoder
                .decode(frame, output)
                .map_err(|e| invalid(e.to_string()));
        }
        let mut value = serde_json::from_str::<UniqueJson>(&data)
            .map_err(|_| invalid("invalid or ambiguous SSE JSON"))?
            .0;
        let Some(object) = value.as_object_mut() else {
            return Err(invalid("SSE data must be a JSON object"));
        };
        // Normalize service fields on every chunk, including late fingerprints
        // and usage-only chunks, before Respan projects changed fields to Native.
        for field in [
            "created",
            "system_fingerprint",
            "service_tier",
            "generationId",
        ] {
            let Some(metadata) = object.remove(field) else {
                continue;
            };
            let invalid_metadata = if field == "created" {
                metadata.as_u64().is_none()
            } else {
                !metadata.is_null() && metadata.as_str().is_none_or(|text| text.len() > 512)
            };
            if invalid_metadata {
                return Err(invalid("invalid Chat response metadata"));
            }
        }
        // Usage extensions are optional accounting metadata. Project only the
        // counters understood by the codec; text and tool validation is unchanged.
        if let Some(usage) = object.get_mut("usage").and_then(Value::as_object_mut) {
            usage.retain(|field, _| {
                matches!(
                    field.as_str(),
                    "prompt_tokens"
                        | "completion_tokens"
                        | "total_tokens"
                        | "prompt_tokens_details"
                        | "completion_tokens_details"
                )
            });
            for (field, counters) in [
                (
                    "prompt_tokens_details",
                    &["cached_tokens", "cache_write_tokens"][..],
                ),
                ("completion_tokens_details", &["reasoning_tokens"][..]),
            ] {
                if let Some(details) = usage.get_mut(field).and_then(Value::as_object_mut) {
                    details.retain(|counter, _| counters.contains(&counter.as_str()));
                }
            }
        }
        crate::metadata::strip_provider_metadata(&mut value).map_err(invalid)?;
        if value
            .get("choices")
            .is_some_and(|choices| !choices.is_array())
        {
            return Err(invalid("choices must be an array"));
        }
        let Some(choices) = value.get_mut("choices").and_then(Value::as_array_mut) else {
            if value.get("usage").is_none() && value.get("error").is_none() {
                return Err(invalid("unsupported Chat event"));
            }
            return self.decode_json(&value, output);
        };
        if choices.len() > 1 {
            return Err(invalid("multiple choices are unsupported"));
        }
        if let Some(choice) = choices.first_mut() {
            if !choice.is_object()
                || choice
                    .get("delta")
                    .is_some_and(|delta| !delta.is_object() && !delta.is_null())
            {
                return Err(invalid("invalid choice delta"));
            }
            if self.finished {
                return Err(invalid("choice after finish reason"));
            }
            if choice
                .get("index")
                .is_some_and(|index| index.as_u64() != Some(0))
            {
                return Err(invalid("unexpected choice index"));
            }
            if let Some(delta) = choice.get_mut("delta") {
                crate::metadata::strip_provider_metadata(delta).map_err(invalid)?;
            }
            self.unsupported_choice_fields |= choice.as_object().is_some_and(|object| {
                object.keys().any(|key| {
                    !matches!(
                        key.as_str(),
                        "index" | "delta" | "finish_reason" | "logprobs"
                    )
                })
            }) || choice
                .get("delta")
                .and_then(Value::as_object)
                .is_some_and(|delta| {
                    delta.keys().any(|key| {
                        !matches!(
                            key.as_str(),
                            "role"
                                | "content"
                                | "reasoning_content"
                                | "reasoning"
                                | "refusal"
                                | "tool_calls"
                        )
                    })
                });
            if let Some(delta) = choice.get_mut("delta").and_then(Value::as_object_mut)
                && let Some(calls) = delta.remove("tool_calls")
            {
                if self.finished {
                    return Err(invalid("tool call after finish"));
                }
                self.collect(calls)?;
            }
            if !self.calls.is_empty() && self.unsupported_choice_fields {
                return Err(invalid("unsupported tool-related choice or delta field"));
            }
            if choice.get("finish_reason").is_some_and(|v| !v.is_null()) {
                let reason = choice["finish_reason"]
                    .as_str()
                    .ok_or_else(|| invalid("invalid finish reason"))?;
                if !self.calls.is_empty() && !matches!(reason, "tool_calls" | "stop") {
                    return Err(invalid("tool calls terminated without a complete finish"));
                }
                self.finished = true;
                self.staging = !self.calls.is_empty();
                let mut synthetic = value.clone();
                self.flush(&mut synthetic, output)?;
            }
        }
        self.decode_json(&value, output)
    }

    fn collect(&mut self, calls: Value) -> Result<(), NormalizeError> {
        let calls = calls
            .as_array()
            .ok_or_else(|| invalid("tool_calls must be an array"))?;
        let mut seen = HashSet::new();
        for part in calls {
            let part = part
                .as_object()
                .ok_or_else(|| invalid("invalid tool call"))?;
            if part
                .keys()
                .any(|k| !matches!(k.as_str(), "index" | "id" | "type" | "function"))
            {
                return Err(invalid("unsupported tool call fields"));
            }
            let index = part
                .get("index")
                .and_then(Value::as_u64)
                .ok_or_else(|| invalid("tool call index required"))?;
            if index >= MAX_CALLS as u64 {
                return Err(invalid("tool call index exceeds limit"));
            }
            if !seen.insert(index) {
                return Err(invalid("duplicate tool call index in one event"));
            }
            if let Some(kind) = part.get("type")
                && kind != "function"
            {
                return Err(invalid("unsupported tool call type"));
            }
            let call = self.calls.entry(index).or_default();
            append_identity(&mut call.id, &mut call.last_id, part.get("id"), "call ID")?;
            if let Some(function) = part.get("function") {
                let function = function
                    .as_object()
                    .ok_or_else(|| invalid("invalid function fragment"))?;
                if function
                    .keys()
                    .any(|k| !matches!(k.as_str(), "name" | "arguments"))
                {
                    return Err(invalid("unsupported function fields"));
                }
                append_identity(
                    &mut call.name,
                    &mut call.last_name,
                    function.get("name"),
                    "function name",
                )?;
                append(
                    &mut call.arguments,
                    function.get("arguments"),
                    MAX_ARGUMENTS,
                    "arguments",
                )?;
            }
        }
        Ok(())
    }

    fn flush(&mut self, value: &mut Value, output: &mut Vec<Output>) -> Result<(), NormalizeError> {
        if self.calls.is_empty() {
            return Ok(());
        }
        let mut ids = HashSet::new();
        let mut calls = Vec::new();
        for (index, call) in &self.calls {
            if call.id.is_empty() || call.name.is_empty() || !ids.insert(&call.id) {
                return Err(invalid("missing or duplicate tool identity"));
            }
            let args: Value = serde_json::from_str(&call.arguments)
                .map_err(|_| invalid("incomplete tool arguments"))?;
            if !args.is_object() {
                return Err(invalid("tool arguments must be an object"));
            }
            calls.push(json!({"index":index,"id":call.id,"type":"function","function":{"name":call.name,"arguments":call.arguments}}));
        }
        let choice = &mut value["choices"][0];
        choice["finish_reason"] = Value::Null;
        choice["delta"] = json!({"tool_calls":calls});
        if let Some(object) = value.as_object_mut() {
            object.remove("usage");
        }
        self.decode_json(value, output)?;
        self.calls.clear();
        Ok(())
    }

    fn decode_json(
        &mut self,
        value: &Value,
        output: &mut Vec<Output>,
    ) -> Result<(), NormalizeError> {
        self.decoder
            .decode(Bytes::from(event(None, value)), output)
            .map_err(|e| invalid(e.to_string()))
    }
}

// JSON object keys must be unique, including within tool identity fragments.
struct UniqueJson(Value);

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

fn append(
    target: &mut String,
    fragment: Option<&Value>,
    limit: usize,
    label: &str,
) -> Result<(), NormalizeError> {
    let Some(fragment) = fragment else {
        return Ok(());
    };
    let fragment = fragment
        .as_str()
        .ok_or_else(|| invalid(format!("invalid {label} fragment")))?;
    if label != "arguments" && !target.is_empty() && fragment == target {
        return Err(invalid(format!("ambiguous repeated {label}")));
    }
    if target.len().saturating_add(fragment.len()) > limit {
        return Err(invalid(format!("{label} exceeds limit")));
    }
    target.push_str(fragment);
    Ok(())
}

// Equal consecutive nonempty fragments are ambiguous with repeated snapshots.
// This deliberately rejects legitimate identities with identical adjacent fragments.
fn append_identity(
    target: &mut String,
    last: &mut String,
    fragment: Option<&Value>,
    label: &str,
) -> Result<(), NormalizeError> {
    if let Some(value) = fragment {
        let text = value
            .as_str()
            .ok_or_else(|| invalid(format!("invalid {label} fragment")))?;
        if !text.is_empty() && text == last.as_str() {
            return Err(invalid(format!("ambiguous repeated {label} fragment")));
        }
        if !text.is_empty() {
            *last = text.to_owned();
        }
    }
    append(target, fragment, MAX_IDENTITY, label)
}
