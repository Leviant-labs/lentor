use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_protocol::openai_models::ReasoningEffort;
use codex_protocol::openai_models::ReasoningEffortPreset;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;
use tracing_test::traced_test;

const MODEL: &str = "vendor/model-exact-v1";
const TOKEN: &str = "fixture-endpoint-secret";
const MISSING_ENV_KEY: &str = "CLODEX_ENDPOINT_PROVIDER_MISSING_KEY_20261026";

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_provider_streams_and_continues_after_local_tool() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let requests = responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("resp-1"),
                responses::ev_function_call(
                    "plan-call",
                    "update_plan",
                    &json!({"plan": [{"step": "Inspect", "status": "in_progress"}]}).to_string(),
                ),
                responses::ev_completed("resp-1"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-2"),
                responses::ev_message_item_added("message-1", ""),
                responses::ev_output_text_delta("Plan "),
                responses::ev_output_text_delta("updated"),
                responses::ev_assistant_message("message-1", "Plan updated"),
                responses::ev_completed("resp-2"),
            ]),
            responses::sse(vec![
                responses::ev_response_created("resp-3"),
                responses::ev_assistant_message("message-2", "Continued"),
                responses::ev_completed("resp-3"),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_model_info_override(MODEL, |model| {
            model.supported_reasoning_levels = vec![ReasoningEffortPreset {
                effort: ReasoningEffort::High,
                description: "High".to_owned(),
            }];
        })
        .with_config(move |config| {
            config.model_provider_id = "custom-endpoint".to_owned();
            config.model_provider.name = "Custom endpoint".to_owned();
            config.model_provider.base_url = Some(base_url.clone());
            config.model_provider.requires_openai_auth = false;
            config.model_provider.experimental_bearer_token = Some(TOKEN.into());
            config.model_reasoning_effort = Some(ReasoningEffort::High);
            config.update_plan_enabled = true;
            config.features.disable(Feature::CodeModeOnly).unwrap();
            config.features.disable(Feature::CodeMode).unwrap();
        })
        .build_with_auto_env(&server)
        .await?;

    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "Update the plan".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let mut streamed = String::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::AgentMessageContentDelta(delta) = event {
            streamed.push_str(&delta.delta);
        }
        matches!(event, EventMsg::TurnComplete(_))
    })
    .await;
    assert_eq!(streamed, "Plan updated");
    test.submit_text_turn("Continue the same conversation").await?;

    let requests = requests.requests();
    assert_eq!(requests.len(), 3);
    for request in &requests {
        assert_eq!(request.path(), "/v1/responses");
        assert_eq!(request.header("authorization"), Some(format!("Bearer {TOKEN}")));
        assert_eq!(request.header("chatgpt-account-id"), None);
        let body = request.body_json();
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["reasoning"]["effort"], "high");
    }
    let output = requests[1].function_call_output("plan-call");
    assert_eq!(output["output"], "Plan updated");
    let continued = serde_json::to_string(&requests[2].input())?;
    assert!(continued.contains("Plan updated"));
    assert!(continued.contains("Continue the same conversation"));
    let all_requests = server.received_requests().await.unwrap();
    assert!(all_requests.iter().all(|request| request.url.path() == "/v1/responses"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_provider_key_fails_without_inference() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let env_key = MISSING_ENV_KEY.to_owned();
    assert!(std::env::var_os(&env_key).is_none());
    let expected_key = env_key.clone();
    let result = test_codex()
        .with_model(MODEL)
        .with_config(move |config| {
            config.model_provider_id = "custom-endpoint".to_owned();
            config.model_provider.base_url = Some(base_url);
            config.model_provider.requires_openai_auth = false;
            config.model_provider.env_key = Some(env_key);
        })
        .build_with_auto_env(&server)
        .await;
    let message = match result {
        Err(error) => format!("{error:#}"),
        Ok(test) => {
            test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "hello".into(),
                text_elements: Vec::new(),
            }])).await?;
            let mut message = String::new();
            wait_for_event(&test.codex, |event| {
                if let EventMsg::Error(error) = event {
                    message.push_str(&error.message);
                }
                matches!(event, EventMsg::TurnComplete(_))
            }).await;
            message
        }
    };
    assert!(
        message.contains(&expected_key),
        "missing credential error: {message}"
    );
    assert!(!message.contains(TOKEN));
    assert!(server.received_requests().await.unwrap().iter().all(|request| request.url.path() != "/v1/responses"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rejected_provider_key_is_redacted() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(401).set_body_string(format!("rejected {TOKEN}")))
        .mount(&server)
        .await;
    let test = test_codex()
        .with_model(MODEL)
        .with_config(move |config| {
            config.model_provider_id = "custom-endpoint".to_owned();
            config.model_provider.base_url = Some(base_url.clone());
            config.model_provider.requires_openai_auth = false;
            config.model_provider.experimental_bearer_token = Some(TOKEN.into());
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
        text: "hello".into(),
        text_elements: Vec::new(),
    }])).await?;
    let mut errors = Vec::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::Error(error) = event {
            errors.push(error.message.clone());
        }
        matches!(event, EventMsg::TurnComplete(_))
    }).await;
    assert!(!errors.is_empty(), "expected an authentication error");
    assert!(errors.iter().all(|error| !error.contains(TOKEN)), "provider error exposed the key");
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].url.path(), "/v1/responses");
    assert_eq!(requests[0].headers["authorization"], format!("Bearer {TOKEN}"));
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn provider_bad_request_does_not_expose_echoed_key() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    tracing::info!("provider-http-redaction-test-sentinel");
    let server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(400).insert_header("x-request-id", TOKEN).insert_header("x-openai-authorization-error", TOKEN).set_body_string(format!(
            r#"{{"error":{{"code":"invalid_prompt","message":"Bearer {TOKEN}"}}}}"#
        )))
        .mount(&server)
        .await;
    let test = test_codex()
        .with_model(MODEL)
        .with_config(move |config| {
            config.model_provider_id = "custom-endpoint".to_owned();
            config.model_provider.base_url = Some(base_url);
            config.model_provider.requires_openai_auth = false;
            config.model_provider.experimental_bearer_token = Some(TOKEN.into());
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
        text: "hello".into(),
        text_elements: Vec::new(),
    }])).await?;
    let mut errors = Vec::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::Error(error) = event {
            errors.push(error.clone());
        }
        matches!(event, EventMsg::TurnComplete(_))
    }).await;
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|error| !error.message.contains(TOKEN)), "provider error exposed the key");
    assert!(errors.iter().any(|error| matches!(&error.codex_error_info, Some(codex_protocol::protocol::CodexErrorInfo::InvalidPrompt))), "expected an invalid prompt error");
    let requests = server.received_requests().await.unwrap();
    assert!(!requests.is_empty());
    assert_eq!(requests[0].headers["authorization"], format!("Bearer {TOKEN}"));
    assert!(logs_contain("provider-http-redaction-test-sentinel"));
    assert!(!logs_contain(TOKEN), "logs exposed the key");
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[traced_test]
async fn provider_stream_error_and_headers_do_not_expose_key() -> Result<()> {
    core_test_support::skip_if_no_network!(Ok(()));
    tracing::info!("provider-stream-redaction-test-sentinel");
    let server = responses::start_mock_server().await;
    let base_url = format!("{}/v1", server.uri());
    let body = responses::sse(vec![
        responses::ev_response_created("resp-1"),
        json!({"type": "response.failed", "response": {"error": {
            "code": "invalid_prompt", "message": format!("Bearer {TOKEN}")
        }}}),
    ]);
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200)
            .insert_header("x-request-id", TOKEN)
            .insert_header("x-openai-authorization-error", TOKEN)
            .set_body_raw(body, "text/event-stream"))
        .mount(&server)
        .await;
    let test = test_codex()
        .with_model(MODEL)
        .with_config(move |config| {
            config.model_provider_id = "custom-endpoint".to_owned();
            config.model_provider.base_url = Some(base_url);
            config.model_provider.requires_openai_auth = false;
            config.model_provider.experimental_bearer_token = Some(TOKEN.into());
            config.model_provider.request_max_retries = Some(0);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex.start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
        text: "hello".into(),
        text_elements: Vec::new(),
    }])).await?;
    let mut errors = Vec::new();
    wait_for_event(&test.codex, |event| {
        if let EventMsg::Error(error) = event {
            errors.push(error.message.clone());
        }
        matches!(event, EventMsg::TurnComplete(_))
    }).await;
    assert!(!errors.is_empty());
    assert!(errors.iter().all(|error| !error.contains(TOKEN)));
    assert!(logs_contain("provider-stream-redaction-test-sentinel"));
    assert!(!logs_contain(TOKEN), "logs exposed the key");
    Ok(())
}
