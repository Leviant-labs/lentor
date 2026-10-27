use super::*;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;

#[test]
fn stored_keys_are_endpoint_bound_and_references_are_immutable() {
    let store = MockKeyringStore::default();
    let first = store_key(&store, "custom", "https://gateway.example/v1/", "first-key").unwrap();
    let second = store_key(&store, "custom", "https://gateway.example/v1", "second-key").unwrap();
    assert_ne!(first, second);
    assert_eq!(
        load_key(&store, &first, "https://gateway.example/v1").unwrap(),
        "first-key"
    );
    assert_eq!(
        load_key(&store, &second, "https://gateway.example/v1/").unwrap(),
        "second-key"
    );
    assert_eq!(
        load_key(&store, &first, "https://other.example/v1"),
        Err(ProviderCredentialError::EndpointMismatch)
    );
    delete_key(&store, &second).unwrap();
    assert_eq!(
        load_key(&store, &second, "https://gateway.example/v1"),
        Err(ProviderCredentialError::MissingKey)
    );
    assert_eq!(
        load_key(&store, &first, "https://gateway.example/v1").unwrap(),
        "first-key"
    );
}

#[test]
fn invalid_endpoint_and_key_are_rejected_before_storage() {
    let store = MockKeyringStore::default();
    for endpoint in [
        "not-a-url",
        "file:///tmp/api",
        "https://user:secret@example.test/v1",
        "https://example.test/v1?key=secret",
        "https://example.test/v1#secret",
        "http://example.test/v1",
        "http://192.168.1.1/v1",
        "http://localhost.example/v1",
    ] {
        assert_eq!(
            store_key(&store, "custom", endpoint, "key"),
            Err(ProviderCredentialError::InvalidEndpoint)
        );
    }
    for key in ["", " \t ", "key\nother"] {
        assert_eq!(
            store_key(&store, "custom", "https://example.test/v1", key),
            Err(ProviderCredentialError::EmptyKey)
        );
    }
    assert_eq!(
        load_key(&store, "invalid", "https://example.test/v1"),
        Err(ProviderCredentialError::InvalidReference)
    );
}

#[test]
fn loopback_http_and_same_origin_catalogs_are_allowed() {
    let store = MockKeyringStore::default();
    for endpoint in [
        "http://localhost:8080/v1",
        "http://127.0.0.2/v1",
        "http://[::1]/v1",
    ] {
        let reference = store_key(&store, "local", endpoint, "key").unwrap();
        assert_eq!(load_key(&store, &reference, endpoint).unwrap(), "key");
    }
    assert_eq!(
        validate_catalog_destination(
            "https://example.test/v1",
            "https://example.test:443/models?version=1"
        ),
        Ok(())
    );
    for catalog in [
        "https://other.test/models",
        "http://example.test/models",
        "https://example.test:444/models",
        "https://user@example.test/models",
    ] {
        assert!(validate_catalog_destination("https://example.test/v1", catalog).is_err());
    }
}
