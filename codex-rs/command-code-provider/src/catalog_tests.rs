use codex_protocol::provider::CommandCodeRoute;
use pretty_assertions::assert_eq;
use serde_json::json;

use crate::catalog::CatalogError;
use crate::catalog::CommandCodeCatalog;
use crate::catalog::MAX_MODELS;

const COMPLETE_CATALOG: &[u8] = br#"
{
  "models": [
    {
      "id": "commandcode-pro",
      "display_name": "CommandCode Pro",
      "supported_endpoints": ["messages", "chat/completions", "responses"]
    },
    {
      "id": "commandcode-chat",
      "supported_endpoints": ["chat/completions"]
    },
    {
      "id": "commandcode-anthropic",
      "supported_endpoints": ["messages"]
    }
  ]
}
"#;

const SPARSE_CATALOG: &[u8] = br#"
{
  "data": [
    {"id": "bare-model"},
    {"id": "named-model", "name": "Named Model", "unknown_field": true},
    {"id": "object-endpoints", "supportedEndpoints": [{"endpoint": "responses"}]},
    {"id": "string-endpoint", "endpoints": "chat/completions"},
    {"id": "unknown-endpoint", "supported_endpoints": ["embeddings"]}
  ]
}
"#;

#[test]
fn complete_catalog_normalizes_routes_in_precedence_order() {
    let catalog = CommandCodeCatalog::parse(COMPLETE_CATALOG).expect("catalog should parse");

    assert_eq!(catalog.len(), 3);
    let model = catalog
        .model("commandcode-pro")
        .expect("model should be present");
    assert_eq!(model.id(), "commandcode-pro");
    assert_eq!(model.display_name(), Some("CommandCode Pro"));
    assert_eq!(
        model.routes(),
        &[
            CommandCodeRoute::Responses,
            CommandCodeRoute::ChatCompletions,
            CommandCodeRoute::AnthropicMessages,
        ]
    );
    assert_eq!(model.select_route(), Ok(CommandCodeRoute::Responses));
    assert_eq!(
        catalog
            .model("commandcode-chat")
            .expect("model should be present")
            .select_route(),
        Ok(CommandCodeRoute::ChatCompletions)
    );
    assert_eq!(
        catalog
            .model("commandcode-anthropic")
            .expect("model should be present")
            .select_route(),
        Ok(CommandCodeRoute::AnthropicMessages)
    );
}

#[test]
fn sparse_catalog_accepts_missing_metadata_and_endpoint_shapes() {
    let catalog = CommandCodeCatalog::parse(SPARSE_CATALOG).expect("catalog should parse");

    assert_eq!(catalog.len(), 5);
    let bare = catalog
        .model("bare-model")
        .expect("model should be present");
    assert_eq!(bare.display_name(), None);
    assert_eq!(bare.routes(), &[]);
    let error = bare.select_route().expect_err("missing route should fail");
    assert_eq!(error.model(), "bare-model");
    assert_eq!(error.supported_endpoints(), &[] as &[String]);

    let named = catalog
        .model("named-model")
        .expect("model should be present");
    assert_eq!(named.display_name(), Some("Named Model"));
    assert_eq!(
        catalog
            .model("object-endpoints")
            .expect("model should be present")
            .select_route(),
        Ok(CommandCodeRoute::Responses)
    );
    assert_eq!(
        catalog
            .model("string-endpoint")
            .expect("model should be present")
            .select_route(),
        Ok(CommandCodeRoute::ChatCompletions)
    );
    assert_eq!(
        catalog
            .model("unknown-endpoint")
            .expect("model should be present")
            .supported_endpoints(),
        &["embeddings".to_string()]
    );
}

#[test]
fn duplicate_routes_are_ignored() {
    let catalog = CommandCodeCatalog::parse(
        br#"{"models":[{"id":"duplicate","supported_endpoints":["responses","responses"]}]}"#,
    )
    .expect("catalog should parse");

    assert_eq!(
        catalog
            .model("duplicate")
            .expect("model should be present")
            .routes(),
        &[CommandCodeRoute::Responses]
    );
}

#[test]
fn unsupported_route_names_model_and_endpoints() {
    let catalog = CommandCodeCatalog::parse(
        br#"{"models":[{"id":"embedding-only","supported_endpoints":["embeddings","moderations"]}]}"#,
    )
    .expect("catalog should parse");
    let error = catalog
        .model("embedding-only")
        .expect("model should be present")
        .select_route()
        .expect_err("unsupported route should fail");

    assert_eq!(error.model(), "embedding-only");
    assert_eq!(
        error.supported_endpoints(),
        &["embeddings".to_string(), "moderations".to_string()]
    );
    assert_eq!(
        error.to_string(),
        "model `embedding-only` does not support a Codex request route; supported endpoints: embeddings, moderations"
    );
}

#[test]
fn empty_route_names_model_and_missing_endpoints() {
    let catalog = CommandCodeCatalog::parse(br#"{"data":[{"id":"empty-routes"}]}"#)
        .expect("catalog should parse");
    let error = catalog
        .model("empty-routes")
        .expect("model should be present")
        .select_route()
        .expect_err("empty route should fail");

    assert_eq!(
        error.to_string(),
        "model `empty-routes` does not support a Codex request route; supported endpoints: none"
    );
}

#[test]
fn model_count_is_capped() {
    let models = (0..=MAX_MODELS)
        .map(|index| json!({"id": format!("model-{index}")}))
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&json!({"models": models})).expect("fixture should encode");

    assert_eq!(
        CommandCodeCatalog::parse(&bytes),
        Err(CatalogError::TooManyModels {
            count: MAX_MODELS + 1,
            limit: MAX_MODELS,
        })
    );
}

#[test]
fn malformed_json_is_rejected_without_response_contents() {
    let error =
        CommandCodeCatalog::parse(br#"{"models": [}"#).expect_err("malformed catalog should fail");
    assert!(matches!(error, CatalogError::InvalidJson(_)));
}

#[test]
fn missing_model_array_is_rejected() {
    assert_eq!(
        CommandCodeCatalog::parse(br#"{"object":"list"}"#),
        Err(CatalogError::MissingModelsArray)
    );
}

#[test]
fn duplicate_model_ids_are_rejected() {
    assert_eq!(
        CommandCodeCatalog::parse(br#"{"models":[{"id":"same"},{"id":"same"}]}"#),
        Err(CatalogError::DuplicateModel {
            id: "same".to_string(),
        })
    );
}
