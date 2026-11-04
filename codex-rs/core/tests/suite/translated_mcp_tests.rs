//! Translated provider continuations preserve real MCP calls and results.

use super::*;
use codex_model_provider_info::WireApi;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_mcp_text_result_reaches_next_request() -> anyhow::Result<()> {
    const WAIT: Duration = Duration::from_secs(30);
    const CALL_ID: &str = "translated-mcp-call";
    const SENTINEL: &str = "translated-mcp-echo-sentinel";
    const TOOL: &str = "mcp__rmcp__image_scenario";

    for (protocol, endpoint) in [
        (WireApi::Chat, "/v1/chat/completions"),
        (WireApi::Messages, "/v1/messages"),
    ] {
        let command = remote_aware_stdio_server_bin()?;
        let server = timeout(WAIT, start_mock_server()).await?;
        let counter = AtomicUsize::new(0);
        Mock::given(method("POST"))
            .and(path(endpoint))
            .respond_with(move |_: &wiremock::Request| {
                let index = counter.fetch_add(1, Ordering::SeqCst);
                let first = index == 0;
                let arguments = json!({"scenario":"text_only","caption":SENTINEL}).to_string();
                let frames = match protocol {
                    WireApi::Chat => {
                        let delta = if first {
                            json!({"role":"assistant","tool_calls":[{"index":0,"id":CALL_ID,"type":"function","function":{"name":TOOL,"arguments":arguments}}]})
                        } else {
                            json!({"role":"assistant","content":"done"})
                        };
                        vec![
                            json!({"id":format!("mcp-{index}"),"model":"m","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
                            json!({"id":format!("mcp-{index}"),"model":"m","choices":[{"index":0,"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}]}),
                        ]
                    }
                    WireApi::Messages => {
                        let mut frames = vec![json!({"type":"message_start","message":{"id":format!("mcp-{index}"),"model":"m","role":"assistant","content":[],"usage":{}}})];
                        if first {
                            frames.extend([
                                json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":CALL_ID,"name":TOOL,"input":{}}}),
                                json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                            ]);
                        } else {
                            frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"done"}}));
                        }
                        frames.extend([
                            json!({"type":"content_block_stop","index":0}),
                            json!({"type":"message_delta","delta":{"stop_reason":if first {"tool_use"} else {"end_turn"}},"usage":{}}),
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
            .mount(&server)
            .await;

        let base_url = format!("{}/v1", server.uri());
        let fixture = timeout(
            WAIT,
            test_codex()
                .with_model_info_override("gpt-5.4", |model| model.supports_search_tool = false)
                .with_config(move |config| {
                    config.model_provider_id = "translated-mcp-test".into();
                    config.model_provider.base_url = Some(base_url);
                    config.model_provider.wire_api = protocol;
                    config.model_provider.requires_openai_auth = false;
                    config.model_provider.supports_websockets = false;
                    config.model_provider.stream_max_retries = Some(0);
                    insert_mcp_server(
                        config,
                        "rmcp",
                        stdio_transport(command, /*env*/ None, Vec::new()),
                        TestMcpServerOptions {
                            environment_id: remote_aware_environment_id(),
                            ..Default::default()
                        },
                    );
                })
                .build_with_auto_env(&server),
        )
        .await??;
        timeout(WAIT, wait_for_mcp_server(&fixture.codex, "rmcp")).await??;
        timeout(
            WAIT,
            fixture
                .codex
                .start_or_steer_turn(read_only_user_turn(&fixture, "return the text")),
        )
        .await??;

        let mut began = false;
        let mut ended = false;
        timeout(WAIT, async {
            loop {
                match fixture.codex.next_event().await?.msg {
                    EventMsg::McpToolCallBegin(event) => {
                        assert_eq!(event.call_id, CALL_ID);
                        assert_eq!(event.invocation.server, "rmcp");
                        assert_eq!(event.invocation.tool, "image_scenario");
                        assert_eq!(
                            event.invocation.arguments,
                            Some(json!({"scenario":"text_only","caption":SENTINEL}))
                        );
                        assert!(!began);
                        began = true;
                    }
                    EventMsg::McpToolCallEnd(event) => {
                        assert!(began && !ended);
                        assert_eq!(event.call_id, CALL_ID);
                        assert_eq!(event.invocation.server, "rmcp");
                        assert_eq!(event.invocation.tool, "image_scenario");
                        ended = true;
                    }
                    EventMsg::Error(error) => {
                        anyhow::bail!("translated MCP turn failed: {}", error.message)
                    }
                    EventMsg::TurnAborted(_) => anyhow::bail!("translated MCP turn aborted"),
                    EventMsg::TurnComplete(_) => break,
                    _ => {}
                }
            }
            anyhow::Ok(())
        })
        .await??;
        assert!(began && ended, "MCP call did not complete");

        let requests = server
            .received_requests()
            .await
            .context("captured requests")?;
        let posts: Vec<_> = requests
            .iter()
            .filter(|request| request.method == "POST")
            .collect();
        assert_eq!(posts.len(), 2);
        assert!(posts.iter().all(|request| request.url.path() == endpoint));
        let body: Value = serde_json::from_slice(&posts[1].body)?;
        let messages = body["messages"].as_array().context("continued messages")?;
        let output = match protocol {
            WireApi::Chat => {
                let call = messages
                    .iter()
                    .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
                    .find(|call| call["id"] == CALL_ID)
                    .context("continued Chat call")?;
                assert_eq!(call["function"]["name"], TOOL);
                messages
                    .iter()
                    .find(|message| message["role"] == "tool" && message["tool_call_id"] == CALL_ID)
                    .context("continued Chat result")?["content"]
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
                    .find(|block| block["type"] == "tool_use" && block["id"] == CALL_ID)
                    .context("continued Messages call")?;
                assert_eq!(call["name"], TOOL);
                blocks
                    .iter()
                    .find(|block| block["type"] == "tool_result" && block["tool_use_id"] == CALL_ID)
                    .context("continued Messages result")?["content"]
                    .clone()
            }
            WireApi::Responses => unreachable!("translated protocols only"),
        };
        assert!(
            output.to_string().contains(SENTINEL),
            "missing exact sentinel in {output}"
        );
        timeout(WAIT, fixture.codex.shutdown_and_wait()).await??;
        timeout(WAIT, server.verify()).await?;
    }
    Ok(())
}
