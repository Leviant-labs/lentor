use anyhow::Result;
use codex_core::config::ConfigBuilder;
use pretty_assertions::assert_eq;
use tempfile::TempDir;

use super::ModelListCursor;
use super::ProviderIdentity;

#[tokio::test]
async fn provider_identity_excludes_credential_values_and_binds_keyring_reference() -> Result<()> {
    let home = TempDir::new()?;
    let mut config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await?;
    config.model_provider_id = "custom".to_string();
    config.model_provider.keyring_key = Some("0123456789abcdef".repeat(4));
    config.model_provider.base_url = Some("https://first.example/v1".to_string());
    config.model_provider.experimental_bearer_token = Some("first-low-entropy-secret".into());
    config.model_provider.http_headers = Some(
        [("X-Test".to_string(), "first-header-secret".into())]
            .into_iter()
            .collect(),
    );
    config.model_provider.query_params = Some(
        [("token".to_string(), "first-query-secret".into())]
            .into_iter()
            .collect(),
    );

    let first = ProviderIdentity::new(
        &config,
        "config-generation".to_string(),
        "manager-generation".to_string(),
    );
    config.model_provider.base_url = Some("https://second.example/v1".to_string());
    config.model_provider.experimental_bearer_token = Some("second-low-entropy-secret".into());
    config.model_provider.http_headers = Some(
        [("X-Test".to_string(), "second-header-secret".into())]
            .into_iter()
            .collect(),
    );
    config.model_provider.query_params = Some(
        [("token".to_string(), "second-query-secret".into())]
            .into_iter()
            .collect(),
    );
    let second = ProviderIdentity::new(
        &config,
        "config-generation".to_string(),
        "manager-generation".to_string(),
    );

    assert_eq!(first, second);
    let models = serde_json::json!([{"id": "vendor/model"}]);
    let first_version = ModelListCursor::version_with_provider(
        &first,
        /*auth_generation*/ Some(7),
        /*include_hidden*/ true,
        &models,
    )
    .unwrap_or_else(|_| panic!("first cursor version must serialize"));
    let second_version = ModelListCursor::version_with_provider(
        &second,
        /*auth_generation*/ Some(7),
        /*include_hidden*/ true,
        &models,
    )
    .unwrap_or_else(|_| panic!("second cursor version must serialize"));
    assert_eq!(first_version, second_version);
    let encoded = serde_json::to_string(&first)?;
    for secret in [
        "first-low-entropy-secret",
        "second-low-entropy-secret",
        "first-header-secret",
        "second-header-secret",
        "first-query-secret",
        "second-query-secret",
    ] {
        assert!(!encoded.contains(secret));
    }

    config.model_provider.keyring_key = Some("fedcba9876543210".repeat(4));
    let replacement = ProviderIdentity::new(
        &config,
        "config-generation".to_string(),
        "manager-generation".to_string(),
    );
    assert_ne!(first, replacement);
    let replacement_version = ModelListCursor::version_with_provider(
        &replacement,
        /*auth_generation*/ Some(7),
        /*include_hidden*/ true,
        &models,
    )
    .unwrap_or_else(|_| panic!("replacement cursor version must serialize"));
    assert_ne!(first_version, replacement_version);
    Ok(())
}
