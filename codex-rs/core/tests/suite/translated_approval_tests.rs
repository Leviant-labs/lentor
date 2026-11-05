//! Translated shell calls retain the local user approval boundary.

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_features::Feature;
use codex_model_provider_info::WireApi;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ReviewDecision;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::start_mock_server;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event_match;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_exec_command_obeys_user_approval() -> Result<()> {
    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        for approved in [true, false] {
            let server = start_mock_server().await;
            let counter = AtomicUsize::new(0);
            let args = json!({
                "cmd": "echo translated-approval-output > translated-approval-marker.txt",
                "login": false,
                "yield_time_ms": 10000,
                "sandbox_permissions": "require_escalated",
                "justification": "Run the requested command."
            });
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(move |_: &wiremock::Request| {
                    let index = counter.fetch_add(1, Ordering::SeqCst);
                    let first = index == 0;
                    let frames = match protocol {
                        WireApi::Chat => {
                            let delta = if first {
                                json!({"role":"assistant","tool_calls":[{"index":0,"id":"approval-call","type":"function","function":{"name":"exec_command","arguments":args.to_string()}}]})
                            } else {
                                json!({"role":"assistant","content":"done"})
                            };
                            vec![
                                json!({"id":"approval-response","model":"m","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                                json!({"id":"approval-response","model":"m","choices":[{"index":0,"delta":{},"finish_reason":if first { "tool_calls" } else { "stop" }}]}),
                            ]
                        }
                        WireApi::Messages => {
                            let mut frames = vec![json!({"type":"message_start","message":{"id":"approval-response","model":"m","role":"assistant","content":[],"usage":{}}})];
                            if first {
                                frames.extend([
                                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"approval-call","name":"exec_command","input":{}}}),
                                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args.to_string()}}),
                                ]);
                            } else {
                                frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"done"}}));
                            }
                            frames.extend([
                                json!({"type":"content_block_stop","index":0}),
                                json!({"type":"message_delta","delta":{"stop_reason":if first { "tool_use" } else { "end_turn" }},"usage":{}}),
                                json!({"type":"message_stop"}),
                            ]);
                            frames
                        }
                        WireApi::Responses => unreachable!("translated protocols only"),
                    };
                    let mut body: String = frames.into_iter().map(|frame| format!("data: {frame}\n\n")).collect();
                    if protocol == WireApi::Chat {
                        body.push_str("data: [DONE]\n\n");
                    }
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(body)
                })
                .expect(2)
                .mount(&server)
                .await;
            let test = test_codex()
                .with_model_info_override("gpt-5.5", |model| {
                    model.shell_type = ConfigShellToolType::UnifiedExec;
                })
                .with_config(move |config| {
                    config.model_provider_id = "translated-approval-test".into();
                    config.model_provider.wire_api = protocol;
                    config.model_provider.requires_openai_auth = false;
                    config.model_provider.supports_websockets = false;
                    config.features.enable(Feature::ShellTool).unwrap();
                    config.features.enable(Feature::UnifiedExec).unwrap();
                })
                .build_with_auto_env(&server)
                .await?;
            let marker = test.cwd.path().join("translated-approval-marker.txt");
            let (sandbox_policy, permission_profile) =
                turn_permission_fields(PermissionProfile::read_only(), test.cwd.path());
            test.codex
                .start_or_steer_turn(
                    TurnInputRequest::user_input(vec![UserInput::Text {
                        text: "run the command".into(),
                        text_elements: Vec::new(),
                    }])
                    .with_thread_settings(ThreadSettingsOverrides {
                        approval_policy: Some(AskForApproval::OnRequest),
                        sandbox_policy: Some(sandbox_policy),
                        permission_profile,
                        ..Default::default()
                    }),
                )
                .await?;
            let approval = tokio::time::timeout(
                Duration::from_secs(30),
                wait_for_event_match(&test.codex, |event| match event {
                    EventMsg::ExecApprovalRequest(approval) => Some(approval.clone()),
                    EventMsg::Error(error) => panic!("approval turn failed: {}", error.message),
                    EventMsg::TurnComplete(_) => panic!("turn completed without approval"),
                    _ => None,
                }),
            )
            .await?;
            assert!(!marker.exists(), "command ran before approval");
            test.codex
                .submit(Op::ExecApproval {
                    id: approval.effective_approval_id(),
                    turn_id: None,
                    decision: if approved {
                        ReviewDecision::Approved
                    } else {
                        ReviewDecision::denied("rejected by user")
                    },
                })
                .await?;
            tokio::time::timeout(
                Duration::from_secs(30),
                wait_for_event_match(&test.codex, |event| match event {
                    EventMsg::Error(error) => panic!("continuation failed: {}", error.message),
                    EventMsg::TurnComplete(_) => Some(()),
                    EventMsg::ExecApprovalRequest(_) => panic!("unexpected second approval"),
                    _ => None,
                }),
            )
            .await?;
            assert_eq!(marker.exists(), approved);
            if approved {
                assert!(!std::fs::read(&marker)?.is_empty());
            }
            let requests = server.received_requests().await.unwrap();
            let calls: Vec<_> = requests
                .iter()
                .filter(|request| request.method == "POST")
                .collect();
            assert_eq!(calls.len(), 2);
            assert!(calls.iter().all(|request| request.url.path() == endpoint));
            let body: serde_json::Value = serde_json::from_slice(&calls[1].body)?;
            let messages = body["messages"].as_array().expect("continuation messages");
            let output = match protocol {
                WireApi::Chat => &messages
                    .iter()
                    .find(|message| {
                        message["role"] == "tool" && message["tool_call_id"] == "approval-call"
                    })
                    .expect("matching Chat tool result")["content"],
                WireApi::Messages => &messages
                    .iter()
                    .filter_map(|message| message["content"].as_array())
                    .flatten()
                    .find(|block| {
                        block["type"] == "tool_result" && block["tool_use_id"] == "approval-call"
                    })
                    .expect("matching Messages tool result")["content"],
                WireApi::Responses => unreachable!("translated protocols only"),
            };
            let output = output.to_string();
            if approved {
                assert!(output.contains("Process exited with code 0"), "{output}");
            } else {
                assert!(output.contains("rejected by user"), "{output}");
                assert!(!output.contains("translated-approval-output"), "{output}");
            }
        }
    }
    Ok(())
}
