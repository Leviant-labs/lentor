//! Replay the plain reasoning emitted by the Chat stream in Chat's native field.

use crate::CodecError;
use respan_gateway_conversation::Message;
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::openai::OpenAiChat;
use serde_json::Value;
use serde_json::json;

pub(crate) fn reasoning_message(item: &Value) -> Result<Message, CodecError> {
    if item
        .get("encrypted_content")
        .is_some_and(|value| !value.is_null())
        || item
            .get("summary")
            .and_then(Value::as_array)
            .is_none_or(|parts| !parts.is_empty())
    {
        return Err(CodecError::new(
            "unsupported opaque or summarized Chat reasoning",
        ));
    }
    let parts = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| CodecError::new("Chat reasoning text missing"))?;
    let mut text = String::new();
    for part in parts {
        if !matches!(part["type"].as_str(), Some("reasoning_text" | "text"))
            || part
                .as_object()
                .is_none_or(|fields| fields.keys().any(|key| key != "type" && key != "text"))
        {
            return Err(CodecError::new("unsupported Chat reasoning part"));
        }
        text.push_str(
            part["text"]
                .as_str()
                .ok_or_else(|| CodecError::new("invalid Chat reasoning text"))?,
        );
    }
    let mut decoded = OpenAiChat.decode_json(
        &json!({"messages":[{"role":"assistant","content":null,"reasoning_content":text}]}),
    );
    decoded
        .messages
        .pop()
        .ok_or_else(|| CodecError::new("invalid native Chat reasoning"))
}

pub(crate) fn has_reasoning_field(message: &Message) -> bool {
    message.native.as_ref().is_some_and(|native| {
        native.format.as_ref() == OpenAiChat::NAME
            && native.fields.len() == 1
            && native
                .fields
                .get("reasoning_content")
                .is_some_and(Value::is_string)
    })
}
