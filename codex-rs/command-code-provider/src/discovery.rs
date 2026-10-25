//! Bounded retrieval of the CommandCode model catalog.

use std::time::Duration;

use codex_model_provider_info::COMMAND_CODE_PROVIDER_BASE_URL;
use reqwest::Client;
use reqwest::Response;
use thiserror::Error;

use crate::catalog::CatalogError;
use crate::catalog::CommandCodeCatalog;

/// Maximum response size accepted from the model catalog endpoint.
pub const MAX_MODEL_CATALOG_BYTES: usize = 1024 * 1024;

/// Deadline for one model catalog request, including its response body.
pub const MODELS_TIMEOUT: Duration = Duration::from_secs(5);

/// Fetches CommandCode's authoritative model catalog over HTTPS.
#[derive(Clone, Debug)]
pub struct CommandCodeDiscovery {
    client: Client,
    endpoint: String,
    timeout: Duration,
}

impl Default for CommandCodeDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

impl CommandCodeDiscovery {
    /// Creates a discovery client for the built-in CommandCode endpoint.
    pub fn new() -> Self {
        Self::for_base_url(COMMAND_CODE_PROVIDER_BASE_URL)
    }

    /// Creates a discovery client for a provider base URL ending in `/provider/v1`.
    pub fn for_base_url(base_url: &str) -> Self {
        Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            endpoint: format!("{}/models", base_url.trim_end_matches('/')),
            timeout: MODELS_TIMEOUT,
        }
    }

    /// Overrides the total request deadline.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Creates a client for a caller-supplied catalog endpoint.
    ///
    /// Test-only: production discovery must target CommandCode so credentials
    /// cannot be sent to another host.
    #[cfg(test)]
    pub(crate) fn with_endpoint(endpoint: impl Into<String>) -> Self {
        Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
            endpoint: endpoint.into(),
            timeout: MODELS_TIMEOUT,
        }
    }

    /// Returns the catalog, rejecting oversized or malformed responses.
    pub async fn discover(
        &self,
        credential: Option<&str>,
    ) -> Result<CommandCodeCatalog, DiscoveryError> {
        tokio::time::timeout(self.timeout, self.fetch(credential))
            .await
            .map_err(|_| DiscoveryError::TimedOut)?
    }

    async fn fetch(&self, credential: Option<&str>) -> Result<CommandCodeCatalog, DiscoveryError> {
        let mut request = self
            .client
            .get(&self.endpoint)
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(credential) = credential.map(str::trim).filter(|value| !value.is_empty()) {
            request = request.bearer_auth(credential);
        }

        let mut response = request
            .send()
            .await
            .map_err(|_| DiscoveryError::RequestFailed)?;
        let status = response.status();
        if !status.is_success() {
            return Err(DiscoveryError::HttpStatus(status.as_u16()));
        }
        let body = read_bounded(&mut response, MAX_MODEL_CATALOG_BYTES).await?;
        CommandCodeCatalog::parse(&body).map_err(DiscoveryError::from)
    }
}

/// Errors from model catalog retrieval.
///
/// Variants intentionally carry no response body or credential contents.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("CommandCode model discovery request failed")]
    RequestFailed,
    #[error("CommandCode model discovery returned HTTP {0}")]
    HttpStatus(u16),
    #[error("CommandCode model discovery timed out")]
    TimedOut,
    #[error("CommandCode model catalog exceeded the {limit}-byte response limit")]
    ResponseTooLarge { limit: usize },
    #[error(transparent)]
    InvalidCatalog(#[from] CatalogError),
}

async fn read_bounded(response: &mut Response, limit: usize) -> Result<Vec<u8>, DiscoveryError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(DiscoveryError::ResponseTooLarge { limit });
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| DiscoveryError::RequestFailed)?
    {
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(DiscoveryError::ResponseTooLarge { limit });
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}
