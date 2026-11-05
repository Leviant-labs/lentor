//! Cancellation must not release a translated tool buffered before HTTP EOF.

use std::time::Duration;

use anyhow::Result;
use codex_core::TurnInputRequest;
use codex_model_provider_info::WireApi;
use codex_protocol::models::PermissionProfile;
use codex_protocol::openai_models::ApplyPatchToolType;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::Op;
use codex_protocol::protocol::ThreadSettingsOverrides;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::start_mock_server;
use core_test_support::streaming_sse::StreamingSseChunk;
use core_test_support::streaming_sse::start_streaming_sse_server;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use serde_json::json;
use tokio::time::timeout;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_withholds_translated_patch_until_clean_eof() -> Result<()> {
    const WAIT: Duration = Duration::from_secs(20);
    const TARGET: &str = "cancelled-translated-patch.txt";
    let patch =
        format!("*** Begin Patch\n*** Add File: {TARGET}\n+must not be created\n*** End Patch\n");

    for protocol in [WireApi::Chat, WireApi::Messages] {
        let arguments = json!({"input": patch}).to_string();
        let frames = match protocol {
            WireApi::Chat => vec![
                json!({"id":"cancel","model":"m","choices":[{"index":0,"delta":{"role":"assistant","content":"prefix","tool_calls":[{"index":0,"id":"patch-call","type":"function","function":{"name":"apply_patch","arguments":arguments}}]},"finish_reason":null}]}),
                json!({"id":"cancel","model":"m","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
            ],
            WireApi::Messages => vec![
                json!({"type":"message_start","message":{"id":"cancel","model":"m","role":"assistant","content":[],"usage":{}}}),
                json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"prefix"}}),
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"patch-call","name":"apply_patch","input":{}}}),
                json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                json!({"type":"content_block_stop","index":1}),
                json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{}}),
                json!({"type":"message_stop"}),
            ],
            WireApi::Responses => unreachable!("translated protocols only"),
        };
        let mut body: String = frames
            .into_iter()
            .map(|frame| format!("data: {frame}\n\n"))
            .collect();
        if protocol == WireApi::Chat {
            body.push_str("data: [DONE]\n\n");
        }
        let (release, gate) = tokio::sync::oneshot::channel();
        let (server, mut completions) = start_streaming_sse_server(vec![vec![
            StreamingSseChunk { gate: None, body },
            StreamingSseChunk {
                gate: Some(gate),
                body: String::new(),
            },
        ]])
        .await;
        let base_url = format!("{}/v1", server.uri());
        let config_server = start_mock_server().await;
        let test = timeout(
            WAIT,
            test_codex()
                .with_model_info_override("gpt-5.5", |model| {
                    model.apply_patch_tool_type = Some(ApplyPatchToolType::Freeform);
                })
                .with_config(move |config| {
                    config.model_provider_id = "translated-cancellation-test".into();
                    config.model_provider.base_url = Some(base_url);
                    config.model_provider.wire_api = protocol;
                    config.model_provider.requires_openai_auth = false;
                    config.model_provider.supports_websockets = false;
                    config.model_provider.stream_max_retries = Some(0);
                })
                .build_with_auto_env(&config_server),
        )
        .await??;
        let target = test.cwd.path().join(TARGET);
        assert!(!target.exists());
        let (sandbox_policy, permission_profile) =
            turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
        timeout(
            WAIT,
            test.codex.start_or_steer_turn(
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
            ),
        )
        .await??;

        timeout(WAIT, async {
            loop {
                match test.codex.next_event().await?.msg {
                    EventMsg::AgentMessageContentDelta(delta) if delta.delta == "prefix" => break,
                    EventMsg::PatchApplyBegin(_) => panic!("patch executed before cancellation"),
                    EventMsg::Error(error) => panic!("translated stream failed: {}", error.message),
                    EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => {
                        panic!("turn ended before prefix")
                    }
                    _ => {}
                }
            }
            anyhow::Ok(())
        })
        .await??;
        assert!(matches!(
            completions[0].try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        timeout(WAIT, test.codex.submit(Op::Interrupt)).await??;
        timeout(WAIT, async {
            loop {
                match test.codex.next_event().await?.msg {
                    EventMsg::TurnAborted(_) => break,
                    EventMsg::PatchApplyBegin(_) => panic!("patch executed during cancellation"),
                    EventMsg::TurnComplete(_) => panic!("turn completed instead of aborting"),
                    EventMsg::Error(error) => panic!("translated stream failed: {}", error.message),
                    _ => {}
                }
            }
            anyhow::Ok(())
        })
        .await??;
        release.send(()).expect("EOF gate still held");
        timeout(WAIT, &mut completions[0]).await??;
        timeout(WAIT, test.codex.shutdown_and_wait()).await??;
        assert!(!target.exists(), "cancelled patch created its target");
        timeout(WAIT, server.shutdown()).await?;
    }
    Ok(())
}
