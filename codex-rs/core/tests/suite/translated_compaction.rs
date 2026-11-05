use anyhow::Result;
use codex_model_provider_info::WireApi;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn resumed_translated_history_is_hydrated_before_manual_compaction() -> Result<()> {
    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let server = start_mock_server().await;
        let counter = AtomicUsize::new(0);
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(move |_: &wiremock::Request| {
                let first = counter.fetch_add(1, Ordering::SeqCst) == 0;
                let frames = match protocol {
                    WireApi::Chat => {
                        let delta = if first {
                            json!({"role":"assistant","reasoning_content":"private reasoning","content":"initial answer"})
                        } else {
                            json!({"role":"assistant","content":"compacted summary"})
                        };
                        let mut frames = vec![
                            json!({"id":"chat-r","model":"m","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                            json!({"id":"chat-r","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                        ].into_iter().map(|frame| format!("data: {frame}\n\n")).collect::<String>();
                        frames.push_str("data: [DONE]\n\n");
                        frames
                    }
                    WireApi::Messages => {
                        let mut frames = vec![json!({"type":"message_start","message":{"id":"messages-r","model":"m","role":"assistant","content":[],"usage":{"input_tokens":1,"output_tokens":0}}})];
                        if first {
                            frames.extend([
                                json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private reasoning"}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"signed-state-sentinel"}}),
                                json!({"type":"content_block_stop","index":0}),
                            ]);
                        }
                        let index = usize::from(first);
                        frames.extend([
                            json!({"type":"content_block_start","index":index,"content_block":{"type":"text","text":if first { "initial answer" } else { "compacted summary" }}}),
                            json!({"type":"content_block_stop","index":index}),
                            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
                            json!({"type":"message_stop"}),
                        ]);
                        frames.into_iter().map(|frame| format!("data: {frame}\n\n")).collect()
                    }
                    WireApi::Responses => unreachable!("translated protocols only"),
                };
                ResponseTemplate::new(200)
                    .insert_header("content-type", "text/event-stream")
                    .set_body_string(frames)
            })
            .expect(2)
            .mount(&server)
            .await;
        let make_builder = || {
            test_codex().with_config(move |config| {
                config.model_provider_id = "translated-compaction-test".into();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.model_provider.stream_max_retries = Some(0);
            })
        };
        let test = make_builder().build_with_auto_env(&server).await?;
        test.submit_text_turn("remember this").await?;
        let resumed = make_builder().restart_with_auto_env(&server, &test).await?;
        resumed.codex.submit(Op::Compact).await?;
        let mut public_events = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(30), async {
            loop {
                let event = resumed.codex.next_event().await?.msg;
                match &event {
                    EventMsg::Error(error) => panic!("compaction failed: {}", error.message),
                    EventMsg::ContextCompacted(_) => break,
                    EventMsg::RawResponseItem(raw) => public_events.push(raw.item.clone()),
                    _ => {}
                }
            }
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        assert!(!serde_json::to_string(&public_events)?.contains("signed-state-sentinel"));
        let requests = server.received_requests().await.unwrap();
        let calls: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST")
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|request| request.url.path() == endpoint));
        let body: serde_json::Value = serde_json::from_slice(&calls[1].body)?;
        let messages = body["messages"].as_array().expect("compaction history");
        match protocol {
            WireApi::Chat => assert!(
                messages
                    .iter()
                    .any(|message| { message["reasoning_content"] == "private reasoning" })
            ),
            WireApi::Messages => assert!(messages.iter().any(|message| {
                message["content"].as_array().is_some_and(|blocks| {
                    blocks.iter().any(|block| {
                        block["type"] == "thinking"
                            && block["thinking"] == "private reasoning"
                            && block["signature"] == "signed-state-sentinel"
                    })
                })
            })),
            WireApi::Responses => unreachable!("translated protocols only"),
        }
    }
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_manual_compaction_preserves_executed_shell_history() -> Result<()> {
    use codex_core::TurnInputRequest;
    use codex_features::Feature;
    use codex_protocol::models::PermissionProfile;
    use codex_protocol::openai_models::ConfigShellToolType;
    use codex_protocol::protocol::AskForApproval;
    use codex_protocol::protocol::ThreadSettingsOverrides;
    use codex_protocol::user_input::UserInput;
    use core_test_support::test_codex::turn_permission_fields;

    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let server = start_mock_server().await;
        let counter = AtomicUsize::new(0);
        let args = json!({"cmd":"echo compaction-tool-output","login":false,"yield_time_ms":10000});
        let response_args = args.clone();
        Mock::given(method("POST")).and(path(endpoint))
            .respond_with(move |_: &wiremock::Request| {
                let index = counter.fetch_add(1, Ordering::SeqCst);
                let first = index == 0;
                let text = if index == 1 { "command finished" } else { "compacted shell history" };
                let frames = match protocol {
                    WireApi::Chat => {
                        let delta = if first {
                            json!({"role":"assistant","tool_calls":[{"index":0,"id":"compact-exec","type":"function","function":{"name":"exec_command","arguments":response_args.to_string()}}]})
                        } else { json!({"role":"assistant","content":text}) };
                        let mut body = [
                            json!({"id":format!("chat-{index}"),"model":"m","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                            json!({"id":format!("chat-{index}"),"model":"m","choices":[{"index":0,"delta":{},"finish_reason":if first { "tool_calls" } else { "stop" }}]}),
                        ].into_iter().map(|frame| format!("data: {frame}\n\n")).collect::<String>();
                        body.push_str("data: [DONE]\n\n");
                        body
                    }
                    WireApi::Messages => {
                        let mut frames = vec![json!({"type":"message_start","message":{"id":format!("messages-{index}"),"model":"m","role":"assistant","content":[],"usage":{}}})];
                        if first {
                            frames.extend([
                                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"compact-exec","name":"exec_command","input":{}}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":response_args.to_string()}}),
                            ]);
                        } else {
                            frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":text}}));
                        }
                        frames.extend([
                            json!({"type":"content_block_stop","index":0}),
                            json!({"type":"message_delta","delta":{"stop_reason":if first { "tool_use" } else { "end_turn" }},"usage":{"output_tokens":2}}),
                            json!({"type":"message_stop"}),
                        ]);
                        frames.into_iter().map(|frame| format!("data: {frame}\n\n")).collect()
                    }
                    WireApi::Responses => unreachable!("translated protocols only"),
                };
                ResponseTemplate::new(200).insert_header("content-type", "text/event-stream").set_body_string(frames)
            }).expect(3).mount(&server).await;
        let test = test_codex()
            .with_model_info_override("gpt-5.5", |model| {
                model.shell_type = ConfigShellToolType::UnifiedExec
            })
            .with_config(move |config| {
                config.model_provider_id = "translated-compaction-tool-test".into();
                config.model_provider.wire_api = protocol;
                config.model_provider.requires_openai_auth = false;
                config.model_provider.supports_websockets = false;
                config.model_provider.stream_max_retries = Some(0);
                config.features.enable(Feature::ShellTool).unwrap();
                config.features.enable(Feature::UnifiedExec).unwrap();
            })
            .build_with_auto_env(&server)
            .await?;
        let (sandbox_policy, permission_profile) =
            turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
        for compact in [false, true] {
            if compact {
                test.codex.submit(Op::Compact).await?;
            } else {
                test.codex
                    .start_or_steer_turn(
                        TurnInputRequest::user_input(vec![UserInput::Text {
                            text: "run the command".into(),
                            text_elements: Vec::new(),
                        }])
                        .with_thread_settings(ThreadSettingsOverrides {
                            approval_policy: Some(AskForApproval::Never),
                            sandbox_policy: Some(sandbox_policy.clone()),
                            permission_profile: permission_profile.clone(),
                            ..Default::default()
                        }),
                    )
                    .await?;
            }
            tokio::time::timeout(std::time::Duration::from_secs(30), async {
                loop {
                    match test
                        .codex
                        .next_event()
                        .await
                        .expect("synthetic tool history event")
                        .msg
                    {
                        EventMsg::Error(error) => panic!(
                            "{protocol:?} shell history compact={compact}: {}",
                            error.message
                        ),
                        EventMsg::TurnComplete(_) if !compact => break,
                        EventMsg::ContextCompacted(_) if compact => break,
                        _ => {}
                    }
                }
            })
            .await
            .unwrap_or_else(|_| panic!("{protocol:?} shell history compact={compact} timed out"));
            let requests = server.received_requests().await.unwrap();
            let calls: Vec<_> = requests
                .iter()
                .filter(|request| request.method == "POST")
                .collect();
            assert_eq!(calls.len(), if compact { 3 } else { 2 });
            assert!(calls.iter().all(|request| request.url.path() == endpoint));
            let body: serde_json::Value = serde_json::from_slice(&calls.last().unwrap().body)?;
            let messages = body["messages"]
                .as_array()
                .expect("historical tool messages");
            let output = match protocol {
                WireApi::Chat => {
                    let call = messages
                        .iter()
                        .find(|message| message["tool_calls"][0]["id"] == "compact-exec")
                        .expect("historical shell call");
                    assert_eq!(call["tool_calls"][0]["function"]["name"], "exec_command");
                    assert_eq!(
                        serde_json::from_str::<serde_json::Value>(
                            call["tool_calls"][0]["function"]["arguments"]
                                .as_str()
                                .unwrap()
                        )?,
                        args
                    );
                    messages
                        .iter()
                        .find(|message| {
                            message["role"] == "tool" && message["tool_call_id"] == "compact-exec"
                        })
                        .expect("historical shell result")["content"]
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
                        .find(|block| block["type"] == "tool_use" && block["id"] == "compact-exec")
                        .expect("historical shell call");
                    assert_eq!(call["name"], "exec_command");
                    assert_eq!(call["input"], args);
                    let result = blocks
                        .iter()
                        .find(|block| {
                            block["type"] == "tool_result" && block["tool_use_id"] == "compact-exec"
                        })
                        .expect("historical shell result");
                    assert_ne!(result["is_error"], json!(true));
                    result["content"].clone()
                }
                WireApi::Responses => unreachable!("translated protocols only"),
            };
            assert!(output.to_string().contains("compaction-tool-output"));
            assert!(output.to_string().contains("Process exited with code 0"));
        }
    }
    Ok(())
}
