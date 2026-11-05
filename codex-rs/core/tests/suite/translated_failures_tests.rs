//! Translated provider HTTP failures through the real agent loop.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_model_provider_info::WireApi;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const MODEL: &str = "gpt-5.5";
const TOKEN: &str = "fixture-translated-secret";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_http_401_rejection_does_not_fall_back() -> Result<()> {
    assert_translated_http_failure(/*status*/ 401).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_http_429_rate_limit_does_not_fall_back() -> Result<()> {
    assert_translated_http_failure(/*status*/ 429).await
}

async fn assert_translated_http_failure(status: u16) -> Result<()> {
    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let server = start_mock_server().await;
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string(format!("provider rejected request: {TOKEN}")),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let test = test_codex()
            .with_model(MODEL)
            .with_config(move |config| {
                config.model_provider_id = "translated-failure-test".into();
                config.model_provider.name = "Translated failure test".into();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.model_provider.experimental_bearer_token = Some(TOKEN.into());
                config.model_provider.request_max_retries = Some(0);
                config.model_provider.stream_max_retries = Some(0);
            })
            .build_with_auto_env(&server)
            .await?;
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }]))
            .await?;
        let errors = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            let mut errors = Vec::new();
            loop {
                match test.codex.next_event().await?.msg {
                    EventMsg::Error(error) => errors.push(error.message),
                    EventMsg::TurnComplete(_) => return Ok::<_, anyhow::Error>(errors),
                    _ => {}
                }
            }
        })
        .await??;
        assert!(!errors.is_empty(), "expected an HTTP {status} error");
        assert!(
            errors.iter().all(|error| !error.contains(TOKEN)),
            "provider error exposed the key"
        );
        let requests = server
            .received_requests()
            .await
            .expect("provider mock must retain captured requests");
        let calls: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST" && request.url.path() == endpoint)
            .collect();
        assert_eq!(calls.len(), 1);
        let (header, expected) = match protocol {
            WireApi::Chat => ("authorization", format!("Bearer {TOKEN}")),
            WireApi::Messages => ("x-api-key", TOKEN.to_string()),
            WireApi::Responses => unreachable!("translated protocols only"),
        };
        assert_eq!(
            calls[0]
                .headers
                .get(header)
                .and_then(|value| value.to_str().ok()),
            Some(expected.as_str())
        );
        let body: serde_json::Value = serde_json::from_slice(&calls[0].body)?;
        assert_eq!(body["model"], MODEL);
        assert!(
            requests
                .iter()
                .all(|request| request.url.path() != "/v1/responses")
        );
        test.codex.shutdown_and_wait().await?;
    }
    Ok(())
}
