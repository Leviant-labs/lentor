use super::*;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;
use std::io::{Read, Write};
use std::net::TcpListener as StdListener;
use std::thread;

fn mock_response(status: &str, body: &'static str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

fn server(status: &'static str, body: &'static str) -> String {
    let listener = StdListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 4096];
        let size = stream.read(&mut request).unwrap();
        let request = String::from_utf8_lossy(&request[..size]);
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer super-secret")
        );
        stream
            .write_all(mock_response(status, body).as_bytes())
            .unwrap();
    });
    format!("http://{address}/alpha/whoami")
}

#[tokio::test]
async fn validates_and_persists_identity_without_cross_provider_access() {
    let endpoint = server(
        "200 OK",
        r#"{"user":{"id":"u-1","userName":"Ada","keyName":"laptop"}}"#,
    );
    let auth = Authenticator::new(endpoint);
    let backend = MockKeyringStore::default();
    let store = CredentialStore::new(backend);
    let identity = auth
        .authenticate_and_store(
            &store,
            PROVIDER,
            ApiKey::new("super-secret"),
            AuthSource::ApiKey,
        )
        .await
        .unwrap();
    assert_eq!(
        identity,
        CommandCodeIdentity {
            user_id: "u-1".into(),
            user_name: "Ada".into(),
            key_name: Some("laptop".into()),
            source: AuthSource::ApiKey
        }
    );
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
    assert!(store.load_secret("other-provider").unwrap().is_none());
}

#[tokio::test]
async fn invalid_key_error_and_debug_redact_secret() {
    let endpoint = server("401 Unauthorized", r#"{"error":"super-secret"}"#);
    let error = Authenticator::new(endpoint)
        .validate_api_key(&ApiKey::new("super-secret"), AuthSource::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::AuthenticationFailed);
    assert!(!format!("{error:?}").contains("super-secret"));
    assert!(!format!("{:?}", ApiKey::new("super-secret")).contains("super-secret"));
}

#[test]
fn manual_callback_requires_matching_state_and_redacts_key() {
    assert_eq!(
        parse_manual(r#"{"apiKey":"secret","state":"wrong"}"#, "right").unwrap_err(),
        AuthError::StateMismatch
    );
    let key = parse_manual("secret", "right").unwrap();
    assert!(!format!("{key:?}").contains("secret"));
}
