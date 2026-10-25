use std::io::Read;
use std::io::Write;
use std::net::TcpListener;
use std::thread;
use std::time::Duration;

use codex_protocol::provider::CommandCodeRoute;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

use crate::catalog::CatalogError;
use crate::catalog::CatalogState;
use crate::catalog::MAX_MODELS;
use crate::discovery::CommandCodeDiscovery;
use crate::discovery::DiscoveryError;
use crate::discovery::MAX_MODEL_CATALOG_BYTES;

const COMPLETE_CATALOG: &str = r#"{
  "models": [
    {"id": "complete", "supported_endpoints": ["responses", "chat/completions"]},
    {"id": "other", "supported_endpoints": ["messages"]}
  ]
}"#;

const SPARSE_CATALOG: &str = r#"{"data":[{"id":"sparse-only"}]}"#;

fn discovery_for(server: &MockServer) -> CommandCodeDiscovery {
    CommandCodeDiscovery::with_endpoint(format!("{}/models", server.uri()))
}

async fn mount_catalog(server: &MockServer, body: &str) {
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_string(body))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn complete_catalog_is_fetched_from_the_models_endpoint() {
    let server = MockServer::start().await;
    mount_catalog(&server, COMPLETE_CATALOG).await;

    let catalog = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect("catalog should load");

    assert_eq!(
        (
            catalog.len(),
            catalog.state(),
            catalog
                .model("complete")
                .expect("model should be present")
                .select_route(),
        ),
        (
            2,
            CatalogState::Loaded { models: 2 },
            Ok(CommandCodeRoute::Responses)
        )
    );
    let requests = server
        .received_requests()
        .await
        .expect("requests should be recorded");
    assert_eq!(
        requests
            .iter()
            .map(|request| request.url.path())
            .collect::<Vec<_>>(),
        vec!["/models"]
    );
}

#[tokio::test]
async fn credential_is_sent_as_a_bearer_token() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .and(header("authorization", "Bearer super-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_string(COMPLETE_CATALOG))
        .expect(1)
        .mount(&server)
        .await;

    let catalog = discovery_for(&server)
        .discover(Some(" super-secret "))
        .await
        .expect("catalog should load");

    assert_eq!(catalog.state(), CatalogState::Loaded { models: 2 });
}

#[tokio::test]
async fn blank_credential_is_omitted() {
    let server = MockServer::start().await;
    mount_catalog(&server, SPARSE_CATALOG).await;

    discovery_for(&server)
        .discover(Some("   "))
        .await
        .expect("catalog should load");

    let requests = server
        .received_requests()
        .await
        .expect("requests should be recorded");
    assert!(requests[0].headers.get("authorization").is_none());
}

#[tokio::test]
async fn sparse_catalog_is_accepted() {
    let server = MockServer::start().await;
    mount_catalog(&server, SPARSE_CATALOG).await;

    let catalog = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect("catalog should load");

    assert_eq!(
        catalog
            .model("sparse-only")
            .expect("model should be present")
            .routes(),
        &[]
    );
}

#[tokio::test]
async fn declared_response_size_over_the_limit_is_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(
            ResponseTemplate::new(200).set_body_bytes(vec![b'x'; MAX_MODEL_CATALOG_BYTES + 1]),
        )
        .mount(&server)
        .await;

    let error = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect_err("oversized catalog should fail");

    assert_eq!(
        error,
        DiscoveryError::ResponseTooLarge {
            limit: MAX_MODEL_CATALOG_BYTES,
        }
    );
}

#[tokio::test]
async fn streamed_response_over_the_limit_is_rejected() {
    let endpoint = serve_chunked_once(vec![b'x'; MAX_MODEL_CATALOG_BYTES + 1]);

    let error = CommandCodeDiscovery::with_endpoint(endpoint)
        .discover(/*credential*/ None)
        .await
        .expect_err("oversized streamed catalog should fail");

    assert_eq!(
        error,
        DiscoveryError::ResponseTooLarge {
            limit: MAX_MODEL_CATALOG_BYTES,
        }
    );
}

#[tokio::test]
async fn malformed_catalog_is_rejected_without_echoing_the_body() {
    let server = MockServer::start().await;
    mount_catalog(&server, r#"{"models": [}"#).await;

    let error = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect_err("malformed catalog should fail");

    assert!(matches!(
        error,
        DiscoveryError::InvalidCatalog(CatalogError::InvalidJson(_))
    ));
    let displayed = error.to_string();
    assert!(
        displayed.contains("model catalog response was not valid JSON"),
        "{displayed}"
    );
    assert!(!displayed.contains('{'), "{displayed}");
}

#[tokio::test]
async fn unavailable_catalog_reports_the_status_without_a_body() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(503).set_body_string("upstream secret detail"))
        .mount(&server)
        .await;

    let error = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect_err("unavailable catalog should fail");

    assert_eq!(error, DiscoveryError::HttpStatus(503));
    assert!(!format!("{error}").contains("secret"));
    assert!(!format!("{error:?}").contains("secret"));
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "/elsewhere"))
        .mount(&server)
        .await;

    let error = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect_err("redirected catalog should fail");

    assert_eq!(error, DiscoveryError::HttpStatus(302));
    let requests = server
        .received_requests()
        .await
        .expect("requests should be recorded");
    assert_eq!(requests.len(), 1);
}

#[tokio::test]
async fn slow_catalog_response_times_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(COMPLETE_CATALOG)
                .set_delay(Duration::from_millis(500)),
        )
        .mount(&server)
        .await;

    let error = discovery_for(&server)
        .with_timeout(Duration::from_millis(50))
        .discover(/*credential*/ None)
        .await
        .expect_err("slow catalog should fail");

    assert_eq!(error, DiscoveryError::TimedOut);
}

#[tokio::test]
async fn model_count_is_capped() {
    let server = MockServer::start().await;
    let models = (0..=MAX_MODELS)
        .map(|index| json!({"id": format!("model-{index}")}))
        .collect::<Vec<_>>();
    let body = serde_json::to_string(&json!({ "models": models })).expect("fixture should encode");
    mount_catalog(&server, &body).await;

    let error = discovery_for(&server)
        .discover(/*credential*/ None)
        .await
        .expect_err("too many models should fail");

    assert_eq!(
        error,
        DiscoveryError::InvalidCatalog(CatalogError::TooManyModels {
            count: MAX_MODELS + 1,
            limit: MAX_MODELS,
        })
    );
}

/// Serves one chunked HTTP response so the client cannot see a declared length.
fn serve_chunked_once(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener should have a local address");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should arrive");
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request);
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
            )
            .expect("response headers should write");
        for chunk in body.chunks(4096) {
            write!(stream, "{:x}\r\n", chunk.len()).expect("chunk size should write");
            stream.write_all(chunk).expect("chunk should write");
            stream
                .write_all(b"\r\n")
                .expect("chunk terminator should write");
        }
        stream
            .write_all(b"0\r\n\r\n")
            .expect("final chunk should write");
        stream.flush().expect("response should flush");
    });
    format!("http://{address}/models")
}
