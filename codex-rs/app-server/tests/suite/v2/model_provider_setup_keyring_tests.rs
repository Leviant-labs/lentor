use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use codex_app_server::in_process;
use codex_app_server::in_process::InProcessClientHandle;
use codex_app_server::in_process::InProcessStartArgs;
use codex_app_server_protocol::ClientInfo;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::InitializeParams;
use codex_arg0::Arg0DispatchPaths;
use codex_config::CloudConfigBundleLoader;
use codex_config::LoaderOverrides;
use codex_config::NoopThreadConfigLoader;
use codex_core::config::ConfigBuilder;
use codex_exec_server::EnvironmentManager;
use codex_feedback::CodexFeedback;
use codex_protocol::protocol::SessionSource;
use keyring::credential::Credential;
use keyring::credential::CredentialApi;
use keyring::credential::CredentialBuilderApi;
use keyring::credential::CredentialPersistence;
use keyring::mock::MockCredential;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::process::Command;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::header;
use wiremock::matchers::method;
use wiremock::matchers::path;

type Entries = Arc<Mutex<HashMap<(String, String), Arc<MockCredential>>>>;

struct TestKeyring(Entries);
struct TestCredential(Arc<MockCredential>);

impl CredentialApi for TestCredential {
    fn set_secret(&self, secret: &[u8]) -> keyring::Result<()> {
        self.0.set_secret(secret)
    }
    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        self.0.get_secret()
    }
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        self.0.set_password(password)
    }
    fn get_password(&self) -> keyring::Result<String> {
        self.0.get_password()
    }
    fn delete_credential(&self) -> keyring::Result<()> {
        self.0.delete_credential()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl CredentialBuilderApi for TestKeyring {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        let credential = self
            .0
            .lock()
            .map_err(|_| {
                keyring::Error::PlatformFailure(Box::new(std::io::Error::other(
                    "test keyring state is unavailable",
                )))
            })?
            .entry((service.to_string(), user.to_string()))
            .or_default()
            .clone();
        Ok(Box::new(TestCredential(credential)))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::ProcessOnly
    }
}

async fn start(home: &TempDir, managed: &str) -> Result<InProcessClientHandle> {
    let managed_path = home.path().join("managed_config.toml");
    std::fs::write(&managed_path, managed)?;
    let overrides = LoaderOverrides::with_managed_config_path_for_tests(managed_path);
    let config = ConfigBuilder::default()
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .loader_overrides(overrides.clone())
        .build()
        .await?;
    Ok(in_process::start(InProcessStartArgs {
        arg0_paths: Arg0DispatchPaths::default(),
        config: Arc::new(config),
        cli_overrides: Vec::new(),
        loader_overrides: overrides,
        strict_config: false,
        cloud_config_bundle: CloudConfigBundleLoader::default(),
        thread_config_loader: Arc::new(NoopThreadConfigLoader),
        feedback: CodexFeedback::new(),
        log_db: None,
        state_db: None,
        environment_manager: Arc::new(EnvironmentManager::default_for_tests()),
        config_warnings: Vec::new(),
        embedded_network_policy: Default::default(),
        session_source: SessionSource::Cli,
        enable_codex_api_key_env: false,
        initialize: InitializeParams {
            client_info: ClientInfo {
                name: "provider-test".into(),
                title: None,
                version: "1".into(),
            },
            capabilities: None,
        },
        channel_capacity: in_process::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    })
    .await?)
}

async fn request(
    client: &InProcessClientHandle,
    id: i64,
    method: &str,
    params: Value,
) -> Result<Result<Value, codex_app_server_protocol::JSONRPCErrorError>> {
    let request: ClientRequest =
        serde_json::from_value(json!({"id": id, "method": method, "params": params}))?;
    Ok(client.request(request).await?)
}

fn params(base: &str) -> Value {
    json!({"providerId":"custom", "name":"Custom", "baseUrl":base,
        "apiKey":"synthetic-provider-key", "model":"vendor/exact"})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn configure_keyring_lifecycle() -> Result<()> {
    const CHILD: &str = "CODEX_PROVIDER_SETUP_KEYRING_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "suite::v2::model_provider_setup::keyring_tests::configure_keyring_lifecycle",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env_remove("OPENAI_API_KEY")
            .env_remove("CODEX_API_KEY")
            .output()
            .await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(
            stdout.contains("test result: ok. 1 passed; 0 failed"),
            "child did not execute exactly one test: {stdout}\n{stderr}"
        );
        assert!(
            stdout.contains("provider-keyring-child-executed"),
            "child did not reach the keyring test: {stdout}\n{stderr}"
        );
        return Ok(());
    }
    println!("provider-keyring-child-executed");
    let entries: Entries = Arc::default();
    keyring::set_default_credential_builder(Box::new(TestKeyring(Arc::clone(&entries))));
    let endpoint = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .and(header("authorization", "Bearer synthetic-provider-key"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"data":[{"id":"vendor/exact"}]})),
        )
        .expect(1..)
        .mount(&endpoint)
        .await;
    let base = format!("{}/v1", endpoint.uri());
    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        r#"
model_reasoning_effort = "high"
plan_mode_reasoning_effort = "low"

[model_providers.custom]
name = "Previous"
base_url = "https://previous.example/v1"
env_key = "PREVIOUS_PROVIDER_KEY"
env_key_instructions = "Previous instructions"
experimental_bearer_token = "previous-synthetic-token"
stream_idle_timeout_ms = 123456
request_max_retries = 7
[model_providers.custom.http_headers]
X-Custom = "preserve-me"
[model_providers.custom.capabilities]
external_web_access = false
remote_compaction = "unsupported"
"#,
    )?;
    let client = start(
        &home,
        "[model_providers.custom.http_headers]\nX-Managed = 'managed-only-synthetic-secret'\n",
    )
    .await?;
    let response = request(
        &client,
        /*id*/ 1,
        "modelProvider/configure",
        params(&base),
    )
    .await?
    .expect("configure succeeds");
    assert_eq!(
        response,
        json!({"providerId":"custom", "baseUrl":base, "model":"vendor/exact"})
    );
    let saved = std::fs::read_to_string(home.path().join("config.toml"))?;
    assert!(!saved.contains("synthetic-provider-key"));
    let config: toml::Value = toml::from_str(&saved)?;
    assert!(config.get("model_reasoning_effort").is_none());
    assert!(config.get("plan_mode_reasoning_effort").is_none());
    assert!(!saved.contains("previous-synthetic-token"));
    assert!(!saved.contains("managed-only-synthetic-secret"));
    for field in [
        "env_key",
        "env_key_instructions",
        "experimental_bearer_token",
        "auth",
        "aws",
    ] {
        assert!(
            config["model_providers"]["custom"].get(field).is_none(),
            "conflicting field {field} was retained"
        );
    }
    assert_eq!(
        config["model_providers"]["custom"]["model_catalog_format"].as_str(),
        Some("openai")
    );
    let reference = config["model_providers"]["custom"]["keyring_key"]
        .as_str()
        .unwrap();
    let stored = entries.lock().unwrap()[&(String::from("clodex-api-providers"), reference.into())]
        .get_password()?;
    assert_eq!(
        serde_json::from_str::<Value>(&stored)?,
        json!({"endpoint": base, "api_key": "synthetic-provider-key"})
    );
    let read = request(&client, /*id*/ 2, "config/read", json!({}))
        .await?
        .expect("read succeeds");
    assert!(!read.to_string().contains("synthetic-provider-key"));
    let models = request(&client, /*id*/ 3, "model/list", json!({}))
        .await?
        .expect("list succeeds");
    assert_eq!(models["data"][0]["id"], "vendor/exact");
    request(
        &client,
        /*id*/ 6,
        "modelProvider/configure",
        params(&base),
    )
    .await?
    .expect("replacement succeeds");
    let updated = std::fs::read_to_string(home.path().join("config.toml"))?;
    let updated: toml::Value = toml::from_str(&updated)?;
    let replacement = updated["model_providers"]["custom"]["keyring_key"]
        .as_str()
        .unwrap();
    let provider = &updated["model_providers"]["custom"];
    assert_eq!(
        provider["http_headers"],
        toml::Value::try_from(json!({"X-Custom": "preserve-me"}))?
    );
    assert_eq!(
        provider["stream_idle_timeout_ms"].as_integer(),
        Some(123456)
    );
    assert_eq!(provider["request_max_retries"].as_integer(), Some(7));
    assert_eq!(
        provider["capabilities"],
        toml::Value::try_from(
            json!({"external_web_access": false, "remote_compaction": "unsupported"})
        )?
    );
    assert_ne!(replacement, reference);
    assert!(
        entries.lock().unwrap()[&(String::from("clodex-api-providers"), reference.into())]
            .get_password()
            .is_ok()
    );
    client.shutdown().await?;

    let home = TempDir::new()?;
    std::fs::write(
        home.path().join("config.toml"),
        "model = 'previous-model'\n",
    )?;
    std::fs::write(
        home.path().join("requirements.toml"),
        "model_provider = 'openai'\n",
    )?;
    let client = start(&home, "").await?;
    let before = entries
        .lock()
        .unwrap()
        .keys()
        .filter(|(service, _)| service == "clodex-api-providers")
        .count();
    let error = request(
        &client,
        /*id*/ 4,
        "modelProvider/configure",
        params(&base),
    )
    .await?
    .expect_err("requirement rejects write");
    assert!(!error.message.contains("synthetic-provider-key"));
    assert_eq!(
        std::fs::read_to_string(home.path().join("config.toml"))?,
        "model = 'previous-model'\n"
    );
    {
        let map = entries.lock().unwrap();
        assert_eq!(
            map.keys()
                .filter(|(service, _)| service == "clodex-api-providers")
                .count(),
            before + 1
        );
        assert_eq!(
            map.iter()
                .filter(|((service, _), entry)| service == "clodex-api-providers"
                    && entry.get_password().is_ok())
                .count(),
            2
        );
    }
    client.shutdown().await?;

    let home = TempDir::new()?;
    let client = start(&home, "model = 'managed-model'\n").await?;
    let error = request(
        &client,
        /*id*/ 5,
        "modelProvider/configure",
        params(&base),
    )
    .await?
    .expect_err("override is reported");
    assert!(error.message.contains("saved but are not effective"));
    assert!(!error.message.contains("synthetic-provider-key"));
    let saved = std::fs::read_to_string(home.path().join("config.toml"))?;
    assert!(!saved.contains("synthetic-provider-key"));
    let config: toml::Value = toml::from_str(&saved)?;
    let reference = config["model_providers"]["custom"]["keyring_key"]
        .as_str()
        .unwrap();
    assert!(
        entries.lock().unwrap()[&(String::from("clodex-api-providers"), reference.into())]
            .get_password()
            .is_ok()
    );
    client.shutdown().await?;
    Ok(())
}
