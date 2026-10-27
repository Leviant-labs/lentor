use super::ModelProviderConfigureParams;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn configure_provider_preserves_wire_input_and_redacts_debug() {
    let input = json!({
        "providerId": "custom-api",
        "name": "Custom API",
        "baseUrl": "https://gateway.example/v1",
        "apiKey": "provider-secret-for-wire-test",
        "model": null,
    });
    let params: ModelProviderConfigureParams = serde_json::from_value(input.clone()).unwrap();
    assert_eq!(serde_json::to_value(&params).unwrap(), input);
    assert!(!format!("{params:?}").contains("provider-secret-for-wire-test"));
}
