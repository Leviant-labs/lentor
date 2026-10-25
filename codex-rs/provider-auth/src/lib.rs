//! Provider-scoped credentials and CommandCode authentication.

use codex_keyring_store::{DefaultKeyringStore, KeyringStore};
use rand::RngCore;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;
use std::time::Duration;
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use url::Url;

const SERVICE: &str = "codex-provider-auth";
const PROVIDER: &str = "commandcode";
const WHOAMI: &str = "https://api.commandcode.ai/alpha/whoami";
const CALLBACK_PORT: u16 = 5959;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(120);

/// Authentication provenance for a provider credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthSource {
    ApiKey,
    OAuth,
    ImportedCli,
}

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

    pub fn save(
        &self,
        provider: &str,
        key: &ApiKey,
        identity: &CommandCodeIdentity,
    ) -> Result<(), AuthError> {
        self.store
            .save(SERVICE, &format!("{provider}:secret"), key.expose())
            .map_err(|_| AuthError::StoreFailed)?;
        let metadata = serde_json::to_string(identity).map_err(|_| AuthError::StoreFailed)?;
        self.store
            .save(SERVICE, &format!("{provider}:identity"), &metadata)
            .map_err(|_| AuthError::StoreFailed)
    }

    pub fn load_secret(&self, provider: &str) -> Result<Option<ApiKey>, AuthError> {
        self.store
            .load(SERVICE, &format!("{provider}:secret"))
            .map(|v| v.map(ApiKey::new))
            .map_err(|_| AuthError::StoreFailed)
    }

    pub fn load_identity(&self, provider: &str) -> Result<Option<CommandCodeIdentity>, AuthError> {
        self.store
            .load(SERVICE, &format!("{provider}:identity"))
            .map_err(|_| AuthError::StoreFailed)?
            .map(|v| serde_json::from_str(&v).map_err(|_| AuthError::StoreFailed))
            .transpose()
    }
}

#[derive(Clone)]
pub struct Authenticator {
    client: Client,
    endpoint: String,
}

impl Default for Authenticator {
    fn default() -> Self {
        Self::new(WHOAMI)
    }
}

impl Authenticator {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("valid HTTP client"),
            endpoint: endpoint.into(),
        }
    }

    pub async fn validate_api_key(
        &self,
        key: &ApiKey,
        source: AuthSource,
    ) -> Result<CommandCodeIdentity, AuthError> {
        let response = self
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
        let body: Whoami = response
            .json()
            .await
            .map_err(|_| AuthError::AuthenticationFailed)?;
        let user = body.user.ok_or(AuthError::AuthenticationFailed)?;
        let user_id = user
            .id
            .filter(|v| !v.trim().is_empty())
            .ok_or(AuthError::AuthenticationFailed)?;
        let user_name = user
            .user_name
            .filter(|v| !v.trim().is_empty())
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

    pub async fn login(
        &self,
        manual: Option<oneshot::Receiver<String>>,
        mut abort: Option<oneshot::Receiver<()>>,
    ) -> Result<(ApiKey, CommandCodeIdentity, String), AuthError> {
        let state = random_state();
        let listener = bind_loopback()
            .await
            .map_err(|_| AuthError::RequestFailed)?;
        let port = listener
            .local_addr()
            .map_err(|_| AuthError::RequestFailed)?
            .port();
        let callback = format!("http://127.0.0.1:{port}/callback");
        let mut auth_url =
            Url::parse("https://commandcode.ai/studio/auth/cli").expect("static URL");
        auth_url
            .query_pairs_mut()
            .append_pair("callback", &callback)
            .append_pair("state", &state);
        let (tx, rx) = oneshot::channel();
        let expected = state.clone();
        let server = tokio::spawn(async move { callback_loop(listener, expected, tx).await });
        let result = tokio::select! {
            biased;
            _ = tokio::time::sleep(LOGIN_TIMEOUT) => Err(AuthError::TimedOut),
            _ = async { if let Some(rx) = abort.as_mut() { let _ = rx.await; } else { std::future::pending::<()>().await; } } => Err(AuthError::Cancelled),
            value = rx => value.map_err(|_| AuthError::Cancelled)?,
            value = async { match manual { Some(rx) => rx.await.map_err(|_| AuthError::Cancelled), None => std::future::pending().await } } => {
                let pasted = value?;
                let key = parse_manual(&pasted, &state)?;
                Ok(key)
            }
        };
        server.abort();
        let key = result?;
        let identity = self.validate_api_key(&key, AuthSource::OAuth).await?;
        Ok((key, identity, auth_url.to_string()))
    }

    pub async fn import_local<S: KeyringStore>(
        &self,
        store: &CredentialStore<S>,
        path: Option<PathBuf>,
    ) -> Result<Option<CommandCodeIdentity>, AuthError> {
        let path = path.unwrap_or_else(|| {
            std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".commandcode/auth.json")
        });
        let bytes = tokio::fs::read(path)
            .await
            .map_err(|_| AuthError::ImportFailed)?;
        let parsed: LocalAuth =
            serde_json::from_slice(&bytes).map_err(|_| AuthError::ImportFailed)?;
        drop(bytes);
        let key = ApiKey::new(
            parsed
                .api_key
                .filter(|s| !s.is_empty())
                .ok_or(AuthError::ImportFailed)?,
        );
        let identity = self.validate_api_key(&key, AuthSource::ImportedCli).await?;
        store.save(PROVIDER, &key, &identity)?;
        Ok(Some(identity))
    }
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
    api_key: Option<String>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Callback {
    api_key: String,
    state: String,
}

fn random_state() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn bind_loopback() -> std::io::Result<TcpListener> {
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", CALLBACK_PORT)).await {
        return Ok(listener);
    }
    let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
    let port = listener.local_addr()?.port();
    let _ipv6 = TcpListener::bind(("::1", port)).await.ok();
    Ok(listener)
}

async fn callback_loop(
    listener: TcpListener,
    state: String,
    tx: oneshot::Sender<Result<ApiKey, AuthError>>,
) {
    let Ok((mut stream, _)) = listener.accept().await else {
        return;
    };
    let mut buffer = vec![0; 16 * 1024];
    let Ok(n) = stream.read(&mut buffer).await else {
        return;
    };
    let request = String::from_utf8_lossy(&buffer[..n]);
    let body = request.split("\r\n\r\n").nth(1).unwrap_or_default();
    let parsed: Result<Callback, _> = serde_json::from_str(body);
    let result = match parsed {
        Ok(data) if data.state == state => {
            let key = ApiKey::new(data.api_key);
            Ok(key)
        }
        Ok(_) => Err(AuthError::StateMismatch),
        Err(_) => Err(AuthError::InvalidCallback),
    };
    let response = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK";
    let _ = stream.write_all(response).await;
    let _ = tx.send(result);
}

fn parse_manual(input: &str, state: &str) -> Result<ApiKey, AuthError> {
    let input = input.trim();
    if input.starts_with('{') {
        let callback: Callback =
            serde_json::from_str(input).map_err(|_| AuthError::InvalidCallback)?;
        if callback.state != state {
            return Err(AuthError::StateMismatch);
        }
        return Ok(ApiKey::new(callback.api_key));
    }
    if let Ok(url) = Url::parse(input) {
        let query: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        if query.get("state").map(String::as_str) != Some(state) {
            return Err(AuthError::StateMismatch);
        }
        return query
            .get("apiKey")
            .or_else(|| query.get("code"))
            .map(|v| ApiKey::new(v.clone()))
            .ok_or(AuthError::InvalidCallback);
    }
    if input.is_empty() {
        Err(AuthError::InvalidCallback)
    } else {
        Ok(ApiKey::new(input))
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
