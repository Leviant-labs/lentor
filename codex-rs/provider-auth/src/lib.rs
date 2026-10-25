//! Provider-scoped credentials and CommandCode authentication.

mod callback;

use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;
use rand::RngCore;
use reqwest::Client;
use serde::Deserialize;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::io::AsyncReadExt as _;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use url::Url;

pub use codex_protocol::provider::CommandCodeAuthSource as AuthSource;

const SERVICE: &str = "codex-provider-auth";
const PROVIDER: &str = "commandcode";
const WHOAMI_ENDPOINT: &str = "https://api.commandcode.ai/alpha/whoami";
const AUTHORIZE_ENDPOINT: &str = "https://commandcode.ai/studio/auth/cli";
const WHOAMI_TIMEOUT: Duration = Duration::from_secs(10);
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_WHOAMI_BYTES: usize = 64 * 1024;
const MAX_LOCAL_AUTH_BYTES: u64 = 64 * 1024;
const LOCAL_AUTH_PATH: &str = ".commandcode/auth.json";

/// Non-secret identity details returned by CommandCode's whoami endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandCodeIdentity {
    pub user_id: String,
    pub user_name: String,
    pub key_name: Option<String>,
    pub source: AuthSource,
}

/// Errors intentionally carry no request, response, or credential contents.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("CommandCode authentication failed")]
    AuthenticationFailed,
    #[error("CommandCode authentication request timed out or failed")]
    RequestFailed,
    #[error("CommandCode callback state did not match")]
    StateMismatch,
    #[error("CommandCode callback payload was invalid")]
    InvalidCallback,
    #[error("CommandCode login timed out")]
    TimedOut,
    #[error("CommandCode login was cancelled")]
    Cancelled,
    #[error("CommandCode credential store failed")]
    StoreFailed,
    #[error("CommandCode CLI credential import failed")]
    ImportFailed,
    #[error("CommandCode API key environment variable is not set")]
    MissingEnvironmentKey,
}

/// Secret-bearing API key with redacted formatting.
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([REDACTED])")
    }
}

/// Persistence for provider-isolated secrets and identities.
pub struct CredentialStore<S = DefaultKeyringStore> {
    store: S,
}

impl Default for CredentialStore<DefaultKeyringStore> {
    fn default() -> Self {
        Self {
            store: DefaultKeyringStore,
        }
    }
}

impl<S: KeyringStore> CredentialStore<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    /// Persists the secret, then non-secret identity metadata. The current
    /// secret is read first so a failed metadata write can restore it, and a
    /// credential that cannot be read is never overwritten.
    pub fn save(
        &self,
        provider: &str,
        key: &ApiKey,
        identity: &CommandCodeIdentity,
    ) -> Result<(), AuthError> {
        let metadata = serde_json::to_string(identity).map_err(|_| AuthError::StoreFailed)?;
        let secret_account = format!("{provider}:secret");
        let identity_account = format!("{provider}:identity");
        let previous_secret = self.load_secret(provider)?;
        self.store
            .save(SERVICE, &secret_account, key.expose())
            .map_err(|_| AuthError::StoreFailed)?;
        if self
            .store
            .save(SERVICE, &identity_account, &metadata)
            .is_err()
        {
            match previous_secret {
                Some(previous) => {
                    let _ = self.store.save(SERVICE, &secret_account, previous.expose());
                }
                None => {
                    let _ = self.store.delete(SERVICE, &secret_account);
                }
            }
            return Err(AuthError::StoreFailed);
        }
        Ok(())
    }

    pub fn load_secret(&self, provider: &str) -> Result<Option<ApiKey>, AuthError> {
        self.store
            .load(SERVICE, &format!("{provider}:secret"))
            .map(|value| value.map(ApiKey::new))
            .map_err(|_| AuthError::StoreFailed)
    }

    pub fn load_identity(&self, provider: &str) -> Result<Option<CommandCodeIdentity>, AuthError> {
        self.store
            .load(SERVICE, &format!("{provider}:identity"))
            .map_err(|_| AuthError::StoreFailed)?
            .map(|value| serde_json::from_str(&value).map_err(|_| AuthError::StoreFailed))
            .transpose()
    }
}

/// Validates CommandCode credentials and drives the browser login flow.
#[derive(Clone)]
pub struct Authenticator {
    client: Client,
    endpoint: String,
    login_timeout: Duration,
}

impl Default for Authenticator {
    fn default() -> Self {
        Self::new()
    }
}

impl Authenticator {
    /// Creates an authenticator for CommandCode's whoami endpoint.
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(WHOAMI_TIMEOUT)
                .build()
                .unwrap_or_default(),
            endpoint: WHOAMI_ENDPOINT.to_string(),
            login_timeout: LOGIN_TIMEOUT,
        }
    }

    /// Creates an authenticator against a caller-supplied endpoint.
    ///
    /// Test-only: production validation must target CommandCode so the
    /// credential cannot be sent to another host.
    #[cfg(test)]
    fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            ..Self::new()
        }
    }

    /// Overrides how long a pending login waits for a callback, paste, or abort.
    pub fn with_login_timeout(mut self, timeout: Duration) -> Self {
        self.login_timeout = timeout;
        self
    }

    pub async fn validate_api_key(
        &self,
        key: &ApiKey,
        source: AuthSource,
    ) -> Result<CommandCodeIdentity, AuthError> {
        let mut response = self
            .client
            .get(&self.endpoint)
            .bearer_auth(key.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|_| AuthError::RequestFailed)?;
        if !response.status().is_success() {
            return Err(AuthError::AuthenticationFailed);
        }
        let body: Whoami = read_bounded_json(&mut response, MAX_WHOAMI_BYTES).await?;
        let user = body.user.ok_or(AuthError::AuthenticationFailed)?;
        let user_id = user
            .id
            .filter(|value| !value.trim().is_empty())
            .ok_or(AuthError::AuthenticationFailed)?;
        let user_name = user
            .user_name
            .filter(|value| !value.trim().is_empty())
            .ok_or(AuthError::AuthenticationFailed)?;
        Ok(CommandCodeIdentity {
            user_id,
            user_name,
            key_name: user.key_name,
            source,
        })
    }

    pub async fn authenticate_and_store<S: KeyringStore>(
        &self,
        store: &CredentialStore<S>,
        provider: &str,
        key: ApiKey,
        source: AuthSource,
    ) -> Result<CommandCodeIdentity, AuthError> {
        let identity = self.validate_api_key(&key, source).await?;
        store.save(provider, &key, &identity)?;
        Ok(identity)
    }

    /// Validates the API key held by the named environment variable.
    pub async fn authenticate_from_env<S: KeyringStore>(
        &self,
        store: &CredentialStore<S>,
        provider: &str,
        variable: &str,
    ) -> Result<CommandCodeIdentity, AuthError> {
        let key = api_key_from_env(variable)?;
        self.authenticate_and_store(store, provider, key, AuthSource::ApiKey)
            .await
    }

    /// Binds the loopback callback listener and builds the authorization URL.
    ///
    /// The returned PendingLogin exposes both URLs before any wait begins, so
    /// a caller can open the browser and then await completion.
    pub async fn begin_login(&self) -> Result<PendingLogin, AuthError> {
        let state = random_state();
        let listeners = callback::bind_loopback()
            .await
            .map_err(|_| AuthError::RequestFailed)?;
        let callback_url = format!("http://127.0.0.1:{}/callback", listeners.port);
        let mut auth_url = Url::parse(AUTHORIZE_ENDPOINT).map_err(|_| AuthError::RequestFailed)?;
        auth_url
            .query_pairs_mut()
            .append_pair("callback", &callback_url)
            .append_pair("state", &state);
        let (key_tx, key_rx) = oneshot::channel();
        let deadline = tokio::time::Instant::now() + self.login_timeout;
        let server_state = state.clone();
        let server = tokio::spawn(async move {
            let _ = tokio::time::timeout_at(
                deadline,
                callback::serve_callback(listeners, server_state, key_tx),
            )
            .await;
        });
        Ok(PendingLogin {
            authenticator: self.clone(),
            state,
            callback_url,
            auth_url: auth_url.to_string(),
            deadline,
            key_rx,
            server,
        })
    }

    /// Runs a complete login, stores the validated credential, and returns the
    /// key, its identity, and the authorization URL that was presented.
    pub async fn login<S: KeyringStore>(
        &self,
        store: &CredentialStore<S>,
        provider: &str,
        options: LoginOptions,
    ) -> Result<(ApiKey, CommandCodeIdentity, String), AuthError> {
        let pending = self.begin_login().await?;
        let auth_url = pending.auth_url().to_string();
        let (key, identity) = pending.complete(store, provider, options).await?;
        Ok((key, identity, auth_url))
    }

    /// Imports the CommandCode CLI credential, validates it, and stores it.
    ///
    /// Only the API key is read. The source file is left untouched and no copy
    /// of it is persisted.
    pub async fn import_local<S: KeyringStore>(
        &self,
        store: &CredentialStore<S>,
        path: Option<PathBuf>,
    ) -> Result<Option<CommandCodeIdentity>, AuthError> {
        let path = path.unwrap_or_else(local_auth_path);
        let bytes = read_local_auth(&path).await?;
        let parsed: LocalAuth =
            serde_json::from_slice(&bytes).map_err(|_| AuthError::ImportFailed)?;
        let key = ApiKey::new(parsed.api_key.trim());
        if key.expose().is_empty() {
            return Err(AuthError::ImportFailed);
        }
        let identity = self.validate_api_key(&key, AuthSource::ImportedCli).await?;
        store.save(PROVIDER, &key, &identity)?;
        Ok(Some(identity))
    }
}

/// Inputs that accompany a pending login.
#[derive(Default)]
pub struct LoginOptions {
    /// Pasted callback payloads, authorization URLs, or API keys. Entries that
    /// fail the applicable state check are ignored while the login keeps
    /// waiting for a valid response.
    pub manual: Option<mpsc::Receiver<String>>,
    /// Resolves when the caller cancels the login.
    pub abort: Option<oneshot::Receiver<()>>,
}

/// A bound loopback listener with an authorization URL ready to present.
pub struct PendingLogin {
    authenticator: Authenticator,
    state: String,
    callback_url: String,
    auth_url: String,
    deadline: tokio::time::Instant,
    key_rx: oneshot::Receiver<ApiKey>,
    server: tokio::task::JoinHandle<()>,
}

impl PendingLogin {
    pub fn auth_url(&self) -> &str {
        &self.auth_url
    }

    pub fn callback_url(&self) -> &str {
        &self.callback_url
    }

    /// Waits for a state-checked callback, a valid paste, a timeout, or an
    /// abort, then validates the accepted key with the whoami endpoint and
    /// stores it for the provider.
    pub async fn complete<S: KeyringStore>(
        mut self,
        store: &CredentialStore<S>,
        provider: &str,
        mut options: LoginOptions,
    ) -> Result<(ApiKey, CommandCodeIdentity), AuthError> {
        let mut abort = options.abort.take();
        let deadline = self.deadline;
        let key = tokio::select! {
            biased;
            _ = async {
                if let Some(receiver) = abort.as_mut() {
                    let _ = receiver.await;
                } else {
                    std::future::pending::<()>().await;
                }
            } => Err(AuthError::Cancelled),
            received = &mut self.key_rx => match received {
                Ok(key) => Ok(key),
                Err(_) if tokio::time::Instant::now() >= deadline => Err(AuthError::TimedOut),
                Err(_) => Err(AuthError::Cancelled),
            },
            _ = tokio::time::sleep_until(deadline) => Err(AuthError::TimedOut),
            key = wait_for_manual(&mut options.manual, &self.state) => key,
        }?;
        self.server.abort();
        let identity = self
            .authenticator
            .validate_api_key(&key, AuthSource::OAuth)
            .await?;
        store.save(provider, &key, &identity)?;
        Ok((key, identity))
    }
}

impl Drop for PendingLogin {
    fn drop(&mut self) {
        self.server.abort();
    }
}

/// Resolves a CommandCode API key from the named environment variable.
pub fn api_key_from_env(variable: &str) -> Result<ApiKey, AuthError> {
    let variable = variable.trim();
    if variable.is_empty() {
        return Err(AuthError::MissingEnvironmentKey);
    }
    let value = std::env::var(variable).map_err(|_| AuthError::MissingEnvironmentKey)?;
    let value = value.trim();
    if value.is_empty() {
        return Err(AuthError::MissingEnvironmentKey);
    }
    Ok(ApiKey::new(value))
}

async fn wait_for_manual(
    manual: &mut Option<mpsc::Receiver<String>>,
    state: &str,
) -> Result<ApiKey, AuthError> {
    let Some(receiver) = manual.as_mut() else {
        return std::future::pending().await;
    };
    while let Some(input) = receiver.recv().await {
        if let Ok(key) = callback::parse_manual(&input, state) {
            return Ok(key);
        }
    }
    std::future::pending().await
}

async fn read_bounded_json<T: DeserializeOwned>(
    response: &mut reqwest::Response,
    limit: usize,
) -> Result<T, AuthError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(AuthError::AuthenticationFailed);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AuthError::RequestFailed)?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(AuthError::AuthenticationFailed);
        }
        body.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&body).map_err(|_| AuthError::AuthenticationFailed)
}

async fn read_local_auth(path: &Path) -> Result<Vec<u8>, AuthError> {
    let mut reader = tokio::fs::File::open(path)
        .await
        .map_err(|_| AuthError::ImportFailed)?
        .take(MAX_LOCAL_AUTH_BYTES + 1);
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| AuthError::ImportFailed)?;
    if bytes.len() as u64 > MAX_LOCAL_AUTH_BYTES {
        return Err(AuthError::ImportFailed);
    }
    Ok(bytes)
}

fn local_auth_path() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
        .join(LOCAL_AUTH_PATH)
}

#[derive(Deserialize)]
struct Whoami {
    user: Option<User>,
}

#[derive(Deserialize)]
struct User {
    id: Option<String>,
    #[serde(rename = "userName")]
    user_name: Option<String>,
    #[serde(rename = "keyName")]
    key_name: Option<String>,
}

#[derive(Deserialize)]
struct LocalAuth {
    #[serde(rename = "apiKey")]
    api_key: String,
}

fn random_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
