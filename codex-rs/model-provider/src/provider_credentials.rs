use codex_keyring_store::DefaultKeyringStore;
use codex_keyring_store::KeyringStore;
use serde::Deserialize;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::fmt;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;
use url::Url;

const SERVICE: &str = "clodex-api-providers";
static NEXT_REFERENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderCredentialError {
    InvalidEndpoint,
    InvalidReference,
    EmptyKey,
    StoreFailed,
    MissingKey,
    EndpointMismatch,
}

impl fmt::Display for ProviderCredentialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidEndpoint => "provider endpoint must be an HTTPS base URL (or HTTP on loopback) without embedded credentials, query, or fragment",
            Self::InvalidReference => "provider credential reference is invalid",
            Self::EmptyKey => "provider API key must contain 1 to 16384 non-control bytes",
            Self::StoreFailed => "provider credential store is unavailable",
            Self::MissingKey => "provider API key is missing; configure the provider again",
            Self::EndpointMismatch => "provider endpoint changed; configure its API key again",
        })
    }
}

impl std::error::Error for ProviderCredentialError {}

#[derive(Serialize, Deserialize)]
struct StoredCredential {
    endpoint: String,
    api_key: String,
}

/// Saves a new endpoint-bound credential; the returned reference contains no secret.
pub fn store_provider_api_key(
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, ProviderCredentialError> {
    store_key(&DefaultKeyringStore, provider_id, base_url, api_key)
}

/// Removes a reference created by a configuration write that did not succeed.
pub fn delete_provider_api_key(reference: &str) -> Result<(), ProviderCredentialError> {
    delete_key(&DefaultKeyringStore, reference)
}

pub(crate) fn load_provider_api_key(
    reference: &str,
    base_url: &str,
) -> Result<String, ProviderCredentialError> {
    load_key(&DefaultKeyringStore, reference, base_url)
}

fn endpoint(base_url: &str) -> Result<String, ProviderCredentialError> {
    let mut url = Url::parse(base_url).map_err(|_| ProviderCredentialError::InvalidEndpoint)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || (url.scheme() == "http"
            && !match url.host() {
                Some(url::Host::Domain(host)) => host.eq_ignore_ascii_case("localhost"),
                Some(url::Host::Ipv4(host)) => host.is_loopback(),
                Some(url::Host::Ipv6(host)) => host.is_loopback(),
                None => false,
            })
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ProviderCredentialError::InvalidEndpoint);
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

pub(crate) fn validate_catalog_destination(
    base_url: &str,
    catalog_url: &str,
) -> Result<(), ProviderCredentialError> {
    let base =
        Url::parse(&endpoint(base_url)?).map_err(|_| ProviderCredentialError::InvalidEndpoint)?;
    let catalog = Url::parse(catalog_url).map_err(|_| ProviderCredentialError::InvalidEndpoint)?;
    if catalog.origin() != base.origin()
        || !catalog.username().is_empty()
        || catalog.password().is_some()
        || catalog.fragment().is_some()
    {
        return Err(ProviderCredentialError::EndpointMismatch);
    }
    Ok(())
}

fn validate_reference(reference: &str) -> Result<(), ProviderCredentialError> {
    if reference.len() == 64 && reference.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ProviderCredentialError::InvalidReference)
    }
}

fn store_key(
    store: &impl KeyringStore,
    provider_id: &str,
    base_url: &str,
    api_key: &str,
) -> Result<String, ProviderCredentialError> {
    let endpoint = endpoint(base_url)?;
    let api_key = api_key.trim();
    if api_key.is_empty() || api_key.len() > 16_384 || api_key.chars().any(char::is_control) {
        return Err(ProviderCredentialError::EmptyKey);
    }
    // References are non-secret and unique so failed config writes cannot replace an active key.
    let nonce = format!(
        "{provider_id}-{endpoint}-{:?}-{}-{}",
        SystemTime::now(),
        std::process::id(),
        NEXT_REFERENCE.fetch_add(/*val*/ 1, Ordering::Relaxed)
    );
    let reference = format!("{:x}", Sha256::digest(nonce.as_bytes()));
    let payload = serde_json::to_string(&StoredCredential {
        endpoint,
        api_key: api_key.to_string(),
    })
    .map_err(|_| ProviderCredentialError::StoreFailed)?;
    store
        .save(SERVICE, &reference, &payload)
        .map_err(|_| ProviderCredentialError::StoreFailed)?;
    Ok(reference)
}

fn load_key(
    store: &impl KeyringStore,
    reference: &str,
    base_url: &str,
) -> Result<String, ProviderCredentialError> {
    validate_reference(reference)?;
    let expected = endpoint(base_url)?;
    let payload = store
        .load(SERVICE, reference)
        .map_err(|_| ProviderCredentialError::StoreFailed)?
        .ok_or(ProviderCredentialError::MissingKey)?;
    let stored: StoredCredential =
        serde_json::from_str(&payload).map_err(|_| ProviderCredentialError::StoreFailed)?;
    if stored.endpoint != expected {
        return Err(ProviderCredentialError::EndpointMismatch);
    }
    Ok(stored.api_key)
}

fn delete_key(store: &impl KeyringStore, reference: &str) -> Result<(), ProviderCredentialError> {
    validate_reference(reference)?;
    store
        .delete(SERVICE, reference)
        .map(|_| ())
        .map_err(|_| ProviderCredentialError::StoreFailed)
}

#[cfg(test)]
#[path = "provider_credentials_tests.rs"]
mod tests;
