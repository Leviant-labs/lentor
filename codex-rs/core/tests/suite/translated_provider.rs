use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_model_provider_info::WireApi;
use codex_protocol::protocol::EventMsg;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_text_prefix_streams_before_eof_with_server_model_metadata() -> Result<()> {
    use core_test_support::streaming_sse::StreamingSseChunk;
    use core_test_support::streaming_sse::start_streaming_sse_server;

    for protocol in [WireApi::Chat, WireApi::Messages] {
        let (prefix, ending) = match protocol {
            WireApi::Chat => (
                provider_sse(vec![
                    json!({"id":"prefix","model":"streaming-model","choices":[{"index":0,"delta":{"role":"assistant","content":"prefix"},"finish_reason":null}]}),
                ]),
                format!(
                    "{}data: [DONE]\n\n",
                    provider_sse(vec![
                        json!({"id":"prefix","model":"streaming-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})
                    ])
                ),
            ),
            WireApi::Messages => (
                provider_sse(vec![
                    json!({"type":"message_start","message":{"id":"prefix","model":"streaming-model","usage":{}}}),
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"prefix"}}),
                ]),
                provider_sse(vec![
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{}}),
                    json!({"type":"message_stop"}),
                ]),
            ),
            WireApi::Responses => unreachable!("translated protocols only"),
        };
        let (release, gate) = tokio::sync::oneshot::channel();
        let (server, mut completions) = start_streaming_sse_server(vec![vec![
            StreamingSseChunk {
                gate: None,
                body: prefix,
            },
            StreamingSseChunk {
                gate: Some(gate),
                body: ending,
            },
        ]])
        .await;
        let base_url = format!("{}/v1", server.uri());
        let config_server = start_mock_server().await;
        let test = test_codex()
            .with_config(move |config| {
                config.model_provider_id = "translated-streaming-test".into();
                config.model_provider.base_url = Some(base_url);
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
            })
            .build_with_auto_env(&config_server)
            .await?;
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "stream a prefix".into(),
                text_elements: Vec::new(),
            }]))
            .await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_for_event_match(&test.codex, |event| match event {
                EventMsg::Error(error) => panic!("translated prefix failed: {}", error.message),
                EventMsg::AgentMessageContentDelta(delta) if delta.delta == "prefix" => Some(()),
                _ => None,
            }),
        )
        .await?;
        assert!(matches!(
            completions[0].try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        release
            .send(())
            .expect("stream remains open until prefix is observed");
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_for_event_match(&test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_)).then_some(())
            }),
        )
        .await?;
        test.codex.shutdown_and_wait().await?;
        server.shutdown().await;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_protocols_stream_text_through_a_real_turn() -> Result<()> {
    for (protocol, endpoint, body) in [
        (
            WireApi::Responses,
            "/v1/responses",
            core_test_support::responses::sse(vec![
                core_test_support::responses::ev_response_created("r"),
                core_test_support::responses::ev_assistant_message("msg_1", "hello"),
                core_test_support::responses::ev_completed("r"),
            ]),
        ),
        (
            WireApi::Chat,
            "/v1/chat/completions",
            concat!(
                "data: {\"id\":\"r\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
                "data: {\"id\":\"r\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                "data: [DONE]\n\n"
            ).to_owned(),
        ),
        (
            WireApi::Messages,
            "/v1/messages",
            concat!(
                "data: {\"type\":\"message_start\",\"message\":{\"id\":\"r\",\"model\":\"m\",\"role\":\"assistant\",\"usage\":{}}}\n\n",
                "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"hello\"}}\n\n",
                "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
                "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{}}\n\n",
                "data: {\"type\":\"message_stop\"}\n\n"
            ).to_owned(),
        ),
    ] {
        let server = start_mock_server().await;
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(body),
            )
            .expect(1)
            .mount(&server)
            .await;
        let test = test_codex()
            .with_config(move |config| {
                config.model_provider_id = "translated-test".to_string();
                config.model_provider.name = "Translated test".to_string();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
            })
            .build_with_auto_env(&server)
            .await?;
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "hello".to_string(),
                text_elements: Vec::new(),
            }]))
            .await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_for_event_match(&test.codex, |event| {
                if let EventMsg::Error(error) = event {
                    panic!("translated text turn failed: {}", error.message);
                }
                matches!(event, EventMsg::AgentMessage(message) if message.message == "hello")
                    .then_some(())
            }),
        )
        .await?;
        let requests = server.received_requests().await.unwrap();
        let calls: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST" && request.url.path() == endpoint)
            .collect();
        assert_eq!(calls.len(), 1);
        let body: serde_json::Value = serde_json::from_slice(&calls[0].body)?;
        let tools = body["tools"].as_array().expect("local tool definitions");
        assert!(protocol == WireApi::Responses ||
            !tools
                .iter()
                .any(|tool| tool["type"] == "web_search" || tool["type"] == "tool_search")
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_for_event_match(&test.codex, |event| {
                matches!(event, EventMsg::TurnComplete(_)).then_some(())
            }),
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn truncated_translated_stream_reports_error_without_falling_back() -> Result<()> {
    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let server = start_mock_server().await;
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string("data: {\"unfinished\":true}"),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/responses"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let test = test_codex()
            .with_config(move |config| {
                config.model_provider_id = "translated-test".to_string();
                config.model_provider.name = "Translated test".to_string();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.model_provider.stream_max_retries = Some(0);
            })
            .build_with_auto_env(&server)
            .await?;
        test.codex
            .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
                text: "hello".to_string(),
                text_elements: Vec::new(),
            }]))
            .await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            wait_for_event_match(&test.codex, |event| {
                matches!(event, EventMsg::Error(_)).then_some(())
            }),
        )
        .await?;
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_provider_reports_explicit_hosted_search_as_unsupported() -> Result<()> {
    let server = start_mock_server().await;
    let test = test_codex()
        .with_pre_build_hook(|home| {
            std::fs::write(home.join("config.toml"), "web_search = 'live'\n").unwrap();
        })
        .with_config(|config| {
            config.model_provider_id = "translated-test".to_string();
            config.model_provider.name = "Translated test".to_string();
            config.model_provider.wire_api = WireApi::Messages;
            config.model_provider.requires_openai_auth = false;
        })
        .build_with_auto_env(&server)
        .await?;
    test.codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "hello".to_string(),
            text_elements: Vec::new(),
        }]))
        .await?;
    let message = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        wait_for_event_match(&test.codex, |event| {
            if let EventMsg::Error(error) = event {
                Some(error.message.clone())
            } else {
                None
            }
        }),
    )
    .await?;
    assert!(
        message.contains("hosted web search is unsupported"),
        "{message}"
    );
    assert!(
        !server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.method == wiremock::http::Method::POST)
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn chat_reasoning_survives_local_tool_continuation_and_resume() -> Result<()> {
    use codex_features::Feature;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::openai_models::ConfigShellToolType;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use core_test_support::test_codex::turn_permission_fields;
    use serde_json::json;

    let server = start_mock_server().await;
    let counter = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(move |_: &wiremock::Request| {
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let body = if index == 0 {
                concat!(
                    "data: {\"id\":\"r1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"reasoning_content\":\"think before tool\",\"tool_calls\":[{\"index\":0,\"id\":\"call-1\",\"type\":\"function\",\"function\":{\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"echo lentor-test\\\",\\\"login\\\":false,\\\"yield_time_ms\\\":1000}\"}}]},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"r1\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
                    "data: [DONE]\n\n"
                )
            } else {
                concat!(
                    "data: {\"id\":\"r2\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"done\"},\"finish_reason\":null}]}\n\n",
                    "data: {\"id\":\"r2\",\"model\":\"m\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                    "data: [DONE]\n\n"
                )
            };
            ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(body)
        })
        .expect(3)
        .mount(&server)
        .await;
    let make_builder = || {
        test_codex()
            .with_model_info_override("gpt-5.5", |model| {
                model.shell_type = ConfigShellToolType::UnifiedExec
            })
            .with_config(|config| {
                config.model_provider_id = "translated-test".to_string();
                config.model_provider.name = "Translated test".to_string();
                config.model_provider.wire_api = WireApi::Chat;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.features.enable(Feature::ShellTool).unwrap();
                config.features.enable(Feature::UnifiedExec).unwrap();
            })
    };
    let mut builder = make_builder();
    let test = builder.build_with_auto_env(&server).await?;
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.cwd.path());
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "run the command".into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                ..Default::default()
            }),
        )
        .await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        wait_for_event_match(&test.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_)).then_some(())
        }),
    )
    .await?;
    let mut resume_builder = make_builder();
    let resumed = resume_builder.restart_with_auto_env(&server, &test).await?;
    resumed
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "continue".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        wait_for_event_match(&resumed.codex, |event| {
            matches!(event, EventMsg::TurnComplete(_)).then_some(())
        }),
    )
    .await?;
    let requests = server.received_requests().await.unwrap();
    let calls: Vec<_> = requests
        .iter()
        .filter(|request| request.method == "POST" && request.url.path() == "/v1/chat/completions")
        .collect();
    assert_eq!(calls.len(), 3);
    for request in &calls[1..] {
        let body: serde_json::Value = serde_json::from_slice(&request.body)?;
        let messages = body["messages"].as_array().expect("messages");
        assert!(
            messages
                .iter()
                .any(|message| message["reasoning_content"] == json!("think before tool"))
        );
        assert!(messages.iter().any(|message| message["role"] == "tool"
            && message["tool_call_id"] == "call-1"
            && message["content"].to_string().contains("lentor-test")));
    }
    Ok(())
}

fn provider_sse(frames: Vec<serde_json::Value>) -> String {
    frames
        .into_iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect()
}

async fn translated_turn_raw_items(
    test: &core_test_support::test_codex::TestCodex,
    phase: &str,
) -> Result<Vec<codex_protocol::models::ResponseItem>> {
    tokio::time::timeout(std::time::Duration::from_secs(30), async {
        let mut items = Vec::new();
        loop {
            match test.codex.next_event().await?.msg {
                EventMsg::RawResponseItem(raw) => items.push(raw.item),
                EventMsg::Error(error) => panic!(
                    "{phase} failed after {} raw response items: {}",
                    items.len(),
                    error.message
                ),
                EventMsg::TurnComplete(_) => return Ok(items),
                _ => {}
            }
        }
    })
    .await?
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn messages_signed_thinking_survives_local_tool_continuation_and_resume() -> Result<()> {
    use codex_features::Feature;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::openai_models::ConfigShellToolType;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use core_test_support::test_codex::turn_permission_fields;

    let server = start_mock_server().await;
    let counter = AtomicUsize::new(0);
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(move |_: &wiremock::Request| {
            let index = counter.fetch_add(1, Ordering::SeqCst);
            let first = index == 0;
            let mut frames = vec![json!({"type":"message_start","message":{"id":format!("signed-r-{index}"),"model":"m","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}})];
            if first {
                frames.extend([
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think before exec"}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"private-signed-"}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"thinking-sentinel"}}),
                    json!({"type":"content_block_stop","index":0}),
                    json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"signed-exec","name":"exec_command","input":{}}}),
                    json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":json!({"cmd":"echo signed-exec-output","login":false,"yield_time_ms":10000}).to_string()}}),
                    json!({"type":"content_block_stop","index":1}),
                ]);
            } else {
                frames.extend([
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"done"}}),
                    json!({"type":"content_block_stop","index":0}),
                ]);
            }
            frames.extend([
                json!({"type":"message_delta","delta":{"stop_reason":if first { "tool_use" } else { "end_turn" }},"usage":{"output_tokens":2}}),
                json!({"type":"message_stop"}),
            ]);
            ResponseTemplate::new(200).insert_header("content-type", "text/event-stream")
                .set_body_string(provider_sse(frames))
        })
        .expect(3)
        .mount(&server).await;
    let make_builder = || {
        test_codex()
            .with_model_info_override("gpt-5.5", |model| {
                model.shell_type = ConfigShellToolType::UnifiedExec
            })
            .with_config(|config| {
                config.model_provider_id = "signed-messages-test".into();
                config.model_provider.wire_api = WireApi::Messages;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.features.enable(Feature::ShellTool).unwrap();
                config.features.enable(Feature::UnifiedExec).unwrap();
            })
    };
    let test = make_builder().build_with_auto_env(&server).await?;
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
    test.codex
        .start_or_steer_turn(
            TurnInputRequest::user_input(vec![UserInput::Text {
                text: "run the command".into(),
                text_elements: Vec::new(),
            }])
            .with_thread_settings(ThreadSettingsOverrides {
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                ..Default::default()
            }),
        )
        .await?;
    let mut raw_items =
        translated_turn_raw_items(&test, "Messages signed thinking/tool continuation").await?;
    assert!(
        raw_items
            .iter()
            .any(|item| matches!(item, ResponseItem::Reasoning { .. }))
    );
    let resumed = make_builder().restart_with_auto_env(&server, &test).await?;
    resumed
        .codex
        .start_or_steer_turn(TurnInputRequest::user_input(vec![UserInput::Text {
            text: "continue".into(),
            text_elements: Vec::new(),
        }]))
        .await?;
    raw_items.extend(translated_turn_raw_items(&resumed, "Messages signed thinking resume").await?);
    let public = serde_json::to_string(&raw_items)?;
    assert!(!public.contains("private-signed-"));
    assert!(!public.contains("thinking-sentinel"));
    let requests = server.received_requests().await.unwrap();
    let calls: Vec<_> = requests
        .iter()
        .filter(|request| request.method == "POST")
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(
        calls
            .iter()
            .all(|request| request.url.path() == "/v1/messages")
    );
    for request in &calls[1..] {
        let body: serde_json::Value = serde_json::from_slice(&request.body)?;
        let messages = body["messages"].as_array().expect("Messages history");
        let (index, assistant) = messages
            .iter()
            .enumerate()
            .find(|(_, message)| {
                message["role"] == "assistant"
                    && message["content"].as_array().is_some_and(|blocks| {
                        blocks.iter().any(|block| block["type"] == "thinking")
                    })
            })
            .expect("assistant thinking and tool use");
        assert_eq!(
            assistant["content"],
            json!([
                {"type":"thinking","thinking":"think before exec","signature":"private-signed-thinking-sentinel"},
                {"type":"tool_use","id":"signed-exec","name":"exec_command","input":{"cmd":"echo signed-exec-output","login":false,"yield_time_ms":10000}}
            ])
        );
        let result = &messages[index + 1];
        assert_eq!(result["role"], "user");
        let blocks = result["content"].as_array().expect("tool result blocks");
        let output = blocks
            .iter()
            .find(|block| block["type"] == "tool_result" && block["tool_use_id"] == "signed-exec")
            .expect("matching exec tool result");
        assert!(output["content"].to_string().contains("signed-exec-output"));
        assert!(
            output["content"]
                .to_string()
                .contains("Process exited with code 0")
        );
        assert_ne!(output["is_error"], json!(true));
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_freeform_apply_patch_mutates_file_and_continues() -> Result<()> {
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::models::ResponseItem;
    use codex_protocol::openai_models::ApplyPatchToolType;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use core_test_support::test_codex::turn_permission_fields;

    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let server = start_mock_server().await;
        let counter = AtomicUsize::new(0);
        let patch = "*** Begin Patch\n*** Add File: translated-patch.txt\n+translated patch content\n*** End Patch\n";
        Mock::given(method("POST")).and(path(endpoint))
            .respond_with(move |_: &wiremock::Request| {
                let index = counter.fetch_add(1, Ordering::SeqCst);
                let first = index == 0;
                let body = match protocol {
                    WireApi::Chat => {
                        let delta = if first {
                            json!({"role":"assistant","tool_calls":[{"index":0,"id":"patch-call","type":"function","function":{"name":"apply_patch","arguments":json!({"input":patch}).to_string()}}]})
                        } else { json!({"role":"assistant","content":"patched"}) };
                        let mut body = provider_sse(vec![
                            json!({"id":format!("patch-r-{index}"),"model":"m","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                            json!({"id":format!("patch-r-{index}"),"model":"m","choices":[{"index":0,"delta":{},"finish_reason":if first { "tool_calls" } else { "stop" }}]}),
                        ]);
                        body.push_str("data: [DONE]\n\n");
                        body
                    }
                    WireApi::Messages => {
                        let mut frames = vec![json!({"type":"message_start","message":{"id":format!("patch-r-{index}"),"model":"m","role":"assistant","content":[],"usage":{}}})];
                        if first {
                            frames.extend([
                                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"patch-call","name":"apply_patch","input":{}}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":json!({"input":patch}).to_string()}}),
                            ]);
                        } else {
                            frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"patched"}}));
                        }
                        frames.extend([
                            json!({"type":"content_block_stop","index":0}),
                            json!({"type":"message_delta","delta":{"stop_reason":if first { "tool_use" } else { "end_turn" }},"usage":{}}),
                            json!({"type":"message_stop"}),
                        ]);
                        provider_sse(frames)
                    }
                    WireApi::Responses => unreachable!("translated protocols only"),
                };
                ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(body)
            }).expect(2).mount(&server).await;
        let test = test_codex()
            .with_model_info_override("gpt-5.5", |model| {
                model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform)
            })
            .with_config(move |config| {
                config.model_provider_id = "translated-patch-test".into();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
            })
            .build_with_auto_env(&server)
            .await?;
        let (sandbox_policy, permission_profile) =
            turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
        test.codex
            .start_or_steer_turn(
                TurnInputRequest::user_input(vec![UserInput::Text {
                    text: "apply the patch".into(),
                    text_elements: Vec::new(),
                }])
                .with_thread_settings(ThreadSettingsOverrides {
                    approval_policy: Some(AskForApproval::Never),
                    sandbox_policy: Some(sandbox_policy),
                    permission_profile,
                    ..Default::default()
                }),
            )
            .await?;
        let raw_items = translated_turn_raw_items(&test, "freeform patch continuation").await?;
        assert!(raw_items.iter().any(|item| matches!(item,
            ResponseItem::CustomToolCall { name, call_id, input, .. }
                if name == "apply_patch" && call_id == "patch-call" && input == patch
        )));
        let contents = test
            .fs()
            .read_file_text(
                &test.workspace_path_uri("translated-patch.txt")?,
                Default::default(),
                /*sandbox*/ None,
            )
            .await?;
        assert_eq!(contents, "translated patch content\n");
        let requests = server.received_requests().await.unwrap();
        let calls: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST")
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|request| request.url.path() == endpoint));
        let body: serde_json::Value = serde_json::from_slice(&calls[1].body)?;
        let messages = body["messages"]
            .as_array()
            .expect("continued provider history");
        let output = match protocol {
            WireApi::Chat => {
                let call = messages
                    .iter()
                    .find(|message| message["tool_calls"][0]["id"] == "patch-call")
                    .expect("patch call");
                assert_eq!(
                    serde_json::from_str::<serde_json::Value>(
                        call["tool_calls"][0]["function"]["arguments"]
                            .as_str()
                            .unwrap()
                    )?,
                    json!({"input":patch})
                );
                messages
                    .iter()
                    .find(|message| {
                        message["role"] == "tool" && message["tool_call_id"] == "patch-call"
                    })
                    .expect("patch result")["content"]
                    .clone()
            }
            WireApi::Messages => {
                let blocks: Vec<_> = messages
                    .iter()
                    .filter_map(|message| message["content"].as_array())
                    .flatten()
                    .collect();
                let call = blocks
                    .iter()
                    .find(|block| block["type"] == "tool_use" && block["id"] == "patch-call")
                    .expect("patch call");
                assert_eq!(call["input"], json!({"input":patch}));
                blocks
                    .iter()
                    .find(|block| {
                        block["type"] == "tool_result" && block["tool_use_id"] == "patch-call"
                    })
                    .expect("patch result")["content"]
                    .clone()
            }
            WireApi::Responses => unreachable!("translated protocols only"),
        };
        assert!(
            output
                .to_string()
                .contains("Success. Updated the following files:")
        );
        assert!(output.to_string().contains("translated-patch.txt"));
    }
    Ok(())
}
