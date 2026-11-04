//! Public RPC lifecycle coverage with an isolated process-local keyring.

use super::*;
use codex_app_server::in_process::InProcessServerEvent;
use codex_app_server_protocol::ServerNotification;
use codex_app_server_protocol::TurnStatus;
use pretty_assertions::assert_eq;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio::time::timeout;

const WAIT: Duration = Duration::from_secs(45);
const CALL_ID: &str = "lifecycle-plan-call";

fn response_stream(protocol: &str, index: usize, model: &str) -> String {
    let first = index == 0;
    let arguments = json!({"plan":[{"step":"Verify","status":"completed"}]}).to_string();
    let id = format!("response-{index}");
    let frames = match protocol {
        "responses" => vec![
            json!({"type":"response.created","response":{"id":id}}),
            if first {
                json!({"type":"response.output_item.done","item":{"type":"function_call","call_id":CALL_ID,"name":"update_plan","arguments":arguments}})
            } else {
                json!({"type":"response.output_item.done","item":{"type":"message","role":"assistant","id":format!("msg_{index}"),"content":[{"type":"output_text","text":"Verify complete"}]}})
            },
            json!({"type":"response.completed","response":{"id":id,"usage":{"input_tokens":0,"input_tokens_details":null,"output_tokens":0,"output_tokens_details":null,"total_tokens":0}}}),
        ],
        "chat" => vec![
            json!({"id":id,"model":model,"choices":[{"index":0,"delta":if first {
                json!({"role":"assistant","tool_calls":[{"index":0,"id":CALL_ID,"type":"function","function":{"name":"update_plan","arguments":arguments}}]})
            } else { json!({"role":"assistant","content":"Verify complete"}) },"finish_reason":null}]}),
            json!({"id":id,"model":model,"choices":[{"index":0,"delta":{},"finish_reason":if first {"tool_calls"} else {"stop"}}]}),
        ],
        "messages" => {
            let mut frames = vec![
                json!({"type":"message_start","message":{"id":id,"model":model,"role":"assistant","content":[],"usage":{}}}),
            ];
            if first {
                frames.extend([
                    json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":CALL_ID,"name":"update_plan","input":{}}}),
                    json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":arguments}}),
                ]);
            } else {
                frames.push(json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"Verify complete"}}));
            }
            frames.extend([
                json!({"type":"content_block_stop","index":0}),
                json!({"type":"message_delta","delta":{"stop_reason":if first {"tool_use"} else {"end_turn"}},"usage":{}}),
                json!({"type":"message_stop"}),
            ]);
            frames
        }
        _ => unreachable!(),
    };
    let mut body: String = frames
        .into_iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect();
    if protocol == "chat" {
        body.push_str("data: [DONE]\n\n");
    }
    body
}

async fn turn(client: &mut InProcessClientHandle, thread: &str, id: i64) -> Result<()> {
    let started = request(
        client,
        id,
        "turn/start",
        json!({
            "threadId":thread,
            "input":[{"type":"text","text":"Verify the plan and respond.","text_elements":[]}]
        }),
    )
    .await?
    .expect("turn starts");
    let turn_id = started["turn"]["id"].as_str().expect("turn id").to_owned();
    timeout(WAIT, async {
        loop {
            if let InProcessServerEvent::ServerNotification(notification) =
                client.next_event().await.expect("event stream open")
                && let ServerNotification::TurnCompleted(completed) = notification.as_ref()
                && completed.thread_id == thread
                && completed.turn.id == turn_id
            {
                assert_eq!(
                    completed.turn.status,
                    TurnStatus::Completed,
                    "turn error: {:?}",
                    completed.turn.error
                );
                break;
            }
        }
    })
    .await?;
    Ok(())
}

fn assert_plan_history(protocol: &str, body: &Value) {
    match protocol {
        "responses" => {
            let items = body["input"].as_array().expect("response input");
            assert!(items.iter().any(|item| item["type"] == "function_call"
                && item["call_id"] == CALL_ID
                && item["name"] == "update_plan"));
            assert!(
                items
                    .iter()
                    .any(|item| item["type"] == "function_call_output"
                        && item["call_id"] == CALL_ID
                        && item["output"].to_string().contains("Plan updated"))
            );
        }
        "chat" => {
            let messages = body["messages"].as_array().expect("chat messages");
            assert!(
                messages
                    .iter()
                    .flat_map(|message| message["tool_calls"].as_array().into_iter().flatten())
                    .any(|call| call["id"] == CALL_ID && call["function"]["name"] == "update_plan")
            );
            assert!(messages.iter().any(|message| message["role"] == "tool"
                && message["tool_call_id"] == CALL_ID
                && message["content"].to_string().contains("Plan updated")));
        }
        "messages" => {
            let blocks: Vec<_> = body["messages"]
                .as_array()
                .expect("messages")
                .iter()
                .filter_map(|message| message["content"].as_array())
                .flatten()
                .collect();
            assert!(blocks.iter().any(|block| block["type"] == "tool_use"
                && block["id"] == CALL_ID
                && block["name"] == "update_plan"));
            assert!(blocks.iter().any(|block| block["type"] == "tool_result"
                && block["tool_use_id"] == CALL_ID
                && block["content"].to_string().contains("Plan updated")));
        }
        _ => unreachable!(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn translated_provider_lifecycle() -> Result<()> {
    const CHILD: &str = "LENTOR_TRANSLATED_LIFECYCLE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = timeout(WAIT * 4, Command::new(std::env::current_exe()?)
            .args(["--exact", "suite::v2::model_provider_setup::keyring_tests::translated_provider_lifecycle_tests::translated_provider_lifecycle", "--nocapture"])
            .env(CHILD, "1")
            .env_remove("VercelAPI")
            .env_remove("OPENAI_API_KEY")
            .env_remove("LENTOR_API_KEY")
            .output()).await??;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(
            stdout.contains("test result: ok. 1 passed; 0 failed")
                && stdout.contains("translated-lifecycle-child-executed"),
            "child did not execute exactly one test: {stdout}\n{stderr}"
        );
        for protocol in ["responses", "chat", "messages"] {
            for status in [200, 404] {
                assert!(
                    stdout.contains(&format!("lifecycle-passed {protocol} discovery={status}")),
                    "missing completed lifecycle scenario: {stdout}\n{stderr}"
                );
            }
        }
        return Ok(());
    }
    println!("translated-lifecycle-child-executed");
    keyring::set_default_credential_builder(Box::new(TestKeyring(Arc::default())));
    for (model, discovery_status) in [("vendor/exact", 200), ("Vendor/Exact-Model:Alpha", 404)] {
        for (protocol, endpoint) in [
            ("responses", "/v1/responses"),
            ("chat", "/v1/chat/completions"),
            ("messages", "/v1/messages"),
        ] {
            let server = timeout(WAIT, MockServer::start()).await?;
            Mock::given(method("GET"))
                .and(path("/v1/models"))
                .respond_with(
                    ResponseTemplate::new(discovery_status)
                        .set_body_json(json!({"data":[{"id":model}]})),
                )
                .mount(&server)
                .await;
            let counter = AtomicUsize::new(0);
            Mock::given(method("POST"))
                .and(path(endpoint))
                .respond_with(move |_: &wiremock::Request| {
                    let index = counter.fetch_add(1, Ordering::SeqCst);
                    ResponseTemplate::new(200)
                        .insert_header("content-type", "text/event-stream")
                        .set_body_string(response_stream(protocol, index, model))
                })
                .mount(&server)
                .await;
            let home = TempDir::new()?;
            std::fs::write(
                home.path().join("config.toml"),
                "[features]\nplugins = false\nshell_tool = false\n[tools.update_plan]\nenabled = true\n",
            )?;
            let mut client = timeout(WAIT, start(&home, "")).await??;
            let base = format!("{}/v1", server.uri());
            let mut configure = params(&base);
            configure["wireApi"] = json!(protocol);
            configure["model"] = json!(model);
            let configured = timeout(
                WAIT,
                request(&client, 1, "modelProvider/configure", configure),
            )
            .await??
            .expect("configure");
            assert_eq!(configured["model"], model);
            let saved: toml::Value =
                toml::from_str(&std::fs::read_to_string(home.path().join("config.toml"))?)?;
            assert_eq!(
                saved["model_providers"]["custom"]["wire_api"].as_str(),
                Some(protocol)
            );
            let models = timeout(
                WAIT,
                request(
                    &client,
                    2,
                    "model/list",
                    json!({"limit":200,"includeHidden":true}),
                ),
            )
            .await??;
            if discovery_status == 200 {
                let models = models.expect("model list");
                assert!(
                    models["data"]
                        .as_array()
                        .expect("models")
                        .iter()
                        .any(|entry| entry["id"] == model)
                );
            } else {
                let error = models.expect_err("discovery is unavailable");
                assert!(!error.message.contains("synthetic-provider-key"));
            }
            let started = timeout(
                WAIT,
                request(
                    &client,
                    3,
                    "thread/start",
                    json!({
                        "model":model,"modelProvider":"custom","cwd":home.path(),
                        "approvalPolicy":"never","sandbox":"read-only","ephemeral":false
                    }),
                ),
            )
            .await??
            .expect("thread start");
            assert_eq!(started["model"], model);
            assert_eq!(started["modelProvider"], "custom");
            let thread = started["thread"]["id"]
                .as_str()
                .expect("thread id")
                .to_owned();
            timeout(WAIT, turn(&mut client, &thread, 4)).await??;
            timeout(WAIT, client.shutdown()).await??;
            let mut client = timeout(WAIT, start(&home, "")).await??;
            let resumed = timeout(
                WAIT,
                request(&client, 5, "thread/resume", json!({"threadId":thread})),
            )
            .await??
            .expect("resume");
            assert_eq!(resumed["thread"]["id"], thread);
            assert_eq!(resumed["model"], model);
            assert_eq!(resumed["modelProvider"], "custom");
            timeout(WAIT, turn(&mut client, &thread, 6)).await??;
            timeout(WAIT, client.shutdown()).await??;
            let requests = server.received_requests().await.expect("captured requests");
            let posts: Vec<_> = requests
                .iter()
                .filter(|request| request.method == "POST")
                .collect();
            assert_eq!(posts.len(), 3, "{protocol}");
            for post in &posts {
                assert_eq!(post.url.path(), endpoint);
                let body: Value = serde_json::from_slice(&post.body)?;
                assert_eq!(body["model"], model);
            }
            for post in &posts[1..] {
                assert_plan_history(protocol, &serde_json::from_slice(&post.body)?);
            }
            timeout(WAIT, server.verify()).await?;
            println!("lifecycle-passed {protocol} discovery={discovery_status}");
        }
    }
    Ok(())
}
