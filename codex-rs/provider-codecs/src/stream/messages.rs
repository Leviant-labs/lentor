use super::CodecStream;
use super::MAX_EVENT;
use super::MessageBlock;
use super::UniqueJson;
use super::err;
use crate::CodecError;
use bytes::Bytes;
use respan_gateway_conversation::Output;
use respan_gateway_http::Decoder;
use respan_gateway_sse::event;
use respan_gateway_sse::event_data;
use serde_json::Value;

impl CodecStream {
    pub(super) fn message_frame(
        &mut self,
        frame: Bytes,
        out: &mut Vec<Output>,
    ) -> Result<(), CodecError> {
        if frame.len() > MAX_EVENT {
            return Err(err("SSE event exceeds limit"));
        }
        std::str::from_utf8(&frame).map_err(|_| err("invalid SSE encoding"))?;
        if self.stop {
            return Err(err("event after message stop"));
        }
        let Some(data) = event_data(&frame) else {
            let text = std::str::from_utf8(&frame).map_err(|_| err("invalid SSE encoding"))?;
            if text
                .lines()
                .all(|line| line.is_empty() || line.starts_with(':'))
            {
                return Ok(());
            }
            return Err(err("invalid SSE event"));
        };
        let mut value = serde_json::from_str::<UniqueJson>(&data)
            .map_err(|_| err("invalid Messages event"))?
            .0;
        crate::metadata::strip_provider_metadata(&mut value).map_err(err)?;
        let kind = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| err("missing event type"))?;
        let mut event_name_seen = false;
        let raw = std::str::from_utf8(&frame).map_err(|_| err("invalid SSE encoding"))?;
        for line in raw.split(['\r', '\n']) {
            if line.is_empty()
                || line.starts_with(':')
                || line == "data"
                || line.starts_with("data:")
            {
                continue;
            }
            if let Some(name) = line.strip_prefix("event:") {
                if event_name_seen || name.strip_prefix(' ').unwrap_or(name) != kind {
                    return Err(err("invalid SSE event name"));
                }
                event_name_seen = true;
            } else {
                return Err(err("unsupported SSE field"));
            }
        }
        if !matches!(
            kind,
            "message_start"
                | "content_block_start"
                | "content_block_delta"
                | "content_block_stop"
                | "message_delta"
                | "message_stop"
                | "ping"
        ) {
            return Err(err("unsupported Messages event"));
        }
        if kind.starts_with("content_block_")
            && value
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|i| u32::try_from(i).ok())
                .is_none()
        {
            return Err(err("invalid block index"));
        }
        if kind == "content_block_delta" {
            let delta = &value["delta"];
            let field = match delta["type"].as_str() {
                Some("text_delta") => "text",
                Some("input_json_delta") => "partial_json",
                Some("thinking_delta") => "thinking",
                Some("signature_delta") => "signature",
                _ => return Err(err("unsupported block delta")),
            };
            if !delta[field].is_string() {
                return Err(err("invalid block delta"));
            }
            if delta
                .as_object()
                .is_none_or(|object| object.keys().any(|key| key != "type" && key != field))
            {
                return Err(err("unsupported block delta field"));
            }
        }
        if kind == "ping" {
            if value
                .as_object()
                .is_none_or(|object| object.keys().any(|key| key != "type"))
            {
                return Err(err("unsupported ping field"));
            }
            return Ok(());
        }
        let index = value
            .get("index")
            .and_then(Value::as_u64)
            .and_then(|i| u32::try_from(i).ok());
        match kind {
            "message_start" => {
                let message = &value["message"];
                if self.message_started
                    || !message.is_object()
                    || message
                        .get("type")
                        .is_some_and(|kind| kind.as_str() != Some("message"))
                    || message
                        .get("role")
                        .is_some_and(|role| role.as_str() != Some("assistant"))
                    || message.get("content").is_some_and(|content| {
                        content.as_array().is_none_or(|items| !items.is_empty())
                    })
                    || message
                        .get("stop_reason")
                        .is_some_and(|reason| !reason.is_null())
                    || message
                        .get("stop_sequence")
                        .is_some_and(|sequence| !sequence.is_null())
                {
                    return Err(err("invalid message start"));
                }
                self.message_started = true;
            }
            "content_block_start" => {
                if !self.message_started || self.message_finished {
                    return Err(err("invalid block start"));
                }
                let index = index.ok_or_else(|| err("invalid block index"))?;
                let block = &value["content_block"];
                let block_kind = match block["type"].as_str() {
                    Some("text") if block["text"].is_string() => MessageBlock::Text,
                    Some("tool_use")
                        if block["id"].as_str().is_some_and(|s| !s.is_empty())
                            && block["name"].as_str().is_some_and(|s| !s.is_empty())
                            && block["input"].is_object() =>
                    {
                        MessageBlock::Tool
                    }
                    Some("thinking")
                        if block["thinking"].is_string()
                            && block.get("signature").is_none_or(Value::is_string) =>
                    {
                        MessageBlock::Thinking
                    }
                    Some("redacted_thinking") if block["data"].is_string() => {
                        MessageBlock::Redacted
                    }
                    _ => return Err(err("unsupported content block")),
                };
                if self.message_blocks.len() >= 1024
                    || self
                        .message_blocks
                        .insert(index, (block_kind, false))
                        .is_some()
                {
                    return Err(err("duplicate or excessive blocks"));
                }
            }
            "content_block_delta" => {
                if self.message_finished {
                    return Err(err("delta after finish"));
                }
                let index = index.ok_or_else(|| err("invalid block index"))?;
                let (block, ended) = self
                    .message_blocks
                    .get(&index)
                    .ok_or_else(|| err("delta without block"))?;
                let expected = match value["delta"]["type"].as_str() {
                    Some("text_delta") => MessageBlock::Text,
                    Some("input_json_delta") => MessageBlock::Tool,
                    Some("thinking_delta" | "signature_delta") => MessageBlock::Thinking,
                    _ => return Err(err("unsupported block delta")),
                };
                if *ended || *block != expected {
                    return Err(err("invalid block delta"));
                }
            }
            "content_block_stop" => {
                if self.message_finished {
                    return Err(err("block end after finish"));
                }
                let index = index.ok_or_else(|| err("invalid block index"))?;
                let (_, ended) = self
                    .message_blocks
                    .get_mut(&index)
                    .ok_or_else(|| err("end without block"))?;
                if *ended {
                    return Err(err("duplicate block end"));
                }
                *ended = true;
            }
            "message_delta" => {
                if !self.message_started || self.message_finished || !value["delta"].is_object() {
                    return Err(err("invalid message delta"));
                }
                if !value["delta"]["stop_reason"].is_null() {
                    if !value["delta"]["stop_reason"].is_string()
                        || self.message_blocks.values().any(|(_, ended)| !ended)
                    {
                        return Err(err("invalid message finish"));
                    }
                    self.message_finished = true;
                }
            }
            "message_stop" => {
                if !self.message_started || !self.message_finished {
                    return Err(err("early message stop"));
                }
                self.stop = true;
            }
            _ => return Err(err("unsupported Messages event")),
        }
        if value.as_object().is_some_and(|object| {
            object.keys().any(|key| !match kind {
                "message_start" => matches!(key.as_str(), "type" | "message"),
                "content_block_start" => matches!(key.as_str(), "type" | "index" | "content_block"),
                "content_block_delta" => matches!(key.as_str(), "type" | "index" | "delta"),
                "content_block_stop" => matches!(key.as_str(), "type" | "index"),
                "message_delta" => matches!(key.as_str(), "type" | "delta" | "usage"),
                "message_stop" => key == "type",
                _ => false,
            })
        }) {
            return Err(err("unsupported event field"));
        }
        self.decoder
            .decode(Bytes::from(event(Some(kind), &value)), out)
            .map_err(|_| err("invalid Messages event"))
    }
}
