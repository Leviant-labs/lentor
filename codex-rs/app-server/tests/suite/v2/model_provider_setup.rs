use anyhow::Result;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::RequestId;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[path = "model_provider_setup_keyring_tests.rs"]
mod keyring_tests;

#[tokio::test]
async fn configure_rejects_invalid_provider_before_writing_config() -> Result<()> {
    let home = TempDir::new()?;
    let original = "model = 'previous-model'\n";
    std::fs::write(home.path().join("config.toml"), original)?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    for provider_id in ["openai", "invalid.id", ""] {
        let id = app.send_request("modelProvider/configure", Some(json!({
            "providerId": provider_id,
            "name": "Example",
            "baseUrl": "https://example.test/v1",
            "apiKey": "synthetic-secret",
            "model": null
        }))).await?;
        let error = app.read_stream_until_error_message(RequestId::Integer(id)).await?;
        assert_eq!(error.error.code, -32600);
        assert!(!error.error.message.contains("synthetic-secret"));
    }
    assert_eq!(std::fs::read_to_string(home.path().join("config.toml"))?, original);
    Ok(())
}

#[tokio::test]
async fn endpoint_model_list_uses_authoritative_sparse_catalog() -> Result<()> {
    let endpoint = MockServer::start().await;
    let _mock = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer synthetic-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": [
            {"id": "vendor/exact-id", "default_reasoning_level": "high",
             "supported_reasoning_levels": [
                {"effort": "low", "description": "Low"},
                {"effort": "high", "description": "High"}
             ]},
            {"id": "vendor/unknown"}
        ]})))
        .expect(1..)
        .mount_as_scoped(&endpoint).await;
    let home = TempDir::new()?;
    let base = endpoint.uri();
    std::fs::write(home.path().join("config.toml"), format!(
        "model_provider = 'custom'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nwire_api = 'responses'\nrequires_openai_auth = false\n"
    ))?;
    let mut app = TestAppServer::builder().with_codex_home(home.path())
        .with_env_overrides(&[("TEST_ENDPOINT_KEY", Some("synthetic-key"))])
        .build_initialized().await?;
    let id = app.send_list_models_request(ModelListParams::default()).await?;
    let response: ModelListResponse = app.read_response(id).await?;
    assert_eq!(response.data.iter().map(|item| item.id.as_str()).collect::<Vec<_>>(),
        vec!["vendor/exact-id", "vendor/unknown"]);
    assert_eq!(serde_json::to_value(&response.data[0].supported_reasoning_efforts)?, json!([
        {"reasoningEffort": "low", "description": "Low"},
        {"reasoningEffort": "high", "description": "High"}
    ]));
    assert_eq!(serde_json::to_value(&response.data[0].default_reasoning_effort)?, json!("high"));
    assert!(response.data[1].supported_reasoning_efforts.is_empty());
    Ok(())
}

#[tokio::test]
async fn endpoint_model_list_preserves_empty_and_error_results() -> Result<()> {
    for status in [200, 401] {
        let endpoint = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"data": []})))
            .mount(&endpoint).await;
        let home = TempDir::new()?;
        let base = endpoint.uri();
        std::fs::write(home.path().join("config.toml"), format!(
            "model_provider = 'custom'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nrequires_openai_auth = false\n"
        ))?;
        let mut app = TestAppServer::builder().with_codex_home(home.path())
            .with_env_overrides(&[("TEST_ENDPOINT_KEY", Some("synthetic-key"))])
            .build_initialized().await?;
        let id = app.send_list_models_request(ModelListParams::default()).await?;
        if status == 200 {
            let response: ModelListResponse = app.read_response(id).await?;
            assert!(response.data.is_empty());
            assert_eq!(response.next_cursor, None);
        } else {
            let error = app.read_stream_until_error_message(RequestId::Integer(id)).await?;
            assert!(!error.error.message.contains("synthetic-key"));
        }
    }
    Ok(())
}
