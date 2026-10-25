//! Tests for the CommandCode model-provider boundary.

use std::sync::Arc;

use codex_protocol::provider::CommandCodeRoute;
use pretty_assertions::assert_eq;
use serde_json::json;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header_regex;
use wiremock::matchers::method;
use wiremock::matchers::path;

use super::*;
use crate::create_model_provider;

const CREDENTIAL: &str = "super-secret";

fn boundary_with_base_url(base_url: Option<String>) -> Arc<CommandCodeProviderBoundary> {
    let mut info = ModelProviderInfo::create_command_code_provider();
    info.base_url = base_url;
    create_model_provider(info, /*auth_manager*/ None)
        .command_code_boundary()
        .expect("CommandCode provider should expose a discovery boundary")
}

fn provider_base_url(server: &MockServer) -> String {
    format!("{}/provider/v1", server.uri())
}

#[test]
fn command_code_provider_starts_without_catalog_or_selection() {
    let boundary = boundary_with_base_url(/*base_url*/ None);

    assert_eq!(boundary.catalog_state(), CatalogState::NotLoaded);
    assert_eq!(boundary.selection(), None);
    assert_eq!(boundary.selected_endpoint(), None);
    assert_eq!(
        boundary.status(),
        CommandCodeProviderStatus {
            provider_id: COMMAND_CODE_PROVIDER_ID.to_string(),
            model: None,
            route: None,
            catalog: CatalogState::NotLoaded,
        }
    );
}

#[test]
fn other_providers_do_not_expose_a_discovery_boundary() {
    let custom = ModelProviderInfo {
        name: "Custom".to_string(),
        base_url: Some("http://localhost:1234/v1".to_string()),
        ..ModelProviderInfo::default()
    };

    for info in [
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        custom,
    ] {
        let provider = create_model_provider(info, /*auth_manager*/ None);

        assert!(
            provider.command_code_boundary().is_none(),
            "provider: {}",
            provider.info().name
        );
    }
}

#[tokio::test]
async fn refresh_loads_catalog_and_selects_the_best_route() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/provider/v1/models"))
        .and(header_regex("Authorization", "Bearer super-secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [
                {
                    "id": "chat-model",
                    "supported_endpoints": ["chat/completions"],
                },
                {
                    "id": "responses-model",
                    "supported_endpoints": ["/provider/v1/responses", "chat/completions"],
                },
            ],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let boundary = boundary_with_base_url(Some(provider_base_url(&server)));

    assert_eq!(
        boundary.refresh(Some(CREDENTIAL)).await,
        CatalogState::Loaded { models: 2 }
    );
    assert_eq!(
        boundary.select(Some("responses-model")),
        Ok(CommandCodeProviderStatus {
            provider_id: COMMAND_CODE_PROVIDER_ID.to_string(),
            model: Some("responses-model".to_string()),
            route: Some(CommandCodeRoute::Responses),
            catalog: CatalogState::Loaded { models: 2 },
        })
    );
    assert_eq!(
        boundary.selected_endpoint(),
        Some(format!("{}/provider/v1/responses", server.uri()))
    );

    assert_eq!(
        boundary
            .select(Some("chat-model"))
            .map(|status| status.route),
        Ok(Some(CommandCodeRoute::ChatCompletions))
    );
    assert_eq!(
        boundary.selected_endpoint(),
        Some(format!("{}/provider/v1/chat/completions", server.uri()))
    );
}

#[tokio::test]
async fn failed_refresh_reports_unavailable_and_clears_the_selection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/provider/v1/models"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    let boundary = boundary_with_base_url(Some(provider_base_url(&server)));

    assert!(boundary.select(/*model*/ None).is_ok());
    assert!(boundary.selection().is_some());

    let unavailable = CatalogState::Unavailable {
        reason: "CommandCode model discovery returned HTTP 503".to_string(),
    };
    assert_eq!(boundary.refresh(/*credential*/ None).await, unavailable);
    assert_eq!(boundary.selection(), None);
    assert_eq!(
        boundary.status(),
        CommandCodeProviderStatus {
            provider_id: COMMAND_CODE_PROVIDER_ID.to_string(),
            model: None,
            route: None,
            catalog: unavailable,
        }
    );
}

#[tokio::test]
async fn unknown_model_clears_the_previous_selection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/provider/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "data": [
                {
                    "id": "known-model",
                    "supported_endpoints": "responses",
                },
            ],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let boundary = boundary_with_base_url(Some(provider_base_url(&server)));

    assert_eq!(
        boundary.refresh(Some(CREDENTIAL)).await,
        CatalogState::Loaded { models: 1 }
    );
    assert!(boundary.select(Some("known-model")).is_ok());

    assert_eq!(
        boundary.select(Some("missing-model")),
        Err(SelectionError::UnknownModel("missing-model".to_string()))
    );
    assert_eq!(boundary.selection(), None);
    assert_eq!(boundary.status().model, None);
}

#[tokio::test]
async fn unsupported_route_clears_the_previous_selection() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/provider/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "models": [
                {
                    "id": "responses-model",
                    "supported_endpoints": ["responses"],
                },
                {
                    "id": "embedding-model",
                    "supported_endpoints": ["embeddings"],
                },
            ],
        })))
        .expect(1)
        .mount(&server)
        .await;
    let boundary = boundary_with_base_url(Some(provider_base_url(&server)));
    boundary.refresh(Some(CREDENTIAL)).await;
    assert!(boundary.select(Some("responses-model")).is_ok());

    let error = boundary
        .select(Some("embedding-model"))
        .expect_err("models without a Codex route should fail selection");

    assert!(matches!(error, SelectionError::UnsupportedRoute(_)));
    assert!(
        error
            .to_string()
            .contains("supported endpoints: embeddings"),
        "{error}"
    );
    assert_eq!(boundary.selection(), None);
}
