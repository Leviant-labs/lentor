use super::*;
use crate::auth::AuthProvider;
use crate::provider::RetryConfig;
use codex_client::Request;
use codex_client::Response;
use codex_client::TransportError;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::Mutex;
use std::time::Duration;

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Request>>>);

impl HttpTransport for Capture {
    async fn execute(&self, _request: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build(
            "unexpected non-stream request".to_string(),
        ))
    }

    async fn stream(&self, request: Request) -> Result<StreamResponse, TransportError> {
        self.0
            .lock()
            .map_err(|_| TransportError::Build("request capture lock poisoned".into()))?
            .push(request);
        Err(TransportError::Build("stream failed".to_string()))
    }
}

struct TestAuth;

impl AuthProvider for TestAuth {
    fn add_auth_headers(&self, headers: &mut HeaderMap) -> Result<(), crate::AuthError> {
        headers.insert("x-api-key", HeaderValue::from_static("test-key"));
        Ok(())
    }
}

struct InvalidAuth;

impl AuthProvider for InvalidAuth {
    fn add_auth_headers(&self, _headers: &mut HeaderMap) -> Result<(), crate::AuthError> {
        Err(crate::AuthError::Build("invalid auth header".to_string()))
    }
}

#[derive(Clone)]
struct RawStream;

impl HttpTransport for RawStream {
    async fn execute(&self, _request: Request) -> Result<Response, TransportError> {
        Err(TransportError::Build(
            "unexpected non-stream request".to_string(),
        ))
    }

    async fn stream(&self, _request: Request) -> Result<StreamResponse, TransportError> {
        Ok(StreamResponse {
            status: http::StatusCode::OK,
            headers: HeaderMap::new(),
            bytes: Box::pin(futures::stream::iter(vec![
                Ok(bytes::Bytes::from_static(b"data: first\n\n")),
                Err(TransportError::Build("upstream interrupted".to_string())),
            ])),
        })
    }
}

#[tokio::test]
async fn translated_stream_exposes_raw_chunks_and_stream_errors() -> anyhow::Result<()> {
    use futures::StreamExt;

    let client = TranslatedStreamClient::new(
        RawStream,
        Provider {
            name: "test".into(),
            base_url: "https://example.com/v1".into(),
            query_params: None,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(1),
        },
        Arc::new(TestAuth),
    );
    let mut response = client
        .stream(TranslatedApi::Chat, &json!({}), HeaderMap::new())
        .await?;
    assert_eq!(
        response
            .bytes
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("missing first stream chunk"))??,
        bytes::Bytes::from_static(b"data: first\n\n")
    );
    assert!(
        response
            .bytes
            .next()
            .await
            .ok_or_else(|| anyhow::anyhow!("missing stream failure"))?
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn translated_stream_does_not_send_on_auth_failure() -> anyhow::Result<()> {
    let transport = Capture::default();
    let client = TranslatedStreamClient::new(
        transport.clone(),
        Provider {
            name: "test".into(),
            base_url: "https://example.com/v1".into(),
            query_params: None,
            headers: HeaderMap::new(),
            retry: RetryConfig {
                max_attempts: 1,
                base_delay: Duration::from_millis(1),
                retry_429: false,
                retry_5xx: false,
                retry_transport: false,
            },
            stream_idle_timeout: Duration::from_secs(1),
        },
        Arc::new(InvalidAuth),
    );
    assert!(
        client
            .stream(TranslatedApi::Messages, &json!({}), HeaderMap::new())
            .await
            .is_err()
    );
    assert!(
        transport
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("request capture lock poisoned"))?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn translated_stream_uses_scoped_paths_and_auth_and_propagates_failure() -> anyhow::Result<()>
{
    for (api, base_url, expected_url) in [
        (
            TranslatedApi::Chat,
            "https://example.com",
            "https://example.com/v1/chat/completions",
        ),
        (
            TranslatedApi::Chat,
            "https://example.com/v1/chat/completions",
            "https://example.com/v1/chat/completions",
        ),
        (
            TranslatedApi::Messages,
            "https://example.com/v1/messages/",
            "https://example.com/v1/messages",
        ),
        (
            TranslatedApi::Messages,
            "https://example.com/custom/api/",
            "https://example.com/custom/api/messages",
        ),
    ] {
        let transport = Capture::default();
        let client = TranslatedStreamClient::new(
            transport.clone(),
            Provider {
                name: "test".into(),
                base_url: base_url.into(),
                query_params: None,
                headers: HeaderMap::new(),
                retry: RetryConfig {
                    max_attempts: 1,
                    base_delay: Duration::from_millis(1),
                    retry_429: false,
                    retry_5xx: false,
                    retry_transport: false,
                },
                stream_idle_timeout: Duration::from_secs(1),
            },
            Arc::new(TestAuth),
        );
        assert!(
            client
                .stream(api, &json!({"stream": true}), HeaderMap::new())
                .await
                .is_err()
        );
        let requests = transport
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("request capture lock poisoned"))?;
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].url, expected_url);
        assert_eq!(
            requests[0].headers.get("x-api-key"),
            Some(&HeaderValue::from_static("test-key"))
        );
        assert_eq!(
            requests[0].headers.get(http::header::ACCEPT),
            Some(&HeaderValue::from_static("text/event-stream"))
        );
    }
    Ok(())
}
