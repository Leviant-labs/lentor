#![allow(clippy::expect_used, clippy::unwrap_used)]

use super::*;
use codex_keyring_store::tests::MockKeyringStore;
use pretty_assertions::assert_eq;
use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::net::TcpListener as StdListener;
use std::thread;
use std::time::Duration;
use tokio::io::AsyncWriteExt as _;
use tokio::net::TcpStream;

const TEST_KEY: &str = "super-secret";
const IDENTITY_BODY: &str = r#"{"user":{"id":"u-1","userName":"Ada","keyName":"laptop"}}"#;

struct MockResponse {
    status: &'static str,
    body: Vec<u8>,
}

impl MockResponse {
    fn ok(body: &str) -> Self {
        Self {
            status: "200 OK",
            body: body.as_bytes().to_vec(),
        }
    }
}

fn mock_whoami(responses: Vec<MockResponse>) -> String {
    let listener = StdListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    thread::spawn(move || {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let mut request = Vec::new();
            let mut chunk = [0u8; 1024];
            loop {
                let read = stream.read(&mut chunk).unwrap();
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&request).to_ascii_lowercase();
            assert!(
                request.contains(&format!("authorization: bearer {TEST_KEY}")),
                "missing bearer credential"
            );
            let head = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.status,
                response.body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&response.body);
        }
    });
    format!("http://{address}/alpha/whoami")
}

async fn send(url: &str, method: &str, content_type: Option<&str>, body: &str) -> String {
    let parsed = Url::parse(url).unwrap();
    let address = format!("{}:{}", parsed.host_str().unwrap(), parsed.port().unwrap());
    let mut stream = TcpStream::connect(address).await.unwrap();
    let mut request = format!(
        "{method} {} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {}\r\nConnection: close\r\n",
        parsed.path(),
        body.len()
    );
    if let Some(content_type) = content_type {
        request.push_str(&format!("Content-Type: {content_type}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).await.unwrap();
    response
}

async fn post_callback(url: &str, body: &str) -> String {
    send(url, "POST", Some("application/json"), body).await
}

fn state_from(pending: &PendingLogin) -> String {
    let url = Url::parse(pending.auth_url()).unwrap();
    let params: HashMap<_, _> = url.query_pairs().into_owned().collect();
    params.get("state").unwrap().clone()
}

fn callback_body(state: &str) -> String {
    format!(r#"{{"apiKey":"{TEST_KEY}","state":"{state}"}}"#)
}

fn test_store() -> (MockKeyringStore, CredentialStore<MockKeyringStore>) {
    let backend = MockKeyringStore::default();
    let store = CredentialStore::new(backend.clone());
    (backend, store)
}

fn identity_named(user_name: &str) -> CommandCodeIdentity {
    CommandCodeIdentity {
        user_id: "u-1".into(),
        user_name: user_name.into(),
        key_name: None,
        source: AuthSource::OAuth,
    }
}

#[tokio::test]
async fn validates_and_persists_identity_without_cross_provider_access() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let auth = Authenticator::with_endpoint(endpoint);
    let backend = MockKeyringStore::default();
    let store = CredentialStore::new(backend);
    let identity = auth
        .authenticate_and_store(&store, PROVIDER, ApiKey::new(TEST_KEY), AuthSource::ApiKey)
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
    assert_eq!(
        store.load_identity(PROVIDER).unwrap(),
        Some(identity.clone())
    );
    assert!(store.load_secret("other-provider").unwrap().is_none());
    let metadata = serde_json::to_string(&identity).unwrap();
    assert!(!metadata.contains(TEST_KEY));
}

#[tokio::test]
async fn invalid_key_is_not_persisted_and_diagnostics_redact_it() {
    let endpoint = mock_whoami(vec![MockResponse {
        status: "401 Unauthorized",
        body: br#"{"error":"super-secret"}"#.to_vec(),
    }]);
    let auth = Authenticator::with_endpoint(endpoint);
    let backend = MockKeyringStore::default();
    let store = CredentialStore::new(backend.clone());
    let error = auth
        .authenticate_and_store(&store, PROVIDER, ApiKey::new(TEST_KEY), AuthSource::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::AuthenticationFailed);
    assert!(!format!("{error:?}").contains(TEST_KEY));
    assert!(!error.to_string().contains(TEST_KEY));
    assert!(!format!("{:?}", ApiKey::new(TEST_KEY)).contains(TEST_KEY));
    assert!(store.load_secret(PROVIDER).unwrap().is_none());
    assert!(store.load_identity(PROVIDER).unwrap().is_none());
    assert!(!backend.contains("commandcode:secret"));
}

#[test]
fn failed_metadata_write_restores_the_previous_credential() {
    let (backend, store) = test_store();
    let previous_identity = identity_named("Grace");
    store
        .save(
            PROVIDER,
            &ApiKey::new("previous-secret"),
            &previous_identity,
        )
        .unwrap();
    backend.set_error(
        "commandcode:identity",
        keyring::Error::Invalid("mock".into(), "save".into()),
    );

    let error = store
        .save(PROVIDER, &ApiKey::new(TEST_KEY), &identity_named("Ada"))
        .unwrap_err();
    assert_eq!(error, AuthError::StoreFailed);
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        "previous-secret"
    );
    assert_eq!(
        store.load_identity(PROVIDER).unwrap(),
        Some(previous_identity)
    );
}

#[test]
fn failed_first_time_metadata_write_leaves_no_secret() {
    let (backend, store) = test_store();
    backend.set_error(
        "commandcode:identity",
        keyring::Error::Invalid("mock".into(), "save".into()),
    );

    let error = store
        .save(PROVIDER, &ApiKey::new(TEST_KEY), &identity_named("Ada"))
        .unwrap_err();
    assert_eq!(error, AuthError::StoreFailed);
    assert!(store.load_secret(PROVIDER).unwrap().is_none());
    assert!(!backend.contains("commandcode:secret"));
}

#[test]
fn unreadable_existing_secret_is_not_overwritten() {
    let (backend, store) = test_store();
    store
        .save(
            PROVIDER,
            &ApiKey::new("previous-secret"),
            &identity_named("Grace"),
        )
        .unwrap();
    backend.set_error(
        "commandcode:secret",
        keyring::Error::Invalid("mock".into(), "load".into()),
    );

    let error = store
        .save(PROVIDER, &ApiKey::new(TEST_KEY), &identity_named("Ada"))
        .unwrap_err();
    assert_eq!(error, AuthError::StoreFailed);
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        "previous-secret"
    );
}

#[test]
fn manual_callback_requires_matching_state_and_redacts_key() {
    assert_eq!(
        callback::parse_manual(r#"{"apiKey":"secret","state":"wrong"}"#, "right").unwrap_err(),
        AuthError::StateMismatch
    );
    let key = callback::parse_manual(TEST_KEY, "right").unwrap();
    assert!(!format!("{key:?}").contains(TEST_KEY));
    let url = format!("http://127.0.0.1:5959/callback?apiKey={TEST_KEY}&state=right");
    assert_eq!(
        callback::parse_manual(&url, "wrong").unwrap_err(),
        AuthError::StateMismatch
    );
    assert!(callback::parse_manual(&url, "right").is_ok());
    let blank_url = "http://127.0.0.1:5959/callback?apiKey=%20&state=right";
    assert_eq!(
        callback::parse_manual(blank_url, "right").unwrap_err(),
        AuthError::InvalidCallback
    );
    assert_eq!(
        callback::parse_manual(r#"{"apiKey":"  ","state":"right"}"#, "right").unwrap_err(),
        AuthError::InvalidCallback
    );
}

#[tokio::test]
async fn login_exposes_urls_before_waiting_and_accepts_callback() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let (backend, store) = test_store();
    let pending = Authenticator::with_endpoint(endpoint)
        .begin_login()
        .await
        .unwrap();
    let auth_url = Url::parse(pending.auth_url()).unwrap();
    let params: HashMap<_, _> = auth_url.query_pairs().into_owned().collect();
    assert_eq!(
        pending.auth_url().split('?').next(),
        Some("https://commandcode.ai/studio/auth/cli")
    );
    assert_eq!(
        params.get("callback"),
        Some(&pending.callback_url().to_string())
    );
    let state = state_from(&pending);
    assert_eq!(params.get("state"), Some(&state));

    let response = post_callback(pending.callback_url(), &callback_body(&state)).await;
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");

    let (key, identity) = pending
        .complete(&store, PROVIDER, LoginOptions::default())
        .await
        .unwrap();
    assert_eq!(identity.source, AuthSource::OAuth);
    assert_eq!(identity.user_name, "Ada");
    assert!(!format!("{key:?}").contains(TEST_KEY));
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        TEST_KEY
    );
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
    assert!(
        !backend
            .saved_value("commandcode:identity")
            .unwrap()
            .contains(TEST_KEY)
    );
}

#[tokio::test]
async fn state_mismatch_rejects_callback_and_keeps_waiting() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let (_backend, store) = test_store();
    let pending = Authenticator::with_endpoint(endpoint)
        .begin_login()
        .await
        .unwrap();
    let rejected = post_callback(pending.callback_url(), &callback_body("wrong")).await;
    assert!(rejected.starts_with("HTTP/1.1 403 Forbidden"), "{rejected}");

    let state = state_from(&pending);
    let accepted = post_callback(pending.callback_url(), &callback_body(&state)).await;
    assert!(accepted.starts_with("HTTP/1.1 200 OK"), "{accepted}");

    let (_, identity) = pending
        .complete(&store, PROVIDER, LoginOptions::default())
        .await
        .unwrap();
    assert_eq!(identity.source, AuthSource::OAuth);
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
}

#[tokio::test]
async fn studio_preflight_receives_cors_permission() {
    let pending = Authenticator::with_endpoint(mock_whoami(Vec::new()))
        .begin_login()
        .await
        .unwrap();
    let response = send(pending.callback_url(), "OPTIONS", None, "").await;
    assert!(
        response.starts_with("HTTP/1.1 204 No Content"),
        "{response}"
    );
    let response = response.to_ascii_lowercase();
    assert!(response.contains("access-control-allow-origin: https://commandcode.ai"));
    assert!(response.contains("access-control-allow-methods: post, options"));
    assert!(response.contains("access-control-allow-headers: content-type"));
}

#[tokio::test]
async fn oversized_callback_body_is_rejected() {
    let pending = Authenticator::with_endpoint(mock_whoami(Vec::new()))
        .begin_login()
        .await
        .unwrap();
    let body = "a".repeat(callback::MAX_BODY_BYTES + 1);
    let response = post_callback(pending.callback_url(), &body).await;
    assert!(
        response.starts_with("HTTP/1.1 413 Payload Too Large"),
        "{response}"
    );
}

#[tokio::test]
async fn manual_paste_ignores_bad_state_then_accepts_bare_key() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let (_backend, store) = test_store();
    let pending = Authenticator::with_endpoint(endpoint)
        .begin_login()
        .await
        .unwrap();
    let (manual_tx, manual_rx) = mpsc::channel(4);
    manual_tx.send(callback_body("wrong")).await.unwrap();
    manual_tx.send("   ".to_string()).await.unwrap();
    manual_tx.send(TEST_KEY.to_string()).await.unwrap();
    let (key, identity) = pending
        .complete(
            &store,
            PROVIDER,
            LoginOptions {
                manual: Some(manual_rx),
                abort: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(identity.source, AuthSource::OAuth);
    assert!(!format!("{key:?}").contains(TEST_KEY));
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        TEST_KEY
    );
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
}

#[tokio::test]
async fn login_wrapper_returns_the_presented_authorization_url() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let auth = Authenticator::with_endpoint(endpoint);
    let (_backend, store) = test_store();
    let (manual_tx, manual_rx) = mpsc::channel(1);
    manual_tx.send(TEST_KEY.to_string()).await.unwrap();
    let (key, identity, auth_url) = auth
        .login(
            &store,
            PROVIDER,
            LoginOptions {
                manual: Some(manual_rx),
                abort: None,
            },
        )
        .await
        .unwrap();
    assert!(auth_url.starts_with("https://commandcode.ai/studio/auth/cli?"));
    assert_eq!(identity.source, AuthSource::OAuth);
    assert!(!format!("{key:?}").contains(TEST_KEY));
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        TEST_KEY
    );
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
}

#[tokio::test]
async fn login_times_out_and_releases_the_callback_listener() {
    let (_backend, store) = test_store();
    let pending = Authenticator::with_endpoint(mock_whoami(Vec::new()))
        .with_login_timeout(Duration::from_millis(25))
        .begin_login()
        .await
        .unwrap();
    let callback_url = pending.callback_url().to_string();
    let parsed = Url::parse(&callback_url).unwrap();
    let address = format!("{}:{}", parsed.host_str().unwrap(), parsed.port().unwrap());
    assert!(
        tokio::net::TcpListener::bind(address.as_str())
            .await
            .is_err(),
        "callback port was free while the login was pending"
    );
    let error = pending
        .complete(&store, PROVIDER, LoginOptions::default())
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::TimedOut);
    assert!(store.load_secret(PROVIDER).unwrap().is_none());

    let mut released = false;
    for _ in 0..100 {
        if tokio::net::TcpListener::bind(address.as_str())
            .await
            .is_ok()
        {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        released,
        "callback listener still holds its port after timeout"
    );
}

#[tokio::test]
async fn login_can_be_aborted() {
    let (_backend, store) = test_store();
    let pending = Authenticator::with_endpoint(mock_whoami(Vec::new()))
        .begin_login()
        .await
        .unwrap();
    let (abort_tx, abort_rx) = oneshot::channel();
    abort_tx.send(()).unwrap();
    let error = pending
        .complete(
            &store,
            PROVIDER,
            LoginOptions {
                manual: None,
                abort: Some(abort_rx),
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::Cancelled);
    assert!(store.load_secret(PROVIDER).unwrap().is_none());
}

#[tokio::test]
async fn oversized_whoami_body_is_rejected() {
    let padding = "a".repeat(MAX_WHOAMI_BYTES);
    let body = format!(r#"{{"user":{{"id":"u-1","userName":"Ada"}},"padding":"{padding}"}}"#);
    let endpoint = mock_whoami(vec![MockResponse::ok(&body)]);
    let error = Authenticator::with_endpoint(endpoint)
        .validate_api_key(&ApiKey::new(TEST_KEY), AuthSource::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::AuthenticationFailed);
}

#[tokio::test]
async fn malformed_whoami_body_is_rejected() {
    let endpoint = mock_whoami(vec![MockResponse::ok("not-json")]);
    let error = Authenticator::with_endpoint(endpoint)
        .validate_api_key(&ApiKey::new(TEST_KEY), AuthSource::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::AuthenticationFailed);
}

#[tokio::test]
async fn whoami_without_display_name_is_rejected() {
    let endpoint = mock_whoami(vec![MockResponse::ok(r#"{"user":{"id":"u-1"}}"#)]);
    let error = Authenticator::with_endpoint(endpoint)
        .validate_api_key(&ApiKey::new(TEST_KEY), AuthSource::ApiKey)
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::AuthenticationFailed);
}

#[tokio::test]
async fn import_local_validates_stores_and_keeps_the_source_file() {
    let endpoint = mock_whoami(vec![MockResponse::ok(IDENTITY_BODY)]);
    let auth = Authenticator::with_endpoint(endpoint);
    let backend = MockKeyringStore::default();
    let store = CredentialStore::new(backend.clone());
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("auth.json");
    let contents = format!(r#"{{"apiKey":"{TEST_KEY}","userId":"local-user"}}"#);
    std::fs::write(&path, &contents).unwrap();

    let identity = auth
        .import_local(&store, Some(path.clone()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(identity.source, AuthSource::ImportedCli);
    assert_eq!(store.load_identity(PROVIDER).unwrap(), Some(identity));
    assert_eq!(
        store.load_secret(PROVIDER).unwrap().unwrap().expose(),
        TEST_KEY
    );
    assert!(store.load_secret("other-provider").unwrap().is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), contents);

    let metadata = backend.saved_value("commandcode:identity").unwrap();
    assert!(!metadata.contains(TEST_KEY));
    assert!(!metadata.contains("local-user"));
}

#[tokio::test]
async fn import_local_rejects_missing_file_without_storing() {
    let store = CredentialStore::new(MockKeyringStore::default());
    let directory = tempfile::tempdir().unwrap();
    let missing = directory.path().join("missing.json");
    let error = Authenticator::with_endpoint("http://127.0.0.1:1/alpha/whoami")
        .import_local(&store, Some(missing))
        .await
        .unwrap_err();
    assert_eq!(error, AuthError::ImportFailed);
    assert!(store.load_secret(PROVIDER).unwrap().is_none());
}

#[test]
fn environment_key_reference_must_name_a_populated_variable() {
    assert_eq!(
        api_key_from_env("CODEX_PROVIDER_AUTH_TEST_VARIABLE_UNSET").unwrap_err(),
        AuthError::MissingEnvironmentKey
    );
    assert_eq!(
        api_key_from_env("   ").unwrap_err(),
        AuthError::MissingEnvironmentKey
    );
}
