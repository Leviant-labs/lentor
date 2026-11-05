use anyhow::Result;
use codex_config::types::AuthCredentialsStoreMode;
use codex_http_client::HttpClientBuilder;
use codex_login::AuthKeyringBackendKind;
use codex_login::ServerOptions;
use codex_login::run_login_server;
use tempfile::tempdir;

#[tokio::test]
async fn login_success_pages_identity() -> Result<()> {
    let home = tempdir()?;
    let client = HttpClientBuilder::new().build_direct()?;
    for (query, snapshot) in [
        ("", "login_success_legacy"),
        ("?codex_streamlined_login=true", "login_success_streamlined"),
    ] {
        let mut opts = ServerOptions::new(
            home.path().to_path_buf(),
            "test-client".to_string(),
            /*forced_chatgpt_workspace_id*/ None,
            AuthCredentialsStoreMode::File,
            AuthKeyringBackendKind::default(),
            codex_login::test_support::transport_default_auth_route_config(),
        );
        opts.port = 0;
        opts.open_browser = false;
        let server = run_login_server(opts)?;
        let url = format!("http://127.0.0.1:{}/success{query}", server.actual_port);
        let response = client.get(url).send().await?;
        assert!(response.status().is_success());
        insta::assert_snapshot!(snapshot, response.text().await?);
        server.block_until_done().await?;
    }
    Ok(())
}
