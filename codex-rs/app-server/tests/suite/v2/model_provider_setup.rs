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
        let id = app
            .send_request(
                "modelProvider/configure",
                Some(json!({
                    "providerId": provider_id,
                    "name": "Example",
                    "baseUrl": "https://example.test/v1",
                    "apiKey": "synthetic-secret",
                    "model": null
                })),
            )
            .await?;
        let error = app
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        assert_eq!(error.error.code, -32600);
        assert!(!error.error.message.contains("synthetic-secret"));
    }
    assert_eq!(
        std::fs::read_to_string(home.path().join("config.toml"))?,
        original
    );
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
        .mount_as_scoped(&endpoint)
        .await;
    let home = TempDir::new()?;
    let base = endpoint.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model_provider = 'custom'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nwire_api = 'responses'\nrequires_openai_auth = false\n"
        ),
    )?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[("TEST_ENDPOINT_KEY", Some("synthetic-key"))])
        .build_initialized()
        .await?;
    let id = app
        .send_list_models_request(ModelListParams::default())
        .await?;
    let response: ModelListResponse = app.read_response(id).await?;
    assert_eq!(
        response
            .data
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        vec!["vendor/exact-id", "vendor/unknown"]
    );
    assert_eq!(
        serde_json::to_value(&response.data[0].supported_reasoning_efforts)?,
        json!([
            {"reasoningEffort": "low", "description": "Low"},
            {"reasoningEffort": "high", "description": "High"}
        ])
    );
    assert_eq!(
        serde_json::to_value(&response.data[0].default_reasoning_effort)?,
        json!("high")
    );
    assert!(response.data[1].supported_reasoning_efforts.is_empty());
    Ok(())
}

#[tokio::test]
async fn endpoint_model_list_keeps_cached_pages_and_returns_full_catalog_without_limit()
-> Result<()> {
    let endpoint = MockServer::start().await;
    let original_ids: Vec<String> = (0..482)
        .map(|index| format!("vendor/model-{index:03}"))
        .collect();
    let original_rows: Vec<_> = original_ids.iter().map(|id| json!({"id": id})).collect();
    let first_catalog = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer synthetic-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": original_rows})))
        // Startup refresh and the first explicit traversal can both fetch.
        .expect(1..)
        .mount_as_scoped(&endpoint)
        .await;
    let home = TempDir::new()?;
    let base = endpoint.uri();
    std::fs::write(
        home.path().join("config.toml"),
        format!(
            "model_provider = 'custom'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nrequires_openai_auth = false\n"
        ),
    )?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[("TEST_ENDPOINT_KEY", Some("synthetic-key"))])
        .build_initialized()
        .await?;
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: None,
            limit: Some(241),
            include_hidden: Some(true),
        })
        .await?;
    let first: ModelListResponse = app.read_response(id).await?;
    let cursor = first
        .next_cursor
        .clone()
        .expect("catalog has a second page");
    assert!(cursor.starts_with("v1:"));
    assert!(cursor.len() <= 96);
    assert!(!cursor.contains("synthetic-key"));
    let requests_before_continuation = endpoint.received_requests().await.unwrap().len();
    drop(first_catalog);

    // Insert a model at the front and remove the last one. A refetch during
    // continuation would repeat model-240 and omit model-481.
    let changed_ids: Vec<String> = std::iter::once("vendor/added".to_string())
        .chain(original_ids.iter().take(481).cloned())
        .collect();
    let changed_rows: Vec<_> = changed_ids.iter().map(|id| json!({"id": id})).collect();
    let changed_catalog = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer synthetic-key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"data": changed_rows})))
        .expect(1)
        .mount_as_scoped(&endpoint)
        .await;
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: first.next_cursor,
            limit: Some(241),
            include_hidden: Some(true),
        })
        .await?;
    let second: ModelListResponse = app.read_response(id).await?;
    assert_eq!(second.next_cursor, None);
    let paginated_ids: Vec<_> = first
        .data
        .into_iter()
        .chain(second.data)
        .map(|model| model.id)
        .collect();
    assert_eq!(paginated_ids, original_ids);
    assert_eq!(
        endpoint.received_requests().await.unwrap().len(),
        requests_before_continuation
    );

    // Match the TUI's request: no page limit and hidden models included. The
    // next traversal refreshes upstream and returns all 482 models in one call.
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: None,
            limit: None,
            include_hidden: Some(true),
        })
        .await?;
    let refreshed: ModelListResponse = app.read_response(id).await?;
    assert_eq!(refreshed.next_cursor, None);
    assert_eq!(
        refreshed
            .data
            .into_iter()
            .map(|model| model.id)
            .collect::<Vec<_>>(),
        changed_ids
    );
    // Another client's first-page refresh replaced the shared catalog. The old
    // offset must never be applied to that new ordering.
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: Some(cursor),
            limit: Some(241),
            include_hidden: Some(true),
        })
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(error.error.code, -32600);
    assert_eq!(
        error.error.message,
        "model list cursor is no longer valid; restart pagination"
    );
    drop(changed_catalog);
    Ok(())
}

#[tokio::test]
async fn endpoint_model_list_rejects_cursors_after_refresh_failure_and_provider_change()
-> Result<()> {
    let endpoint = MockServer::start().await;
    let body = json!({"data": [{"id": "vendor/a"}, {"id": "vendor/b"}]});
    let success = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body.clone()))
        .expect(1..)
        .mount_as_scoped(&endpoint)
        .await;
    let home = TempDir::new()?;
    let base = endpoint.uri();
    let config = format!(
        "model_provider = 'custom'\ncli_auth_credentials_store = 'file'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nrequires_openai_auth = false\n"
    );
    std::fs::write(home.path().join("config.toml"), &config)?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .with_env_overrides(&[
            ("TEST_ENDPOINT_KEY", Some("synthetic-key")),
            (
                "REPLACEMENT_ENDPOINT_KEY",
                Some("synthetic-replacement-key"),
            ),
            ("OPENAI_API_KEY", None),
            ("CODEX_API_KEY", None),
        ])
        .build_initialized()
        .await?;
    let page = ModelListParams {
        cursor: None,
        limit: Some(1),
        include_hidden: Some(true),
    };
    let id = app.send_list_models_request(page.clone()).await?;
    let first: ModelListResponse = app.read_response(id).await?;
    let cursor = first.next_cursor.expect("second page");
    let requests_before_invalid_cursors = endpoint.received_requests().await.unwrap().len();
    drop(success);

    // The view is part of the cursor even if this catalog has no hidden rows.
    for invalid in [cursor.clone(), "1".to_string()] {
        let id = app
            .send_list_models_request(ModelListParams {
                cursor: Some(invalid),
                limit: Some(1),
                include_hidden: Some(false),
            })
            .await?;
        let error = app
            .read_stream_until_error_message(RequestId::Integer(id))
            .await?;
        assert_eq!(error.error.code, -32600);
    }
    assert_eq!(
        endpoint.received_requests().await.unwrap().len(),
        requests_before_invalid_cursors
    );
    let failure = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(400))
        .expect(1..)
        .mount_as_scoped(&endpoint)
        .await;
    let id = app.send_list_models_request(page.clone()).await?;
    app.read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: Some(cursor),
            ..page.clone()
        })
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(error.error.code, -32600);
    assert_eq!(
        error.error.message,
        "model list cursor is no longer valid; restart pagination"
    );
    drop(failure);

    let _recovered = Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .expect(1..)
        .mount_as_scoped(&endpoint)
        .await;
    let id = app.send_list_models_request(page.clone()).await?;
    let first: ModelListResponse = app.read_response(id).await?;
    // Change provider/credential routing without changing the catalog rows.
    // Login invalidates the app-server catalog and reloads the provider config.
    std::fs::write(
        home.path().join("config.toml"),
        config
            .replace("name = 'Custom'", "name = 'Replacement'")
            .replace(
                "env_key = 'TEST_ENDPOINT_KEY'",
                "env_key = 'REPLACEMENT_ENDPOINT_KEY'",
            ),
    )?;
    let id = app
        .send_login_account_api_key_request("synthetic-account-key")
        .await?;
    let _: codex_app_server_protocol::LoginAccountResponse = app.read_response(id).await?;
    let id = app
        .send_list_models_request(ModelListParams {
            cursor: first.next_cursor,
            ..page
        })
        .await?;
    let error = app
        .read_stream_until_error_message(RequestId::Integer(id))
        .await?;
    assert_eq!(error.error.code, -32600);
    assert_eq!(
        error.error.message,
        "model list cursor is no longer valid; restart pagination"
    );
    Ok(())
}

#[tokio::test]
async fn endpoint_model_list_preserves_empty_and_error_results() -> Result<()> {
    for status in [200, 401] {
        let endpoint = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(status).set_body_json(json!({"data": []})))
            .mount(&endpoint)
            .await;
        let home = TempDir::new()?;
        let base = endpoint.uri();
        std::fs::write(
            home.path().join("config.toml"),
            format!(
                "model_provider = 'custom'\n[features]\napi_key_model_discovery = true\n[model_providers.custom]\nname = 'Custom'\nbase_url = '{base}/v1'\nmodel_catalog_url = '{base}/v1/models'\nmodel_catalog_format = 'openai'\nenv_key = 'TEST_ENDPOINT_KEY'\nrequires_openai_auth = false\n"
            ),
        )?;
        let mut app = TestAppServer::builder()
            .with_codex_home(home.path())
            .with_env_overrides(&[("TEST_ENDPOINT_KEY", Some("synthetic-key"))])
            .build_initialized()
            .await?;
        let id = app
            .send_list_models_request(ModelListParams::default())
            .await?;
        if status == 200 {
            let response: ModelListResponse = app.read_response(id).await?;
            assert!(response.data.is_empty());
            assert_eq!(response.next_cursor, None);
        } else {
            let error = app
                .read_stream_until_error_message(RequestId::Integer(id))
                .await?;
            assert!(!error.error.message.contains("synthetic-key"));
        }
    }
    Ok(())
}
