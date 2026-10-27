use super::*;
use codex_http_client::OutboundProxyPolicy;
use pretty_assertions::assert_eq;
use wiremock::MockServer;

#[tokio::test]
async fn keyring_catalog_rejects_other_origin_before_network_or_key_lookup() {
    let endpoint_server = MockServer::start().await;
    let catalog_server = MockServer::start().await;
    let endpoint = OpenAiModelsEndpoint::new(
        ModelProviderInfo {
            base_url: Some(endpoint_server.uri()),
            model_catalog_url: Some(format!("{}/models", catalog_server.uri()).into()),
            keyring_key: Some("a".repeat(64)),
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
        /*gateway_auth_manager*/ None,
    );
    let error = endpoint
        .list_models(
            "1.0.0",
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "provider endpoint changed; configure its API key again"
    );
    assert_eq!(endpoint_server.received_requests().await.unwrap().len(), 0);
    assert_eq!(catalog_server.received_requests().await.unwrap().len(), 0);
}
