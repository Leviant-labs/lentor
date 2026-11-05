use codex_provider_codecs::AnthropicNativeState;
use pretty_assertions::assert_eq;
use respan_gateway_conversation_http::Schema;
use respan_gateway_conversation_http::anthropic::AnthropicMessages;
use respan_gateway_http::Decoder;
use respan_gateway_http::bytes::Bytes;
use serde_json::json;

#[test]
fn signed_and_redacted_thinking_round_trip_through_public_decoder() {
    let frames = [
        json!({"type":"message_start","message":{"id":"m","model":"claude","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":"","extension":{"version":1}}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"reason"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig-"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"part"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"opaque"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ];
    let mut decoder = AnthropicMessages.decoder();
    let mut state = AnthropicNativeState::default();
    for frame in frames {
        let mut events = Vec::new();
        decoder
            .decode(
                Bytes::from(respan_gateway_sse::event(None, frame)),
                &mut events,
            )
            .unwrap();
        for event in &events {
            state.push(event).unwrap();
        }
    }
    decoder.end(Ok(()), &mut Vec::new()).unwrap();
    let blocks = state.finish().unwrap();
    assert_eq!(
        blocks,
        [
            json!({"type":"thinking","thinking":"reason","signature":"sig-part","extension":{"version":1}}),
            json!({"type":"redacted_thinking","data":"opaque"})
        ]
    );
    let request = json!({"model":"claude","max_tokens":4096,"messages":[{"role":"assistant","content":blocks},{"role":"user","content":"continue"}]});
    let input = AnthropicMessages.decode_json(&request);
    let encoded = AnthropicMessages.encode_request(&input).unwrap();
    assert_eq!(
        encoded["messages"][0]["content"],
        request["messages"][0]["content"]
    );
}

#[test]
fn serialized_native_state_must_fit_history_limit() {
    let mut state = AnthropicNativeState::default();
    state
        .push(&respan_gateway_conversation::Output::block_start(
            0,
            respan_gateway_conversation::Block::Reasoning,
        ))
        .unwrap();
    state
        .push(&respan_gateway_conversation::Output::reasoning(
            0,
            "x".repeat(32 * 1024),
        ))
        .unwrap();
    state
        .push(&respan_gateway_conversation::Output::block_fields(
            "anthropic-messages",
            0,
            serde_json::Map::from_iter([("signature".into(), json!("sig"))]),
        ))
        .unwrap();
    assert!(state.finish().is_err());
}

#[test]
fn rejected_event_poisoned_native_state_cannot_be_finalized() {
    use respan_gateway_conversation::Block;
    use respan_gateway_conversation::Output;
    let mut state = AnthropicNativeState::default();
    state
        .push(&Output::block_start(0, Block::Reasoning))
        .unwrap();
    state
        .push(&Output::block_fields(
            "anthropic-messages",
            0,
            serde_json::Map::from_iter([("signature".into(), json!("sig"))]),
        ))
        .unwrap();
    assert!(
        state
            .push(&Output::block_fields(
                "anthropic-messages",
                0,
                serde_json::Map::from_iter([("signature".into(), json!(null))])
            ))
            .is_err()
    );
    assert!(state.finish().is_err());
}

#[test]
fn native_fields_without_a_reasoning_block_cannot_be_finalized() {
    use respan_gateway_conversation::Output;
    let mut state = AnthropicNativeState::default();
    state
        .push(&Output::block_fields(
            "anthropic-messages",
            0,
            serde_json::Map::from_iter([("extension".into(), json!({"keep":null}))]),
        ))
        .unwrap();
    assert!(state.finish().is_err());
}
