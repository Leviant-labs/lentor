use codex_provider_codecs::ChatNormalizer;
use pretty_assertions::assert_eq;
use respan_gateway_conversation::Block;
use respan_gateway_conversation::Delta;
use respan_gateway_conversation::Output;
use serde_json::Value;
use serde_json::json;

fn frame(delta: Value, finish: Value) -> String {
    format!(
        "data: {}\n\n",
        json!({"id":"r","model":"m","choices":[{"index":0,"delta":delta,"finish_reason":finish}]})
    )
}

#[test]
fn fragmented_parallel_calls_are_delivered_exactly_before_finish() {
    let chunks = [
        frame(
            json!({"content":"hello","tool_calls":[{"index":0,"id":"call_","type":"function","function":{"name":"get_","arguments":"{\"q\":"}},{"index":1,"id":"other","type":"function","function":{"name":"sum","arguments":"{\"n\":"}}]}),
            Value::Null,
        ),
        frame(
            json!({"tool_calls":[{"index":1,"function":{"arguments":"2}"}},{"index":0,"id":"one","function":{"name":"weather","arguments":"\"x\"}"}}]}),
            Value::Null,
        ),
        frame(json!({}), json!("tool_calls")),
        "data: [DONE]\n\n".to_owned(),
    ];
    let mut normalizer = ChatNormalizer::default();
    let mut events = Vec::new();
    for chunk in chunks {
        for byte in chunk.as_bytes() {
            events.extend(normalizer.push(&[*byte]).unwrap());
        }
    }
    events.extend(normalizer.end().unwrap());
    let calls: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Output::BlockStart {
                kind: Block::ToolCall { id, name },
                ..
            } => Some((id.as_str(), name.as_str())),
            _ => None,
        })
        .collect();
    let args: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Output::Delta {
                delta: Delta::Arguments(text),
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(calls, [("call_one", "get_weather"), ("other", "sum")]);
    assert_eq!(args, ["{\"q\":\"x\"}", "{\"n\":2}"]);
    assert!(
        events.iter().any(
            |e| matches!(e, Output::Delta { delta: Delta::Text(text), .. } if text == "hello")
        )
    );
}

#[test]
fn delayed_name_is_accepted_and_calls_wait_for_finish() {
    let mut codec = ChatNormalizer::default();
    let first = frame(
        json!({"tool_calls":[{"index":0,"id":"call","function":{"arguments":"{}"}}]}),
        Value::Null,
    );
    let second = frame(
        json!({"tool_calls":[{"index":0,"function":{"name":"run"}}]}),
        Value::Null,
    );
    let mut events = codec.push(first.as_bytes()).unwrap();
    events.extend(codec.push(second.as_bytes()).unwrap());
    assert!(!events.iter().any(|e| matches!(
        e,
        Output::BlockStart {
            kind: Block::ToolCall { .. },
            ..
        }
    )));
    events.extend(
        codec
            .push(frame(json!({}), json!("tool_calls")).as_bytes())
            .unwrap(),
    );
    events.extend(codec.push(b"data: [DONE]\n\n").unwrap());
    assert!(!events.iter().any(|e| matches!(
        e,
        Output::BlockStart {
            kind: Block::ToolCall { .. },
            ..
        }
    )));
    events.extend(codec.end().unwrap());
    assert!(events.iter().any(|e| matches!(e, Output::BlockStart { kind: Block::ToolCall { id, name }, .. } if id == "call" && name == "run")));
    assert!(codec.end().is_err());
}

#[test]
fn tool_calls_wait_for_clean_eof_and_bad_trailers_poison_stream() {
    for trailer in [
        b"data: {bad}\n\n".as_slice(),
        b"data: [DONE]\n\ndata: [DONE]\n\n".as_slice(),
        b"data: {\"incomplete\":".as_slice(),
    ] {
        let mut codec = ChatNormalizer::default();
        let first = frame(
            json!({"content":"visible","tool_calls":[{"index":0,"id":"call","function":{"name":"run","arguments":"{}"}}]}),
            Value::Null,
        );
        let events = codec.push(first.as_bytes()).unwrap();
        assert!(events.iter().any(|event| matches!(event, Output::Delta { delta: Delta::Text(text), .. } if text == "visible")));
        let events = codec
            .push(frame(json!({}), json!("tool_calls")).as_bytes())
            .unwrap();
        assert!(!events.iter().any(|event| matches!(
            event,
            Output::BlockStart {
                kind: Block::ToolCall { .. },
                ..
            }
        )));
        let _ = codec.push(trailer);
        assert!(codec.end().is_err());
    }
}

#[test]
fn rejects_unknown_delta_fields_when_a_tool_call_follows() {
    let mut codec = ChatNormalizer::default();
    codec
        .push(frame(json!({"provider_tool_mode":true}), Value::Null).as_bytes())
        .unwrap();
    let call = frame(
        json!({"tool_calls":[{"index":0,"id":"call","function":{"name":"run","arguments":"{}"}}]}),
        Value::Null,
    );
    assert!(codec.push(call.as_bytes()).is_err());
}

#[test]
fn rejects_repeated_identity_suffix_fragments() {
    for field in ["id", "name"] {
        let mut codec = ChatNormalizer::default();
        let first = frame(
            json!({"tool_calls":[{"index":0,"id":"call_","function":{"name":"get_","arguments":"{}"}}]}),
            Value::Null,
        );
        codec.push(first.as_bytes()).unwrap();
        let fragment = if field == "id" {
            json!({"index":0,"id":"suffix"})
        } else {
            json!({"index":0,"function":{"name":"suffix"}})
        };
        codec
            .push(frame(json!({"tool_calls":[fragment.clone()]}), Value::Null).as_bytes())
            .unwrap();
        assert!(
            codec
                .push(frame(json!({"tool_calls":[fragment]}), Value::Null).as_bytes())
                .is_err()
        );
    }
}

#[test]
fn rejects_ambiguous_invalid_and_truncated_calls() {
    for fragment in [
        json!([{"id":"x","function":{"name":"f","arguments":"{}"}}]),
        json!([{"index":64,"id":"x","function":{"name":"f","arguments":"{}"}}]),
        json!([{"index":0,"id":"x","function":{"name":"f","arguments":"{}","extra":1}}]),
    ] {
        let mut codec = ChatNormalizer::default();
        assert!(
            codec
                .push(frame(json!({"tool_calls":fragment}), Value::Null).as_bytes())
                .is_err()
        );
    }
    let mut codec = ChatNormalizer::default();
    codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{"}}]}), Value::Null).as_bytes()).unwrap();
    assert!(
        codec
            .push(frame(json!({}), json!("tool_calls")).as_bytes())
            .is_err()
    );
    let mut codec = ChatNormalizer::default();
    codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{}"}}]}), Value::Null).as_bytes()).unwrap();
    assert!(codec.push(b"data: [DONE]\n\n").is_err());
    let mut codec = ChatNormalizer::default();
    codec.push(b"data: {\"id\":").unwrap();
    assert!(codec.end().is_err());
}

#[test]
fn rejects_duplicate_ids_and_oversized_name() {
    let mut codec = ChatNormalizer::default();
    codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{}"}},{"index":1,"id":"x","function":{"name":"g","arguments":"{}"}}]}), Value::Null).as_bytes()).unwrap();
    assert!(
        codec
            .push(frame(json!({}), json!("tool_calls")).as_bytes())
            .is_err()
    );
    let mut codec = ChatNormalizer::default();
    assert!(codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"x".repeat(129),"arguments":"{}"}}]}), Value::Null).as_bytes()).is_err());
}

#[test]
fn eof_after_finish_and_usage_succeeds_without_done() {
    let mut codec = ChatNormalizer::default();
    let mut events = codec
        .push(frame(json!({"content":"answer"}), json!("stop")).as_bytes())
        .unwrap();
    let usage = json!({"id":"r","model":"m","choices":[],"usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}});
    events.extend(codec.push(format!("data: {usage}\n\n").as_bytes()).unwrap());
    events.extend(codec.end().unwrap());
    assert!(events.iter().any(|event| matches!(event, Output::Usage(_))));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Output::Finish(_)))
    );
}

#[test]
fn rejects_repeated_ambiguous_identity_and_duplicate_done() {
    let mut codec = ChatNormalizer::default();
    codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{}"}}]}), Value::Null).as_bytes()).unwrap();
    assert!(
        codec
            .push(frame(json!({"tool_calls":[{"index":0,"id":"x"}]}), Value::Null).as_bytes())
            .is_err()
    );
    let mut codec = ChatNormalizer::default();
    codec
        .push(frame(json!({}), json!("stop")).as_bytes())
        .unwrap();
    codec.push(b"data: [DONE]\n\n").unwrap();
    assert!(codec.push(b"data: [DONE]\n\n").is_err());
}

#[test]
fn refuses_tool_execution_when_finish_indicates_truncation() {
    let mut codec = ChatNormalizer::default();
    codec.push(frame(json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":"{}"}}]}), Value::Null).as_bytes()).unwrap();
    assert!(
        codec
            .push(frame(json!({}), json!("length")).as_bytes())
            .is_err()
    );
}

#[test]
fn refuses_oversized_fragments_and_complete_events() {
    let mut codec = ChatNormalizer::default();
    let too_big = "x".repeat(1024 * 1024 + 1);
    let input = frame(
        json!({"tool_calls":[{"index":0,"id":"x","function":{"name":"f","arguments":too_big}}]}),
        Value::Null,
    );
    assert!(codec.push(input.as_bytes()).is_err());
    let mut codec = ChatNormalizer::default();
    let event = format!("data: {}\n\n", "x".repeat(2 * 1024 * 1024));
    assert!(codec.push(event.as_bytes()).is_err());
}

#[test]
fn rejects_duplicate_json_keys_and_invalid_utf8() {
    let mut codec = ChatNormalizer::default();
    let duplicate = b"data: {\"id\":\"r\",\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"index\":1,\"id\":\"x\",\"function\":{\"name\":\"f\",\"arguments\":\"{}\"}}]}}]}\n\n";
    assert!(codec.push(duplicate).is_err());
    let mut codec = ChatNormalizer::default();
    assert!(codec.push(b"data: \xff\n\n").is_err());
}

#[test]
fn rejects_malformed_choice_instead_of_dropping_it() {
    for input in [
        b"data: {\"choices\":{}}\n\n".as_slice(),
        b"data: {\"choices\":[{\"delta\":42}]}\n\n".as_slice(),
        b"data: []\n\n".as_slice(),
    ] {
        assert!(ChatNormalizer::default().push(input).is_err());
    }
}
