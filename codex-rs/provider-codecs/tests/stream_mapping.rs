use codex_api::ResponseEvent;
use codex_api::ResponsesApiRequest;
use codex_api::ResponsesApiTools;
use codex_protocol::models::ResponseItem;
use codex_provider_codecs::CodecStream;
use codex_provider_codecs::Protocol;
use codex_provider_codecs::encode_request;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

#[expect(
    clippy::unwrap_used,
    reason = "Invalid fixture data must fail the test during setup."
)]
fn encoded(protocol: Protocol) -> codex_provider_codecs::EncodedRequest {
    let user = serde_json::from_value(
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"continue"}]}),
    )
    .unwrap();
    encode_request(protocol, &"a".repeat(64), &request(vec![user]), &[]).unwrap()
}

#[expect(
    clippy::unwrap_used,
    reason = "Invalid fixture data must fail the test during setup."
)]
fn request(input: Vec<ResponseItem>) -> ResponsesApiRequest {
    let tools = json!([
        {"type":"function","name":"read","description":"Read","strict":false,"parameters":{"type":"object"}},
        {"type":"custom","name":"patch","description":"Patch","format":{"type":"text"}}
    ]);
    let raw = serde_json::value::RawValue::from_string(tools.to_string()).unwrap();
    ResponsesApiRequest {
        model: "model".into(),
        stream: true,
        service_tier: None,
        input,
        tools: Some(ResponsesApiTools::from(Arc::from(raw))),
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

fn sse(value: Value) -> String {
    format!("data: {value}\n\n")
}
fn message(kind: &str, body: Value) -> String {
    let mut value = body;
    value["type"] = kind.into();
    sse(value)
}
fn tool_events(events: &[ResponseEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                ResponseEvent::OutputItemAdded(
                    ResponseItem::FunctionCall { .. } | ResponseItem::CustomToolCall { .. }
                ) | ResponseEvent::OutputItemDone(
                    ResponseItem::FunctionCall { .. } | ResponseItem::CustomToolCall { .. }
                ) | ResponseEvent::ToolCallInputDelta { .. }
            )
        })
        .count()
}

#[test]
fn messages_parallel_tools_are_released_only_at_clean_eof() {
    let request = encoded(Protocol::Messages);
    let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &request);
    let frames = [
        message(
            "message_start",
            json!({"message":{"id":"r","model":"m","role":"assistant","usage":{"input_tokens":12}}}),
        ),
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"text","text":"hello"}}),
        ),
        message("content_block_stop", json!({"index":0})),
        message(
            "content_block_start",
            json!({"index":1,"content_block":{"type":"tool_use","id":"c1","name":"read","input":{}}}),
        ),
        message(
            "content_block_delta",
            json!({"index":1,"delta":{"type":"input_json_delta","partial_json":"{\"path\":\"x\"}"}}),
        ),
        message("content_block_stop", json!({"index":1})),
        message(
            "content_block_start",
            json!({"index":2,"content_block":{"type":"tool_use","id":"c2","name":"patch","input":{}}}),
        ),
        message(
            "content_block_delta",
            json!({"index":2,"delta":{"type":"input_json_delta","partial_json":"{\"input\":\"raw\\ntext\"}"}}),
        ),
        message("content_block_stop", json!({"index":2})),
        message(
            "message_delta",
            json!({"delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":4}}),
        ),
        message("message_stop", json!({})),
    ];
    for frame in &frames {
        for byte in frame.as_bytes() {
            assert_eq!(tool_events(&codec.push(&[*byte]).unwrap().events), 0);
        }
    }
    let batch = codec.finish().unwrap();
    assert_eq!(tool_events(&batch.events), 4);
    assert!(batch.events.iter().any(|e| matches!(e, ResponseEvent::OutputItemDone(ResponseItem::CustomToolCall { call_id, input, .. }) if call_id == "c2" && input == "raw\ntext")));
    assert!(batch.events.iter().any(|e| matches!(e, ResponseEvent::Completed { end_turn: Some(false), token_usage: Some(u), .. } if u.input_tokens == 12 && u.output_tokens == 4)));
}

#[test]
fn messages_trailer_poison_and_no_tool_events() {
    let request = encoded(Protocol::Messages);
    let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &request);
    for frame in [
        message(
            "message_start",
            json!({"message":{"id":"r","model":"m","usage":{}}}),
        ),
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"tool_use","id":"c","name":"read","input":{}}}),
        ),
        message(
            "content_block_delta",
            json!({"index":0,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        ),
        message("content_block_stop", json!({"index":0})),
        message(
            "message_delta",
            json!({"delta":{"stop_reason":"tool_use"},"usage":{}}),
        ),
        message("message_stop", json!({})),
    ] {
        assert_eq!(
            tool_events(&codec.push(frame.as_bytes()).unwrap().events),
            0
        );
    }
    assert!(codec.push(b"data: {bad}\n\n").is_err());
    assert!(codec.finish().is_err());
    assert!(codec.push(b"").is_err());
}

#[test]
fn messages_signed_reasoning_attaches_private_state_to_same_item() {
    let encoded_request = encoded(Protocol::Messages);
    let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded_request);
    for frame in [
        message(
            "message_start",
            json!({"message":{"id":"r","model":"m","usage":{}}}),
        ),
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"thinking","thinking":"why","signature":"","extension":{"keep":null}}}),
        ),
        message(
            "content_block_delta",
            json!({"index":0,"delta":{"type":"signature_delta","signature":"sig-"}}),
        ),
        message(
            "content_block_delta",
            json!({"index":0,"delta":{"type":"signature_delta","signature":"part"}}),
        ),
        message("content_block_stop", json!({"index":0})),
        message(
            "message_delta",
            json!({"delta":{"stop_reason":"end_turn"},"usage":{}}),
        ),
        message("message_stop", json!({})),
    ] {
        codec.push(frame.as_bytes()).unwrap();
    }
    let batch = codec.finish().unwrap();
    assert_eq!(batch.native_state.len(), 1);
    assert_eq!(
        batch.native_state[0].payload["blocks"][0]["signature"],
        "sig-part"
    );
    assert_eq!(
        batch.native_state[0].payload["blocks"][0]["extension"],
        json!({"keep":null})
    );
    let id = &batch.native_state[0].item_id;
    assert!(batch.events.iter().any(|e| matches!(e, ResponseEvent::OutputItemDone(ResponseItem::Reasoning { id: Some(item_id), encrypted_content: None, .. }) if item_id.to_string() == *id)));
    for event in &batch.events {
        if let ResponseEvent::OutputItemAdded(item) | ResponseEvent::OutputItemDone(item) = event {
            let public = serde_json::to_string(item).unwrap();
            assert!(!public.contains("sig-part"));
            assert!(!public.contains("extension"));
        }
    }
    let done = batch
        .events
        .iter()
        .find_map(|event| match event {
            ResponseEvent::OutputItemDone(item @ ResponseItem::Reasoning { .. }) => {
                Some(item.clone())
            }
            _ => None,
        })
        .unwrap();
    let replay = encode_request(
        Protocol::Messages,
        &"a".repeat(64),
        &request(vec![done]),
        &batch.native_state,
    )
    .unwrap();
    assert_eq!(
        replay.body["messages"][0]["content"],
        batch.native_state[0].payload["blocks"]
    );
}

#[test]
fn chat_parallel_tool_stream_waits_for_eof() {
    let request = encoded(Protocol::Chat);
    let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(64), &request);
    let first = sse(
        json!({"id":"r","model":"m","choices":[{"index":0,"delta":{"content":"hi","tool_calls":[{"index":0,"id":"c1","type":"function","function":{"name":"read","arguments":"{}"}},{"index":1,"id":"c2","type":"function","function":{"name":"patch","arguments":"{\"input\":\"raw\"}"}}]},"finish_reason":null}]}),
    );
    let end = sse(
        json!({"id":"r","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":2,"completion_tokens":3,"total_tokens":5}}),
    );
    for frame in [first.as_str(), end.as_str(), "data: [DONE]\n\n"] {
        assert_eq!(
            tool_events(&codec.push(frame.as_bytes()).unwrap().events),
            0
        );
    }
    let batch = codec.finish().unwrap();
    assert_eq!(tool_events(&batch.events), 4);
    assert!(batch.events.iter().any(|e| matches!(
        e,
        ResponseEvent::Completed {
            end_turn: Some(false),
            ..
        }
    )));
}

#[test]
fn messages_redacted_state_is_available_before_matching_done_and_replays() {
    let encoded = encoded(Protocol::Messages);
    let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
    for frame in [
        message(
            "message_start",
            json!({"message":{"id":"redacted","model":"m","usage":{}}}),
        ),
        message(
            "content_block_start",
            json!({"index":1,"content_block":{"type":"redacted_thinking","data":"opaque","extension":{"x":1}}}),
        ),
        message("content_block_stop", json!({"index":1})),
        message(
            "message_delta",
            json!({"delta":{"stop_reason":"end_turn"},"usage":{}}),
        ),
        message("message_stop", json!({})),
    ] {
        let batch = codec.push(frame.as_bytes()).unwrap();
        assert!(batch.native_state.is_empty());
        assert!(!batch.events.iter().any(|event| matches!(
            event,
            ResponseEvent::OutputItemDone(ResponseItem::Reasoning { .. })
                | ResponseEvent::Completed { .. }
        )));
    }
    let batch = codec.finish().unwrap();
    assert_eq!(batch.native_state.len(), 1);
    assert_eq!(
        batch.native_state[0].payload,
        json!({"protocol":"messages","endpoint_hash":"a".repeat(64),"blocks":[{"type":"redacted_thinking","data":"opaque","extension":{"x":1}}]})
    );
    let done = batch
        .events
        .iter()
        .find_map(|event| match event {
            ResponseEvent::OutputItemDone(
                item @ ResponseItem::Reasoning {
                    id: Some(id),
                    encrypted_content: None,
                    ..
                },
            ) if id.as_str() == batch.native_state[0].item_id => Some(item.clone()),
            _ => None,
        })
        .expect("native state precedes the matching completed item in the returned batch");
    assert_eq!(
        batch
            .events
            .iter()
            .filter(|event| matches!(
                event,
                ResponseEvent::Completed {
                    end_turn: Some(true),
                    ..
                }
            ))
            .count(),
        1
    );
    let replay = encode_request(
        Protocol::Messages,
        &"a".repeat(64),
        &request(vec![done]),
        &batch.native_state,
    )
    .unwrap();
    assert!(replay.body.to_string().contains("opaque"));
    assert!(replay.body.to_string().contains("extension"));
    assert!(codec.finish().is_err());
}

#[test]
fn messages_require_terminal_and_reject_trailing_or_duplicate_terminal() {
    let encoded = encoded(Protocol::Messages);
    let start = message(
        "message_start",
        json!({"message":{"id":"r","model":"m","usage":{}}}),
    );
    let finished = message(
        "message_delta",
        json!({"delta":{"stop_reason":"end_turn"},"usage":{}}),
    );
    let stop = message("message_stop", json!({}));
    let mut missing = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
    for frame in [&start, &finished] {
        assert!(
            !missing
                .push(frame.as_bytes())
                .unwrap()
                .events
                .iter()
                .any(|e| matches!(e, ResponseEvent::Completed { .. }))
        );
    }
    assert!(missing.finish().is_err());
    assert!(missing.finish().is_err());
    for trailer in [
        stop.as_str(),
        "data: {bad}\n\n",
        "garbage\n",
        "data: {\"type\":\"ping\"}\n\n",
    ] {
        let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
        for frame in [&start, &finished, &stop] {
            let batch = codec.push(frame.as_bytes()).unwrap();
            assert!(
                !batch
                    .events
                    .iter()
                    .any(|e| matches!(e, ResponseEvent::Completed { .. }))
            );
        }
        let _ = codec.push(trailer.as_bytes());
        assert!(codec.finish().is_err());
    }
    let mut duplicate_finish = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
    duplicate_finish.push(start.as_bytes()).unwrap();
    duplicate_finish.push(finished.as_bytes()).unwrap();
    assert!(duplicate_finish.push(finished.as_bytes()).is_err());
    assert!(duplicate_finish.finish().is_err());
}

#[test]
fn messages_reject_unsupported_or_malformed_native_blocks_and_errors() {
    let encoded = encoded(Protocol::Messages);
    let start = message(
        "message_start",
        json!({"message":{"id":"r","model":"m","usage":{}}}),
    );
    for bad in [
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"server_tool_use","id":"x","name":"x","input":{}}}),
        ),
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"redacted_thinking","data":42}}),
        ),
        message("error", json!({"error":{"message":"provider secret"}})),
        "event: message_stop\ndata: {\"type\":\"ping\"}\n\n".to_owned(),
        "data: {\"type\":\"message_stop\",\"type\":\"message_stop\"}\n\n".to_owned(),
    ] {
        let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
        codec.push(start.as_bytes()).unwrap();
        let error = match codec.push(bad.as_bytes()) {
            Ok(_) => panic!("unexpected acceptance"),
            Err(error) => error,
        };
        assert!(!error.to_string().contains("provider secret"));
        assert!(codec.finish().is_err());
    }
}

#[test]
fn messages_malformed_tool_trailer_never_releases_tool() {
    let encoded = encoded(Protocol::Messages);
    for trailer in [
        message(
            "content_block_delta",
            json!({"index":0,"delta":{"type":"input_json_delta","partial_json":"{}","unexpected":true}}),
        ),
        message("content_block_stop", json!({"index":1})),
        "data: {\"type\":\"message_delta\",\"delta\":\n".to_owned(),
    ] {
        let mut codec = CodecStream::new(Protocol::Messages, &"a".repeat(64), &encoded);
        for frame in [
            message(
                "message_start",
                json!({"message":{"id":"r","model":"m","usage":{}}}),
            ),
            message(
                "content_block_start",
                json!({"index":0,"content_block":{"type":"tool_use","id":"c","name":"read","input":{}}}),
            ),
        ] {
            assert_eq!(
                tool_events(&codec.push(frame.as_bytes()).unwrap().events),
                0
            );
        }
        if let Ok(batch) = codec.push(trailer.as_bytes()) {
            assert_eq!(tool_events(&batch.events), 0);
        }
        assert!(codec.finish().is_err());
        assert!(codec.push(b"").is_err());
    }
}

#[test]
fn stream_rejects_an_incompatible_request_identity() {
    let request = encoded(Protocol::Chat);
    for (protocol, provider) in [
        (Protocol::Messages, "a".repeat(64)),
        (Protocol::Chat, "b".repeat(64)),
    ] {
        let mut codec = CodecStream::new(protocol, &provider, &request);
        assert!(codec.push(b"data: {}\n\n").is_err());
        assert!(codec.finish().is_err());
    }
}

#[test]
fn chat_malformed_trailers_and_ambiguous_wrappers_never_release_tools() {
    let request = encoded(Protocol::Chat);
    for arguments in [
        "{\"input\":\"first\",\"input\":\"second\"}",
        "{\"input\":\"ok\"}",
    ] {
        let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(64), &request);
        let frames = [
            sse(
                json!({"id":"r","model":"m","choices":[{"index":0,"delta":{"content":"prompt","tool_calls":[{"index":0,"id":"exact-call","function":{"name":"patch","arguments":arguments}}]},"finish_reason":null}]}),
            ),
            sse(
                json!({"id":"r","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
            ),
            "data: [DONE]\n\n".to_owned(),
        ];
        for frame in frames {
            assert_eq!(
                tool_events(&codec.push(frame.as_bytes()).unwrap().events),
                0
            );
        }
        if arguments == "{\"input\":\"ok\"}" {
            assert!(codec.push(b"data: {bad}\n\n").is_err());
        }
        assert!(codec.finish().is_err());
        assert!(codec.finish().is_err());
    }
}

#[test]
fn chat_non_data_trailer_cannot_release_a_valid_tool() {
    let request = encoded(Protocol::Chat);
    let mut codec = CodecStream::new(Protocol::Chat, &"a".repeat(64), &request);
    for frame in [
        sse(
            json!({"id":"r","model":"m","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"c","function":{"name":"read","arguments":"{}"}}]},"finish_reason":null}]}),
        ),
        sse(
            json!({"id":"r","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ),
        "data: [DONE]\n\n".to_owned(),
    ] {
        assert_eq!(
            tool_events(&codec.push(frame.as_bytes()).unwrap().events),
            0
        );
    }
    if let Ok(batch) = codec.push(b"garbage\n") {
        assert_eq!(tool_events(&batch.events), 0);
    }
    assert!(codec.finish().is_err());
}

#[test]
fn chat_reasoning_and_both_tools_round_trip_through_canonical_history() {
    let original = encoded(Protocol::Chat);
    let identity = "a".repeat(64);
    let mut codec = CodecStream::new(Protocol::Chat, &identity, &original);
    let mut items = Vec::new();
    for frame in [
        sse(
            json!({"id":"reasoning-turn","model":"m","choices":[{"index":0,"delta":{"reasoning_content":"first\n"},"finish_reason":null}]}),
        ),
        sse(
            json!({"id":"reasoning-turn","model":"m","choices":[{"index":0,"delta":{"reasoning_content":"second","tool_calls":[{"index":0,"id":"read-exact","function":{"name":"read","arguments":"{\"path\":\"x\"}"}},{"index":1,"id":"patch-exact","function":{"name":"patch","arguments":"{\"input\":\"raw\\ntext\"}"}}]},"finish_reason":null}]}),
        ),
        sse(
            json!({"id":"reasoning-turn","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
        ),
        "data: [DONE]\n\n".to_owned(),
    ] {
        let batch = codec.push(frame.as_bytes()).unwrap();
        assert_eq!(tool_events(&batch.events), 0);
        for event in batch.events {
            if let ResponseEvent::OutputItemDone(item) = event {
                items.push(item);
            }
        }
    }
    let batch = codec.finish().unwrap();
    assert_eq!(batch.native_state.len(), 1);
    for event in batch.events {
        if let ResponseEvent::OutputItemDone(item) = event {
            items.push(item);
        }
    }
    assert!(items.iter().any(|item| matches!(item, ResponseItem::CustomToolCall { call_id, name, input, .. } if call_id == "patch-exact" && name == "patch" && input == "raw\ntext")));
    items.push(
        serde_json::from_value(
            json!({"type":"function_call_output","call_id":"read-exact","output":"read-result"}),
        )
        .unwrap(),
    );
    items.push(serde_json::from_value(json!({"type":"custom_tool_call_output","call_id":"patch-exact","output":"patch-result"})).unwrap());
    let continued = request(items.clone());
    let replay =
        encode_request(Protocol::Chat, &identity, &continued, &batch.native_state).unwrap();
    assert_eq!(
        replay.body["messages"][0],
        json!({"role":"assistant","content":null,"reasoning_content":"first\nsecond","tool_calls":[{"type":"function","id":"read-exact","function":{"name":"read","arguments":"{\"path\":\"x\"}"}},{"type":"function","id":"patch-exact","function":{"name":"patch","arguments":"{\"input\":\"raw\\ntext\"}"}}]})
    );
    assert_eq!(
        replay.body["messages"][1],
        json!({"role":"tool","tool_call_id":"read-exact","content":"read-result"})
    );
    assert_eq!(
        replay.body["messages"][2],
        json!({"role":"tool","tool_call_id":"patch-exact","content":"patch-result"})
    );
    assert!(
        encode_request(
            Protocol::Chat,
            &"b".repeat(64),
            &continued,
            &batch.native_state
        )
        .is_err()
    );
    assert!(encode_request(Protocol::Chat, &identity, &continued, &[]).is_err());
    let mut opaque = continued.clone();
    for item in &mut opaque.input {
        if let ResponseItem::Reasoning {
            encrypted_content, ..
        } = item
        {
            *encrypted_content = Some("opaque".into());
        }
    }
    assert!(encode_request(Protocol::Chat, &identity, &opaque, &batch.native_state).is_err());
    let mut unknown = batch.native_state;
    unknown[0].payload["signature"] = json!("unsupported");
    assert!(encode_request(Protocol::Chat, &identity, &continued, &unknown).is_err());
}

#[test]
fn reasoning_and_following_text_reach_the_consumer_as_ordered_complete_items() {
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let frames = match protocol {
            Protocol::Chat => vec![
                sse(
                    json!({"id":"ordered","model":"m","choices":[{"index":0,"delta":{"reasoning_content":"why"},"finish_reason":null}]}),
                ),
                sse(
                    json!({"id":"ordered","model":"m","choices":[{"index":0,"delta":{"content":"answer"},"finish_reason":null}]}),
                ),
                sse(
                    json!({"id":"ordered","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                ),
                "data: [DONE]\n\n".into(),
            ],
            Protocol::Messages => vec![
                message(
                    "message_start",
                    json!({"message":{"id":"ordered","model":"m","usage":{}}}),
                ),
                message(
                    "content_block_start",
                    json!({"index":0,"content_block":{"type":"thinking","thinking":"why","signature":"sig"}}),
                ),
                message("content_block_stop", json!({"index":0})),
                message(
                    "content_block_start",
                    json!({"index":1,"content_block":{"type":"text","text":"answer"}}),
                ),
                message("content_block_stop", json!({"index":1})),
                message(
                    "content_block_start",
                    json!({"index":2,"content_block":{"type":"text","text":"more"}}),
                ),
                message("content_block_stop", json!({"index":2})),
                message(
                    "message_delta",
                    json!({"delta":{"stop_reason":"end_turn"},"usage":{}}),
                ),
                message("message_stop", json!({})),
            ],
        };
        let request = encoded(protocol);
        let mut codec = CodecStream::new(protocol, &"a".repeat(64), &request);
        let mut events = Vec::new();
        for frame in frames {
            let batch = codec.push(frame.as_bytes()).unwrap();
            assert!(batch.events.iter().all(|event| matches!(
                event,
                ResponseEvent::Created { .. } | ResponseEvent::ServerModel(_)
            )));
            events.extend(batch.events);
        }
        let batch = codec.finish().unwrap();
        assert_eq!(batch.native_state.len(), 1);
        events.extend(batch.events);
        let mut active = None;
        let mut completed = Vec::new();
        for event in events {
            match event {
                ResponseEvent::OutputItemAdded(item) => {
                    assert!(
                        active.is_none(),
                        "a later item bypassed an uncommitted item"
                    );
                    active = item.id().cloned();
                }
                ResponseEvent::OutputItemDone(item) => {
                    assert_eq!(active.take(), item.id().cloned());
                    completed.push(match item {
                        ResponseItem::Reasoning { .. } => "reasoning",
                        ResponseItem::Message { .. } => "text",
                        _ => panic!("unexpected completed item"),
                    });
                }
                ResponseEvent::OutputTextDelta(_) | ResponseEvent::ReasoningContentDelta { .. } => {
                    assert!(active.is_some())
                }
                _ => {}
            }
        }
        assert!(active.is_none());
        assert_eq!(
            completed,
            match protocol {
                Protocol::Chat => vec!["reasoning", "text"],
                Protocol::Messages => vec!["reasoning", "text", "text"],
            }
        );
    }
}

#[test]
fn messages_signed_thinking_and_tool_result_encode_as_one_assistant_turn() {
    let original = encoded(Protocol::Messages);
    let identity = "a".repeat(64);
    let mut codec = CodecStream::new(Protocol::Messages, &identity, &original);
    for frame in [
        message(
            "message_start",
            json!({"message":{"id":"signed","model":"m","usage":{}}}),
        ),
        message(
            "content_block_start",
            json!({"index":0,"content_block":{"type":"thinking","thinking":"why","signature":"sig"}}),
        ),
        message("content_block_stop", json!({"index":0})),
        message(
            "content_block_start",
            json!({"index":1,"content_block":{"type":"tool_use","id":"read-id","name":"read","input":{}}}),
        ),
        message(
            "content_block_delta",
            json!({"index":1,"delta":{"type":"input_json_delta","partial_json":"{}"}}),
        ),
        message("content_block_stop", json!({"index":1})),
        message(
            "message_delta",
            json!({"delta":{"stop_reason":"tool_use"},"usage":{}}),
        ),
        message("message_stop", json!({})),
    ] {
        codec.push(frame.as_bytes()).unwrap();
    }
    let batch = codec.finish().unwrap();
    let mut input = batch
        .events
        .into_iter()
        .filter_map(|event| match event {
            ResponseEvent::OutputItemDone(item) => Some(item),
            _ => None,
        })
        .collect::<Vec<_>>();
    input.push(
        serde_json::from_value(
            json!({"type":"function_call_output","call_id":"read-id","output":"read contents"}),
        )
        .unwrap(),
    );
    let replay = encode_request(
        Protocol::Messages,
        &identity,
        &request(input),
        &batch.native_state,
    )
    .unwrap();
    assert_eq!(
        replay.body["messages"][0],
        json!({"role":"assistant","content":[
            {"type":"thinking","thinking":"why","signature":"sig"},
            {"type":"tool_use","id":"read-id","name":"read","input":{}}
        ]})
    );
    assert_eq!(
        replay.body["messages"][1]["content"][0]["tool_use_id"],
        "read-id"
    );
}
