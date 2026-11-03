use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn native_state_roundtrips_and_redacts_debug() -> anyhow::Result<()> {
    let state = NativeProviderState::new(
        NativeProviderProtocol::Messages,
        "a".repeat(64),
        "item-1".into(),
        json!({"signature": "private"}),
    )
    .map_err(anyhow::Error::msg)?;
    let encoded = serde_json::to_value(&state)?;
    assert_eq!(
        serde_json::from_value::<NativeProviderState>(encoded)?,
        state
    );
    assert!(!format!("{state:?}").contains("private"));
    Ok(())
}

#[test]
fn reject_oversized_or_unscoped_state_on_read() {
    for value in [
        json!({"protocol":"messages", "endpoint_hash":"x", "item_id":"id", "payload":{}}),
        json!({"protocol":"messages", "endpoint_hash":"a".repeat(64), "item_id":"id", "payload":"x".repeat(MAX_STATE_BYTES)}),
        json!({"protocol":"messages", "endpoint_hash":"a".repeat(64), "item_id":"", "payload":{}}),
    ] {
        assert!(serde_json::from_value::<NativeProviderState>(value).is_err());
    }
}

#[test]
fn legacy_metadata_roundtrip_preserves_absent_native_state() -> anyhow::Result<()> {
    let legacy = json!({"client_authored": true, "user_input_order": 7});
    let metadata: crate::CodexHarnessMetadata = serde_json::from_value(legacy.clone())?;
    assert_eq!(
        metadata,
        crate::CodexHarnessMetadata {
            client_authored: true,
            user_input_order: Some(7),
            ..Default::default()
        }
    );
    assert_eq!(serde_json::to_value(metadata)?, legacy);
    Ok(())
}

#[test]
fn native_metadata_roundtrip_preserves_canonical_response_item() -> anyhow::Result<()> {
    let item: codex_protocol::models::ResponseItem = serde_json::from_value(json!({
        "type": "function_call", "id": "item-1", "call_id": "call-1",
        "name": "shell", "arguments": "{}"
    }))?;
    let canonical = serde_json::to_value(&item)?;
    let state = NativeProviderState::new(
        NativeProviderProtocol::Messages,
        "a".repeat(64),
        "item-1".into(),
        json!({"signature": "private"}),
    )
    .map_err(anyhow::Error::msg)?;
    let envelope = crate::ResponseItemEnvelope {
        item,
        metadata: Some(crate::CodexHarnessMetadata {
            native_provider_state: Some(state),
            ..Default::default()
        }),
    };
    let rollout = crate::RolloutItem::ResponseItem(envelope.clone());
    let decoded: crate::RolloutItem = serde_json::from_value(serde_json::to_value(rollout)?)?;
    let crate::RolloutItem::ResponseItem(decoded) = decoded else {
        anyhow::bail!("expected a response-item rollout record");
    };
    assert_eq!(decoded, envelope);
    assert_eq!(serde_json::to_value(&decoded.item)?, canonical);
    Ok(())
}
