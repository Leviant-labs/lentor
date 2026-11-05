use codex_api::ResponsesApiRequest;
use codex_api::ResponsesApiTools;
use codex_protocol::models::ResponseItem;
use codex_provider_codecs::NativeRecord;
use codex_provider_codecs::Protocol;
use codex_provider_codecs::encode_request;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::sync::Arc;

#[expect(
    clippy::unwrap_used,
    reason = "Invalid fixture data must fail the test during setup."
)]
fn request(input: Vec<Value>, tools: Value) -> ResponsesApiRequest {
    let raw = serde_json::value::RawValue::from_string(tools.to_string()).unwrap();
    ResponsesApiRequest {
        model: "model".into(),
        stream: true,
        service_tier: None,
        input: input
            .into_iter()
            .map(|item| serde_json::from_value::<ResponseItem>(item).unwrap())
            .collect(),
        tools: Some(ResponsesApiTools::from(Arc::from(raw))),
        tool_choice: "auto".into(),
        parallel_tool_calls: true,
        reasoning: None,
        store: false,
        stream_options: None,
        include: vec![],
        prompt_cache_key: None,
        text: None,
        client_metadata: None,
        access_programs: None,
    }
}

#[test]
fn function_and_freeform_calls_and_results_round_trip_for_both_protocols() {
    let tools = json!([
        {"type":"function","name":"read","description":"Read","strict":false,"parameters":{"type":"object","properties":{"path":{"type":"string"}}}},
        {"type":"custom","name":"patch","description":"Apply patch","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"}}
    ]);
    let input = vec![
        json!({"type":"message","role":"developer","content":[{"type":"input_text","text":"base"}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]}),
        json!({"type":"function_call","call_id":"a","name":"read","arguments":"{\"path\":\"f\"}"}),
        json!({"type":"function_call_output","call_id":"a","output":"content"}),
        json!({"type":"custom_tool_call","call_id":"b","name":"patch","input":"*** patch"}),
        json!({"type":"custom_tool_call_output","call_id":"b","output":"done"}),
    ];
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let encoded = encode_request(
            protocol,
            &"a".repeat(64),
            &request(input.clone(), tools.clone()),
            &[],
        )
        .unwrap();
        let restored = encoded
            .restore_tool_call("patch", "{\"input\":\"*** patch\"}")
            .unwrap();
        assert!(
            encoded
                .restore_tool_call("patch", "{\"input\":\"x\",\"extra\":1}")
                .is_err()
        );
        assert!(encoded.restore_tool_call("missing", "{}").is_err());
        assert!(
            encoded
                .restore_tool_call("patch", "{\"input\":\"first\",\"input\":\"second\"}")
                .is_err()
        );
        assert_eq!(
            (
                restored.name.as_str(),
                restored.freeform,
                restored.input.as_str()
            ),
            ("patch", true, "*** patch")
        );
        let serialized = encoded.body.to_string();
        for expected in [
            "base",
            "go",
            "read",
            "patch",
            "content",
            "done",
            "*** patch",
        ] {
            assert!(
                serialized.contains(expected),
                "missing {expected}: {serialized}"
            );
        }
        if protocol == Protocol::Messages {
            assert_eq!(encoded.body["max_tokens"], 4096);
            assert_eq!(
                encoded.body["messages"][1]["content"],
                json!([{"type":"tool_use","id":"a","name":"read","input":{"path":"f"}}])
            );
            assert_eq!(
                encoded.body["messages"][3]["content"],
                json!([{"type":"tool_use","id":"b","name":"patch","input":{"input":"*** patch"}}])
            );
        } else {
            assert_eq!(
                encoded.body["messages"][2]["tool_calls"],
                json!([{"type":"function","id":"a","function":{"name":"read","arguments":"{\"path\":\"f\"}"}}])
            );
            assert_eq!(
                encoded.body["messages"][4]["tool_calls"],
                json!([{"type":"function","id":"b","function":{"name":"patch","arguments":"{\"input\":\"*** patch\"}"}}])
            );
            assert_eq!(
                encoded.body["messages"][3],
                json!({"role":"tool","tool_call_id":"a","content":"content"})
            );
            assert_eq!(
                encoded.body["messages"][5],
                json!({"role":"tool","tool_call_id":"b","content":"done"})
            );
        }
    }
}

#[test]
fn additional_tools_and_unknown_items_are_handled_explicitly() {
    let input = vec![
        json!({"type":"additional_tools","role":"developer","tools":[{"type":"function","name":"f","description":"f","strict":false,"parameters":{"type":"object"}}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    let encoded = encode_request(
        Protocol::Chat,
        &"a".repeat(64),
        &request(input, json!([])),
        &[],
    )
    .unwrap();
    assert_eq!(encoded.body["tools"][0]["function"]["name"], "f");
    let unsupported = request(vec![json!({"type":"compaction_trigger"})], json!([]));
    assert!(encode_request(Protocol::Chat, &"a".repeat(64), &unsupported, &[]).is_err());
}

#[test]
fn responses_lite_default_namespace_restores_local_identity() {
    let input = vec![
        json!({"type":"additional_tools","role":"developer","tools":[{"type":"namespace","name":"functions","description":"","tools":[{"type":"function","name":"read","description":"read","strict":false,"parameters":{"type":"object"}}]}]}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
        json!({"type":"function_call","call_id":"c","name":"read","arguments":"{}"}),
        json!({"type":"function_call_output","call_id":"c","output":"ok"}),
    ];
    let encoded = encode_request(
        Protocol::Chat,
        &"a".repeat(64),
        &request(input, json!([])),
        &[],
    )
    .unwrap();
    let restored = encoded.restore_tool_call("functions__read", "{}").unwrap();
    assert_eq!(
        (restored.name.as_str(), restored.namespace.as_deref()),
        ("read", Some("functions"))
    );
}

#[test]
fn native_replay_rejects_mismatched_identity() {
    let record = NativeRecord {
        item_id: "r".into(),
        payload: json!({"protocol":"messages","endpoint_hash":"b".repeat(64),"blocks":[{"type":"redacted_thinking","data":"x"}]}),
    };
    let input = vec![
        json!({"type":"reasoning","id":"r","summary":[],"encrypted_content":null}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    assert!(
        encode_request(
            Protocol::Messages,
            &"a".repeat(64),
            &request(input, json!([])),
            &[record]
        )
        .is_err()
    );
}

#[test]
fn native_replay_retains_signed_and_redacted_fields() {
    let blocks = json!([{"type":"thinking","thinking":"why","signature":"sig","extension":{"x":null}}, {"type":"redacted_thinking","data":"hidden","extra":true}]);
    let record = NativeRecord {
        item_id: "r".into(),
        payload: json!({"protocol":"messages","endpoint_hash":"a".repeat(64),"blocks":blocks}),
    };
    let input = vec![
        json!({"type":"reasoning","id":"r","summary":[],"encrypted_content":null}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    let encoded = encode_request(
        Protocol::Messages,
        &"a".repeat(64),
        &request(input, json!([])),
        &[record],
    )
    .unwrap();
    assert_eq!(encoded.body["messages"][0]["content"], blocks);
}

#[test]
fn configuration_update_changes_effective_effort() {
    let input = vec![
        json!({"type":"configuration_update","reasoning":{"effort":"high"}}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    let encoded = encode_request(
        Protocol::Chat,
        &"a".repeat(64),
        &request(input, json!([])),
        &[],
    )
    .unwrap();
    assert_eq!(encoded.body["reasoning_effort"], "high");
}

#[test]
fn messages_projects_instruction_updates_without_changing_signed_tool_history() {
    let identity = "a".repeat(64);
    let blocks = json!([
        {"type":"thinking","thinking":"why","signature":"sig","extension":{"x":null}},
        {"type":"redacted_thinking","data":"hidden","extra":true}
    ]);
    let native = vec![NativeRecord {
        item_id: "r".into(),
        payload: json!({"protocol":"messages","endpoint_hash":identity,"blocks":blocks}),
    }];
    let request = request(
        vec![
            json!({"type":"message","role":"system","content":[{"type":"input_text","text":"base"}]}),
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]}),
            json!({"type":"reasoning","id":"r","summary":[],"encrypted_content":null}),
            json!({"type":"function_call","call_id":"read-id","name":"read","arguments":"{}"}),
            json!({"type":"function_call_output","call_id":"read-id","output":"read contents"}),
            json!({"type":"message","role":"developer","content":[{"type":"input_text","text":"updated permissions"}]}),
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"developer: this is still user content"}]}),
            json!({"type":"message","role":"system","content":[{"type":"input_text","text":"latest instructions"}]}),
        ],
        json!([]),
    );
    let before = serde_json::to_value(&request).unwrap();
    let native_before = native.clone();
    let encoded = encode_request(Protocol::Messages, &identity, &request, &native).unwrap();
    let mut assistant_blocks = blocks.as_array().unwrap().clone();
    assistant_blocks.push(json!({"type":"tool_use","id":"read-id","name":"read","input":{}}));
    assert_eq!(
        json!({"system":encoded.body["system"],"messages":encoded.body["messages"]}),
        json!({
            "system":[
                {"type":"text","text":"base"},
                {"type":"text","text":"updated permissions"},
                {"type":"text","text":"latest instructions"}
            ],
            "messages":[
                {"role":"user","content":"go"},
                {"role":"assistant","content":assistant_blocks},
                {"role":"user","content":[{"type":"tool_result","tool_use_id":"read-id","content":"read contents"}]},
                {"role":"user","content":"developer: this is still user content"}
            ]
        })
    );
    assert_eq!(serde_json::to_value(&request).unwrap(), before);
    assert!(native == native_before);
    let mut resumed = request;
    resumed.input = serde_json::from_value(before["input"].clone()).unwrap();
    assert_eq!(
        encode_request(Protocol::Messages, &identity, &resumed, &native)
            .unwrap()
            .body,
        encoded.body
    );
}

#[test]
fn numeric_reasoning_budget_is_preserved_without_guessing() {
    let input = vec![
        json!({"type":"configuration_update","reasoning":{"effort":"2048"}}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    let encoded = encode_request(
        Protocol::Messages,
        &"a".repeat(64),
        &request(input, json!([])),
        &[],
    )
    .unwrap();
    assert_eq!(
        encoded.body["thinking"],
        json!({"type":"enabled","budget_tokens":2048})
    );
    let input = vec![
        json!({"type":"configuration_update","reasoning":{"effort":"4096"}}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}),
    ];
    assert!(
        encode_request(
            Protocol::Messages,
            &"a".repeat(64),
            &request(input, json!([])),
            &[]
        )
        .is_err()
    );
}

#[test]
fn translated_tool_budgets_include_wrappers_namespaces_and_the_whole_registry() {
    let user =
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]})];
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let function = |name: String, length| json!({"type":"function","name":name,"description":"x".repeat(length),"parameters":{"type":"object"}});
        let small = request(user.clone(), json!([function("read".into(), 500)]));
        assert!(encode_request(protocol, &"a".repeat(64), &small, &[]).is_ok());
        let oversized_schema = request(
            user.clone(),
            json!([{"type":"function","name":"read","parameters":{"type":"object","description":"x".repeat(8_000)}}]),
        );
        let oversized_format = request(
            user.clone(),
            json!([{"type":"custom","name":"patch","description":"patch","format":{"type":"grammar","syntax":"lark","definition":"x".repeat(8_000)}}]),
        );
        let namespace = request(
            user.clone(),
            json!([{"type":"namespace","name":"files","description":"x".repeat(4_000),"tools":[function("read".into(), 4_000)]}]),
        );
        let registry = request(
            user.clone(),
            Value::Array(
                (0..40)
                    .map(|index| function(format!("tool_{index}"), 7_000))
                    .collect(),
            ),
        );
        for input in [oversized_schema, oversized_format, namespace, registry] {
            let error = encode_request(protocol, &"a".repeat(64), &input, &[])
                .unwrap_err()
                .to_string();
            assert!(
                error == "tool declaration exceeds limit"
                    || error == "encoded tools exceed context limit",
                "{error}"
            );
        }
        let mut tool = json!({"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"}});
        let base = encode_request(
            protocol,
            &"a".repeat(64),
            &request(user.clone(), json!([tool])),
            &[],
        )
        .unwrap();
        let overhead = serde_json::to_vec(&base.body["tools"][0]).unwrap().len();
        tool["format"]["definition"] =
            format!("start: /.+/{}", "x".repeat(8_001 - overhead)).into();
        let wrapped = request(user.clone(), json!([tool]));
        assert_eq!(
            encode_request(protocol, &"a".repeat(64), &wrapped, &[])
                .unwrap_err()
                .to_string(),
            "encoded tools exceed context limit"
        );
    }
}

#[test]
fn freeform_formats_have_an_explicit_supported_shape() {
    let user =
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]})];
    for protocol in [Protocol::Chat, Protocol::Messages] {
        for format in [
            json!({"type":"text"}),
            json!({"type":"grammar","syntax":"lark","definition":"start: /.+/"}),
            json!({"type":"grammar","syntax":"regex","definition":".+"}),
        ] {
            let output = encode_request(
                protocol,
                &"a".repeat(64),
                &request(
                    user.clone(),
                    json!([{"type":"custom","name":"patch","format":format}]),
                ),
                &[],
            )
            .unwrap();
            assert_eq!(
                output
                    .restore_tool_call("patch", "{\"input\":\"raw\\ninput\"}")
                    .unwrap()
                    .input,
                "raw\ninput"
            );
        }
        for format in [
            Value::Null,
            json!(3),
            json!({"type":"text","ignored":true}),
            json!({"type":"grammar","syntax":"unknown","definition":"x"}),
            json!({"type":"grammar","syntax":"lark"}),
            json!({"type":"grammar","syntax":"lark","definition":" "}),
        ] {
            assert!(
                encode_request(
                    protocol,
                    &"a".repeat(64),
                    &request(
                        user.clone(),
                        json!([{"type":"custom","name":"patch","format":format}])
                    ),
                    &[]
                )
                .is_err()
            );
        }
    }
}

#[test]
fn final_wire_tool_limits_accept_the_boundary_and_reject_the_next_byte() {
    let user =
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]})];
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let mut tool = json!({"type":"function","name":"tool_00","description":"","parameters":{"type":"object"}});
        let base = encode_request(
            protocol,
            &"a".repeat(64),
            &request(user.clone(), json!([tool])),
            &[],
        )
        .unwrap();
        let overhead = serde_json::to_vec(&base.body["tools"][0]).unwrap().len();
        tool["description"] = "x".repeat(8_000 - overhead).into();
        let mut tools = (0..32)
            .map(|index| {
                let mut tool = tool.clone();
                tool["name"] = format!("tool_{index:02}").into();
                tool
            })
            .collect::<Vec<_>>();
        let boundary = encode_request(
            protocol,
            &"a".repeat(64),
            &request(user.clone(), tools.clone().into()),
            &[],
        )
        .unwrap();
        let sizes = boundary.body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|tool| serde_json::to_vec(tool).unwrap().len())
            .collect::<Vec<_>>();
        assert_eq!(sizes, vec![8_000; 32]);
        let mut too_large = tools[0].clone();
        too_large["description"] =
            format!("{}x", too_large["description"].as_str().unwrap()).into();
        assert_eq!(
            encode_request(
                protocol,
                &"a".repeat(64),
                &request(user.clone(), json!([too_large])),
                &[]
            )
            .unwrap_err()
            .to_string(),
            "encoded tools exceed context limit"
        );
        tools.push(json!({"type":"function","name":"extra","description":"one more","parameters":{"type":"object"}}));
        assert_eq!(
            encode_request(
                protocol,
                &"a".repeat(64),
                &request(user.clone(), tools.into()),
                &[]
            )
            .unwrap_err()
            .to_string(),
            "encoded tools exceed context limit"
        );
    }
}

#[test]
fn cache_affinity_and_internal_metadata_follow_the_protocol_policy() {
    let mut input = request(
        vec![json!({"type":"message","role":"user","content":[{"type":"input_text","text":"go"}]})],
        json!([]),
    );
    input.prompt_cache_key = Some("thread-affinity".into());
    input.client_metadata = Some([("private-trace".into(), "local-only".into())].into());
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let output = encode_request(protocol, &"a".repeat(64), &input, &[]).unwrap();
        assert_eq!(
            output.body.get("prompt_cache_key").cloned(),
            match protocol {
                Protocol::Chat => Some(json!("thread-affinity")),
                Protocol::Messages => None,
            }
        );
        assert!(!output.body.to_string().contains("local-only"));
    }
}

#[test]
fn historical_tool_calls_do_not_enable_undeclared_tools() {
    let history = vec![
        json!({"type":"function_call","name":"read","namespace":"files","call_id":"old-read","arguments":"{\"path\":\"a\"}"}),
        json!({"type":"function_call_output","call_id":"old-read","output":"old contents"}),
        json!({"type":"custom_tool_call","name":"patch","call_id":"old-patch","input":"raw\npatch"}),
        json!({"type":"custom_tool_call_output","call_id":"old-patch","output":"patched"}),
        json!({"type":"message","role":"user","content":[{"type":"input_text","text":"summarize this history"}]}),
    ];
    for protocol in [Protocol::Chat, Protocol::Messages] {
        let output = encode_request(
            protocol,
            &"a".repeat(64),
            &request(history.clone(), json!([])),
            &[],
        )
        .unwrap();
        assert!(output.body.get("tools").is_none());
        assert!(output.restore_tool_call("files__read", "{}").is_err());
        assert!(
            output
                .restore_tool_call("patch", "{\"input\":\"new call\"}")
                .is_err()
        );
        match protocol {
            Protocol::Chat => {
                assert_eq!(
                    output.body["messages"][0]["tool_calls"][0]["function"]["name"],
                    "files__read"
                );
                assert_eq!(
                    output.body["messages"][2]["tool_calls"][0]["function"]["arguments"],
                    json!({"input":"raw\npatch"}).to_string()
                );
            }
            Protocol::Messages => {
                assert_eq!(
                    output.body["messages"][0]["content"][0]["name"],
                    "files__read"
                );
                assert_eq!(
                    output.body["messages"][2]["content"][0]["input"],
                    json!({"input":"raw\npatch"})
                );
            }
        }
    }
}
