use super::*;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn sparse_catalog_preserves_ids_and_advertised_reasoning() {
    let body = json!({"data":[{
        "id":"vendor/model", "name":"Discovered model", "context_window":1048576,
        "reasoning_options":[{"type":"toggle"},{"type":"effort","values":["none","low","high"]}]
    }]});
    let mut expected = model_info_from_slug("vendor/model");
    expected.display_name = "Discovered model".to_string();
    expected.priority = 0;
    expected.visibility = ModelVisibility::List;
    expected.supports_reasoning_summary_parameter = false;
    expected.context_window = Some(1048576);
    expected.max_context_window = Some(1048576);
    expected.default_reasoning_level = Some(ReasoningEffort::None);
    expected.supported_reasoning_levels = vec![
        ReasoningEffort::None,
        ReasoningEffort::Low,
        ReasoningEffort::High,
    ]
    .into_iter()
    .map(|effort| ReasoningEffortPreset {
        description: effort.to_string(),
        effort,
    })
    .collect();
    assert_eq!(
        parse(&serde_json::to_vec(&body).unwrap()).unwrap(),
        vec![expected]
    );
}

#[test]
fn sparse_models_do_not_invent_reasoning_or_accept_remote_instructions() {
    let body = json!({"models":[{"id":"sparse","model_messages":{"instructions_template":"untrusted instructions"}}]});
    let mut expected = model_info_from_slug("sparse");
    expected.priority = 0;
    expected.visibility = ModelVisibility::List;
    expected.supports_reasoning_summary_parameter = false;
    assert_eq!(
        parse(&serde_json::to_vec(&body).unwrap()).unwrap(),
        vec![expected]
    );
}

#[test]
fn empty_and_invalid_catalogs_remain_distinct_without_response_leakage() {
    assert_eq!(parse(br#"{"data":[]}"#).unwrap(), Vec::<ModelInfo>::new());
    for body in [
        json!({"data":[{"id":"same"},{"id":"same"}]}),
        json!({"data":[{"id":""}]}),
        json!({"secret":"credential-from-provider"}),
    ] {
        let error = parse(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert!(!format!("{error:?}").contains("credential-from-provider"));
    }
    let error =
        parse(&serde_json::to_vec(&json!({"data":vec![json!({"id":"x"});MAX_MODELS+1]})).unwrap())
            .unwrap_err();
    assert!(error.to_string().contains("1024-model limit"));
}

#[test]
fn generic_full_rows_ignore_instructions_policy_and_unlisted_fields() {
    let mut row = serde_json::to_value(model_info_from_slug("vendor/full")).unwrap();
    row["display_name"] = json!("Bounded name");
    row["description"] = json!("x".repeat(2049));
    row["model_messages"] = json!({"instructions_template": "untrusted instructions"});
    row["guardian"] = json!({});
    row["supports_search_tool"] = json!(true);
    row["multi_agent"] = json!({"untrusted": "policy"});
    // Preserve only the whitelisted reasoning levels, not remote policy.
    let baseline = json!({"data":[{"id":"vendor/full", "name":"Bounded name", "supported_reasoning_levels":row["supported_reasoning_levels"], "default_reasoning_level":row["default_reasoning_level"], "context_window":row["context_window"]}]});
    let expected = parse(&serde_json::to_vec(&baseline).unwrap()).unwrap();
    assert_eq!(
        parse(&serde_json::to_vec(&json!({"data":[row]})).unwrap()).unwrap(),
        expected
    );
}

#[test]
fn generic_rows_prefer_openai_id_and_allow_slug_fallback() {
    let preferred = parse(br#"{"data":[{"id":"openai/model","slug":"native/model"}]}"#).unwrap();
    assert_eq!(preferred[0].slug, "openai/model");

    let fallback = parse(br#"{"data":[{"slug":"native/model"}]}"#).unwrap();
    assert_eq!(fallback[0].slug, "native/model");
}

#[test]
fn generic_efforts_are_bounded_and_default_is_advertised() {
    for levels in [
        json!(["x".repeat(33)]),
        json!(["bad\nvalue"]),
        json!((0..17).map(|i| format!("effort-{i}")).collect::<Vec<_>>()),
    ] {
        assert!(
            parse(
                &serde_json::to_vec(
                    &json!({"data":[{"id":"test", "supported_reasoning_levels":levels}]})
                )
                .unwrap()
            )
            .is_err()
        );
    }
    let options = json!({"data":[{"id":"test", "reasoning_options":[{"type":"effort","values":(0..16).map(|i| format!("a{i}")).collect::<Vec<_>>()},{"type":"effort","values":["overflow"]}]}]});
    assert!(parse(&serde_json::to_vec(&options).unwrap()).is_err());
    let body = json!({"data":[{"id":"test", "supported_reasoning_levels":[{"effort":"low", "description":"x".repeat(257)}], "default_reasoning_level":"unknown"}]});
    let models = parse(&serde_json::to_vec(&body).unwrap()).unwrap();
    assert_eq!(
        models[0].default_reasoning_level,
        Some(ReasoningEffort::Low)
    );
    assert_eq!(
        models[0].supported_reasoning_levels,
        vec![ReasoningEffortPreset {
            effort: ReasoningEffort::Low,
            description: "low".to_string()
        }]
    );
}

#[test]
fn generic_context_window_is_capped() {
    let body = json!({"data":[{"id":"test", "context_window":i64::MAX}]});
    let model = parse(&serde_json::to_vec(&body).unwrap())
        .unwrap()
        .remove(0);
    let fallback = model_info_from_slug("test");
    assert_eq!(
        (model.context_window, model.max_context_window),
        (fallback.context_window, fallback.max_context_window)
    );
    let body = json!({"data":[{"id":"test", "context_window":100_000_000}]});
    let model = parse(&serde_json::to_vec(&body).unwrap())
        .unwrap()
        .remove(0);
    assert_eq!(
        (model.context_window, model.max_context_window),
        (Some(100_000_000), Some(100_000_000))
    );
}

#[test]
fn native_catalog_preserves_full_model_instructions() {
    let mut model = model_info_from_slug("native-model");
    model.used_fallback_model_metadata = false;
    model
        .model_messages
        .get_or_insert_default()
        .instructions_template = Some("trusted catalog instructions".to_string());
    let bytes = serde_json::to_vec(&codex_protocol::openai_models::ModelsResponse {
        models: vec![model.clone()],
    })
    .unwrap();
    assert_eq!(parse_codex(&bytes).unwrap(), vec![model]);
}
