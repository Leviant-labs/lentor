use super::*;
use pretty_assertions::assert_eq;

#[test]
fn translated_endpoints_share_the_expected_api_base() {
    for (input, protocol, expected) in [
        (
            "https://example.com",
            WireApi::Messages,
            "https://example.com/v1",
        ),
        (
            "https://example.com/v1/",
            WireApi::Messages,
            "https://example.com/v1",
        ),
        (
            "https://example.com/v1/messages",
            WireApi::Messages,
            "https://example.com/v1",
        ),
        (
            "https://example.com/v1/chat/completions",
            WireApi::Chat,
            "https://example.com/v1",
        ),
        (
            "https://example.com/custom/api/messages/",
            WireApi::Messages,
            "https://example.com/custom/api",
        ),
        (
            "https://example.com/custom/api",
            WireApi::Chat,
            "https://example.com/custom/api",
        ),
        (
            "https://example.com/messages",
            WireApi::Messages,
            "https://example.com/v1",
        ),
    ] {
        assert_eq!(
            normalize_translated_base_url(input, protocol).unwrap(),
            expected
        );
    }
    assert!(
        normalize_translated_base_url("https://example.com/v1?key=secret", WireApi::Messages)
            .is_err()
    );
}

#[test]
fn api_provider_normalizes_translated_config_but_keeps_responses_unchanged() {
    for (protocol, expected) in [
        (WireApi::Messages, "https://example.com/v1"),
        (WireApi::Responses, "https://example.com/v1/messages"),
    ] {
        let provider = crate::ModelProviderInfo {
            base_url: Some("https://example.com/v1/messages".to_string()),
            wire_api: protocol,
            ..Default::default()
        };
        assert_eq!(
            provider
                .to_api_provider(/*auth_mode*/ None)
                .unwrap()
                .base_url,
            expected
        );
    }
}
