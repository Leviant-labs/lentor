use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

fn envelope(id: &str, state: NativeProviderState) -> ResponseItemEnvelope {
    let mut item = ResponseItem::Reasoning {
        id: None,
        summary: Vec::new(),
        content: None,
        encrypted_content: None,
        internal_chat_message_metadata_passthrough: None,
    };
    item.set_id(Some(codex_protocol::ResponseItemId::from_server(
        id.to_string(),
    )));
    let mut envelope = ResponseItemEnvelope::new(item);
    envelope
        .metadata
        .get_or_insert_default()
        .native_provider_state = Some(state);
    envelope
}

#[test]
fn native_state_hydrates_retained_items_and_preserves_full_blocks() {
    let scope = scope("provider", "https://EXAMPLE.com/v1/", WireApi::Messages).unwrap();
    assert_eq!(scope, scope_for_test());
    let payload =
        json!({"type":"thinking", "thinking":"text", "signature":"sig", "nested":{"extra":null}});
    let state =
        NativeProviderState::new(scope.0, scope.1.clone(), "item-1".into(), payload).unwrap();
    let saved = envelope("item-1", state.clone());
    let public_item = serde_json::to_string(&saved.item).unwrap();
    assert!(!public_item.contains("sig"));
    assert!(!public_item.contains("thinking"));
    let encoded = serde_json::to_vec(&codex_history::RolloutItem::ResponseItem(saved)).unwrap();
    let codex_history::RolloutItem::ResponseItem(restored) =
        serde_json::from_slice(&encoded).unwrap()
    else {
        panic!("expected response item");
    };
    let mut store = NativeStateStore::default();
    store.hydrate(scope.clone(), &[restored]).unwrap();
    assert_eq!(store.records.get("item-1"), Some(&state));
    store.hydrate(scope, &[]).unwrap();
    assert!(store.records.is_empty());
}

fn scope_for_test() -> (NativeProviderProtocol, String) {
    scope("provider", "https://example.com/v1", WireApi::Messages).unwrap()
}

#[test]
fn native_scope_distinguishes_provider_endpoint_and_protocol() {
    let baseline = scope_for_test();
    assert_eq!(
        baseline,
        scope(
            "provider",
            "https://example.com/v1/messages",
            WireApi::Messages
        )
        .unwrap()
    );
    assert_ne!(
        baseline,
        scope("other", "https://example.com/v1", WireApi::Messages).unwrap()
    );
    assert_ne!(
        baseline,
        scope("provider", "https://example.com/v2", WireApi::Messages).unwrap()
    );
    assert_ne!(
        baseline,
        scope("provider", "https://example.com/v1", WireApi::Chat).unwrap()
    );
    assert!(
        scope(
            "provider",
            "https://user:password@example.com/v1",
            WireApi::Messages
        )
        .is_err()
    );
}

#[test]
fn native_state_rejects_mismatched_scope_item_and_duplicates() {
    let scope = scope_for_test();
    let state = NativeProviderState::new(
        scope.0,
        scope.1.clone(),
        "item-1".into(),
        json!({"signature":"sig"}),
    )
    .unwrap();
    let mut store = NativeStateStore::default();
    assert!(
        store
            .hydrate(scope.clone(), &[envelope("other", state.clone())])
            .is_err()
    );
    let other_scope = super::scope("other", "https://example.com/v1", WireApi::Messages).unwrap();
    assert!(
        store
            .hydrate(other_scope, &[envelope("item-1", state.clone())])
            .is_err()
    );
    assert!(
        store
            .hydrate(
                scope.clone(),
                &[
                    envelope("item-1", state.clone()),
                    envelope("item-1", state.clone())
                ]
            )
            .is_err()
    );
    store.hydrate(scope, &[]).unwrap();
    store.insert(state.clone()).unwrap();
    assert!(store.insert(state).is_err());
}

#[test]
fn native_state_aggregate_limit_fails_without_replacing_existing_state() {
    let scope = scope_for_test();
    let mut store = NativeStateStore::default();
    store.hydrate(scope.clone(), &[]).unwrap();
    for index in 0..8 {
        let state = NativeProviderState::new(
            scope.0,
            scope.1.clone(),
            format!("item-{index}"),
            json!("x".repeat(32_000)),
        )
        .unwrap();
        store.insert(state).unwrap();
    }
    let state = NativeProviderState::new(
        scope.0,
        scope.1,
        "item-over".into(),
        json!("x".repeat(32_000)),
    )
    .unwrap();
    assert!(store.insert(state).is_err());
    assert_eq!(store.records.len(), 8);
}

#[test]
fn native_state_batch_failure_does_not_insert_earlier_records() {
    let scope = scope_for_test();
    let state = NativeProviderState::new(
        scope.0,
        scope.1.clone(),
        "item-1".into(),
        json!({"signature":"sig"}),
    )
    .unwrap();
    let mut store = NativeStateStore::default();
    store.hydrate(scope, &[]).unwrap();
    assert!(store.insert_batch(vec![state.clone(), state]).is_err());
    assert!(store.records.is_empty());
}
