//! Authenticated HTTP streaming for translated provider protocols.

use std::sync::Arc;

use codex_client::EncodedJsonBody;
use codex_client::HttpTransport;
use codex_client::RequestTelemetry;
use codex_client::StreamResponse;
use http::HeaderMap;
use http::HeaderValue;
use http::Method;
use serde_json::Value;

use crate::auth::SharedAuthProvider;
use crate::endpoint::session::EndpointSession;
use crate::error::ApiError;
use crate::provider::Provider;

/// Supported translated generation endpoints.
#[derive(Clone, Copy, Debug)]
pub enum TranslatedApi {
    Chat,
    Messages,
}

#[cfg(test)]
#[path = "translated_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "translated_base_url_tests.rs"]
mod translated_base_url_tests;

/// Streams provider JSON using the normal configured transport, auth and retry policy.
pub struct TranslatedStreamClient<T: HttpTransport> {
    session: EndpointSession<T>,
}

impl<T: HttpTransport> TranslatedStreamClient<T> {
    pub fn new(transport: T, provider: Provider, auth: SharedAuthProvider) -> Self {
        Self {
            session: EndpointSession::new(transport, provider, auth),
        }
    }

    pub fn with_telemetry(self, telemetry: Option<Arc<dyn RequestTelemetry>>) -> Self {
        Self {
            session: self.session.with_request_telemetry(telemetry),
        }
    }

    pub async fn stream(
        &self,
        api: TranslatedApi,
        body: &Value,
        headers: HeaderMap,
    ) -> Result<StreamResponse, ApiError> {
        let path = match api {
            TranslatedApi::Chat => "/chat/completions",
            TranslatedApi::Messages => "/messages",
        };
        let mut provider = self.session.provider().clone();
        provider.base_url = codex_client::normalize_translated_base_url(&provider.base_url, path)
            .map_err(|message| ApiError::InvalidRequest { message })?;
        let request_url = provider.url_for_path(path);
        let body = EncodedJsonBody::encode(body).map_err(|error| {
            ApiError::Stream(format!("failed to encode provider request: {error}"))
        })?;
        self.session
            .stream_encoded_json_with(Method::POST, path, headers, Some(body), |request| {
                request.url.clone_from(&request_url);
                request.headers.insert(
                    http::header::ACCEPT,
                    HeaderValue::from_static("text/event-stream"),
                );
            })
            .await
    }
}
