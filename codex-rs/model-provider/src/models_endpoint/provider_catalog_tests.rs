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
    expected.supported_reasoning_levels = vec![ReasoningEffort::None,ReasoningEffort::Low,ReasoningEffort::High]
        .into_iter().map(|effort| ReasoningEffortPreset { description: effort.to_string(), effort }).collect();
    assert_eq!(parse(&serde_json::to_vec(&body).unwrap()).unwrap(),vec![expected]);
}

#[test]
fn sparse_models_do_not_invent_reasoning_or_accept_remote_instructions() {
    let body = json!({"models":[{"id":"sparse","model_messages":{"instructions_template":"untrusted instructions"}}]});
    let mut expected = model_info_from_slug("sparse");
    expected.priority=0;
    expected.visibility=ModelVisibility::List;
    expected.supports_reasoning_summary_parameter=false;
    assert_eq!(parse(&serde_json::to_vec(&body).unwrap()).unwrap(),vec![expected]);
}

#[test]
fn empty_and_invalid_catalogs_remain_distinct_without_response_leakage() {
    assert_eq!(parse(br#"{"data":[]}"#).unwrap(), Vec::<ModelInfo>::new());
    for body in [json!({"data":[{"id":"same"},{"id":"same"}]}),json!({"data":[{"id":""}]}),json!({"secret":"credential-from-provider"})] {
        let error=parse(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        assert!(!format!("{error:?}").contains("credential-from-provider"));
    }
    let error=parse(&serde_json::to_vec(&json!({"data":vec![json!({"id":"x"});MAX_MODELS+1]})).unwrap()).unwrap_err();
    assert!(error.to_string().contains("1024-model limit"));
}
