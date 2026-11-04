use codex_api::ResponseEvent;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesApiTools;
use codex_protocol::models::ResponseItem;
use codex_protocol::protocol::TokenUsage;
use codex_provider_codecs::CodecStream;
use codex_provider_codecs::NativeRecord;
use codex_provider_codecs::Protocol;
use codex_provider_codecs::encode_request;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

#[expect(clippy::expect_used, reason = "Invalid fixture data must fail during setup.")]
fn user() -> ResponseItem {
    serde_json::from_value(json!({
        "type":"message", "role":"user",
        "content":[{"type":"input_text","text":"continue"}]
    }))
    .expect("valid gateway fixture")
}

#[expect(clippy::expect_used, reason = "Invalid fixture data must fail during setup.")]
fn request(input: Vec<ResponseItem>) -> ResponsesApiRequest {
    let tools = serde_json::value::RawValue::from_string(
        json!([
            {"type":"function","name":"read","description":"Read",
             "strict":false,"parameters":{"type":"object"}}
        ])
        .to_string(),
    )
    .expect("valid gateway fixture");
    ResponsesApiRequest {
        model: "model".into(),
        stream: true,
        service_tier: None,
        input,
        tools: Some(ResponsesApiTools::from(Arc::from(tools))),
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

fn chat(delta: Value, finish: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({
            "id":"gateway-response","model":"model","object":"chat.completion.chunk",
            "created":1700000000,"system_fingerprint":null,"generationId":"gateway-generation",
            "choices":[{"index":0,"delta":delta,"finish_reason":finish}]
        })
    )
}

fn message(kind: &str, mut body: Value) -> String {
    body["type"] = kind.into();
    format!("event: {kind}\ndata: {body}\n\n")
}

#[test]
fn chat_gateway_metadata_preserves_completed_tool_calls() {
    let encoded = encode_request(Protocol::Chat, &"a".repeat(/*n*/ 64), &request(vec![user()]), &[])
        .expect("valid gateway fixture");
    let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(/*n*/ 64), &encoded);
    for frame in [
        chat(json!({"role":"assistant"}), Value::Null),
        chat(
            json!({"tool_calls":[{"index":0,"id":"call-1","type":"function",
            "function":{"name":"read","arguments":"{}"}}]}),
            Value::Null,
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id":"gateway-response","model":"model",
                "provider_metadata":{"gateway":{"cost":"0","routing":{"provider":"fixture"}}},
                "choices":[{"index":0,"delta":{"provider_metadata":{
                    "gateway":{"generationId":"fixture"}
                }},"finish_reason":"tool_calls"}]
            })
        ),
        format!(
            "data: {}\n\n",
            json!({
                "id":"gateway-response","model":"model","choices":[],
                "system_fingerprint":"fp_changed","service_tier":"default",
                "usage":{
                    "prompt_tokens":2,"completion_tokens":3,"total_tokens":5,
                    "prompt_tokens_details":{"cached_tokens":1,"cache_write_tokens":1,
                        "audio_tokens":0,"video_tokens":0},
                    "completion_tokens_details":{"reasoning_tokens":1,"image_tokens":0},
                    "cost":0.001,"market_cost":0.001,"gateway_cost":0.001,"is_byok":false,
                    "cost_details":{"upstream_inference_cost":0.001},
                    "cache_creation_input_tokens":1
                }
            })
        ),
        "data: [DONE]\n\n".to_owned(),
    ] {
        let batch = codec.push(frame.as_bytes()).expect("valid gateway fixture");
        assert!(!batch.events.iter().any(|event| matches!(
            event,
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall { .. })
        )));
    }
    let batch = codec.finish().expect("valid gateway fixture");
    let calls: Vec<_> = batch
        .events
        .iter()
        .filter_map(|event| match event {
            ResponseEvent::OutputItemDone(ResponseItem::FunctionCall {
                call_id,
                name,
                arguments,
                ..
            }) => Some((call_id.as_str(), name.as_str(), arguments.as_str())),
            _ => None,
        })
        .collect();
    assert_eq!(calls, vec![("call-1", "read", "{}")]);
    assert!(batch.native_state.is_empty());
    let token_usage = batch.events.last().and_then(|event| match event {
        ResponseEvent::Completed { token_usage, .. } => token_usage.as_ref(),
        _ => None,
    });
    assert_eq!(
        token_usage,
        Some(&TokenUsage {
            input_tokens: 2,
            cached_input_tokens: 1,
            cache_write_input_tokens: 1,
            output_tokens: 3,
            reasoning_output_tokens: 1,
            total_tokens: 5,
            ..TokenUsage::default()
        })
    );
}

#[test]
fn gateway_metadata_is_validated_before_tools_commit() {
    for metadata in [
        json!("invalid"),
        json!({"gateway":{"value":"x".repeat(/*n*/ 32 * 1024)}}),
    ] {
        let encoded = encode_request(Protocol::Chat, &"a".repeat(/*n*/ 64), &request(vec![user()]), &[])
            .expect("valid gateway fixture");
        let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(/*n*/ 64), &encoded);
        codec
            .push(
                chat(
                    json!({"tool_calls":[{"index":0,"id":"call-1","type":"function",
            "function":{"name":"read","arguments":"{}"}}]}),
                    Value::Null,
                )
                .as_bytes(),
            )
            .expect("valid gateway fixture");
        assert!(
            codec
                .push(chat(json!({"provider_metadata":metadata}), json!("tool_calls")).as_bytes())
                .is_err()
        );
        assert!(codec.finish().is_err());
    }
}

#[test]
fn malformed_late_chat_metadata_cannot_release_tools() {
    for (field, metadata) in [
        ("created", json!(-1)),
        ("system_fingerprint", json!("x".repeat(/*n*/ 513))),
        ("service_tier", json!([])),
    ] {
        let encoded = encode_request(Protocol::Chat, &"a".repeat(/*n*/ 64), &request(vec![user()]), &[])
            .expect("valid gateway fixture");
        let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(/*n*/ 64), &encoded);
        for frame in [
            chat(
                json!({"tool_calls":[{"index":0,"id":"call-1","type":"function",
                "function":{"name":"read","arguments":"{}"}}]}),
                Value::Null,
            ),
            chat(json!({}), json!("tool_calls")),
        ] {
            codec.push(frame.as_bytes()).expect("valid gateway fixture");
        }
        let mut late = json!({"id":"gateway-response","model":"model","choices":[],"usage":{}});
        late[field] = metadata;
        assert!(codec.push(format!("data: {late}\n\n").as_bytes()).is_err());
        assert!(codec.finish().is_err());
    }
}

#[test]
fn messages_gateway_thinking_survives_private_state_restart_and_replay() {
    for signature in [None, Some("fragmented-signature")] {
        let identity = "a".repeat(/*n*/ 64);
        let encoded = encode_request(Protocol::Messages, &identity, &request(vec![user()]), &[])
            .expect("valid gateway fixture");
        let mut codec = CodecStream::new(Protocol::Messages, &identity, &encoded);
        let mut frames = vec![
            message(
                "message_start",
                json!({"message":{
                    "id":"gateway-response","model":"model","type":"message","role":"assistant",
                    "content":[],"stop_reason":null,"usage":{"input_tokens":2,"output_tokens":0}
                }}),
            ),
            message(
                "content_block_start",
                json!({"index":0,
                    "content_block":{"type":"thinking","thinking":""}
                }),
            ),
            message(
                "content_block_delta",
                json!({"index":0,
                    "delta":{"type":"thinking_delta","thinking":"private reasoning"}
                }),
            ),
        ];
        if let Some(signature) = signature {
            frames.push(message(
                "content_block_delta",
                json!({"index":0,
                    "delta":{"type":"signature_delta","signature":signature}
                }),
            ));
        }
        frames.extend([
            message("content_block_stop", json!({"index":0})),
            message(
                "content_block_start",
                json!({"index":1,
                    "content_block":{"type":"tool_use","id":"call-1","name":"read","input":{}}
                }),
            ),
            message(
                "content_block_delta",
                json!({"index":1,
                    "delta":{"type":"input_json_delta","partial_json":"{}"}
                }),
            ),
            message("content_block_stop", json!({"index":1})),
            message(
                "message_delta",
                json!({
                    "delta":{"stop_reason":"tool_use","stop_sequence":null},
                    "usage":{"output_tokens":4},
                    "provider_metadata":{"gateway":{"cost":"0"},"compat":{"usage":4}}
                }),
            ),
            message("message_stop", json!({})),
        ]);
        for frame in frames {
            codec.push(frame.as_bytes()).expect("valid gateway fixture");
        }
        let batch = codec.finish().expect("valid gateway fixture");
        let mut thinking = json!({"type":"thinking","thinking":"private reasoning"});
        if let Some(signature) = signature {
            thinking["signature"] = signature.into();
        }
        assert_eq!(batch.native_state.len(), 1);
        assert_eq!(
            batch.native_state[0].payload,
            json!({
                "protocol":"messages","endpoint_hash":identity,"blocks":[thinking.clone()]
            })
        );
        let restored: Vec<_> = batch
            .native_state
            .iter()
            .map(|record| NativeRecord {
                item_id: record.item_id.clone(),
                payload: serde_json::from_slice(
                    &serde_json::to_vec(&record.payload).expect("valid gateway fixture"),
                )
                .expect("valid gateway fixture"),
            })
            .collect();
        let mut input: Vec<_> = batch
            .events
            .iter()
            .filter_map(|event| match event {
                ResponseEvent::OutputItemDone(item) => Some(item.clone()),
                _ => None,
            })
            .collect();
        input.push(
            serde_json::from_value(json!({
                "type":"function_call_output","call_id":"call-1","output":"ok"
            }))
            .expect("valid gateway fixture"),
        );
        input.push(user());
        let replay = encode_request(Protocol::Messages, &identity, &request(input), &restored)
            .expect("valid gateway fixture");
        assert_eq!(
            replay.body["messages"][0],
            json!({
                "role":"assistant","content":[thinking,
                    {"type":"tool_use","id":"call-1","name":"read","input":{}}
                ]
            })
        );
        for event in &batch.events {
            if let ResponseEvent::OutputItemAdded(item) | ResponseEvent::OutputItemDone(item) =
                event
            {
                assert!(
                    !serde_json::to_string(item)
                        .expect("valid gateway fixture")
                        .contains("fragmented-signature")
                );
            }
        }
    }
}
