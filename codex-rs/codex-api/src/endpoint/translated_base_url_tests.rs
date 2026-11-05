use codex_client::normalize_translated_base_url;
use pretty_assertions::assert_eq;

#[test]
fn normalizes_empty_paths_and_selected_endpoint_suffixes() {
    for (input, suffix, expected) in [
        ("https://example.com", "/messages", "https://example.com/v1"),
        (
            "https://example.com/",
            "/messages",
            "https://example.com/v1",
        ),
        (
            "https://example.com/v1/",
            "/messages",
            "https://example.com/v1",
        ),
        (
            "https://example.com/custom/v1/chat/completions/",
            "/chat/completions",
            "https://example.com/custom/v1",
        ),
        (
            "https://example.com/custom/messages",
            "/chat/completions",
            "https://example.com/custom/messages",
        ),
    ] {
        assert_eq!(
            normalize_translated_base_url(input, suffix),
            Ok(expected.to_owned())
        );
    }
}

#[test]
fn rejects_urls_with_query_fragment_or_userinfo() {
    for input in [
        "https://example.com/v1?key=value",
        "https://example.com/v1#fragment",
        "https://user@example.com/v1",
        "https://user:password@example.com/v1",
        "not a url",
    ] {
        assert!(normalize_translated_base_url(input, "/messages").is_err());
    }
}
