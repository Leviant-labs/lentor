//! Versioned offsets reject continuation against a changed catalog or identity.

use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelListParams;
use codex_core::config::Config;
use codex_models_manager::manager::SharedModelsManager;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use std::sync::Arc;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;

pub(super) struct ModelListCursor {
    pub(super) version: String,
    pub(super) offset: usize,
}

#[derive(Debug, Eq, PartialEq, Serialize)]
struct ProviderIdentity {
    provider_id: String,
    keyring_reference: Option<String>,
    config_generation: String,
    manager_generation: String,
}

impl ProviderIdentity {
    fn new(config: &Config, config_generation: String, manager_generation: String) -> Self {
        Self {
            provider_id: config.model_provider_id.clone(),
            // This is a random nonsecret configured reference, not credential material.
            keyring_reference: config.model_provider.keyring_key.clone(),
            config_generation,
            manager_generation,
        }
    }
}

impl ModelListCursor {
    pub(super) fn parse(cursor: &str) -> Result<Self, JSONRPCErrorError> {
        // Bound parsing and diagnostics even for caller-supplied cursor strings.
        if cursor.len() <= 96 {
            let parts: Vec<_> = cursor.split(':').collect();
            if let ["v1", version, offset] = parts.as_slice()
                && version.len() == 64
                && version.bytes().all(|byte| byte.is_ascii_hexdigit())
                && let Ok(offset) = offset.parse::<usize>()
            {
                return Ok(Self {
                    version: (*version).to_string(),
                    offset,
                });
            }
        }
        Err(invalid_request(format!(
            "invalid cursor: {}",
            cursor.chars().take(96).collect::<String>()
        )))
    }

    pub(super) fn version(
        config: &Arc<Config>,
        manager: &SharedModelsManager,
        models: &[Model],
        params: &ModelListParams,
    ) -> Result<String, JSONRPCErrorError> {
        let auth_generation = manager
            .auth_manager()
            .map(|auth| *auth.auth_change_receiver().borrow());
        // ModelCatalog allocates a new config Arc on each invalidated reload,
        // rebuilds the manager for provider/catalog changes, and AuthManager
        // increments auth_generation on credential transitions. Keep the full
        // provider outside this identity: base URLs, headers, query parameters,
        // bearer values, and environment-derived auth are credential-bearing.
        let provider = ProviderIdentity::new(
            config,
            format!("{:p}", Arc::as_ptr(config)),
            format!("{:p}", Arc::as_ptr(manager)),
        );
        Self::version_with_provider(
            &provider,
            auth_generation,
            params.include_hidden.unwrap_or(false),
            models,
        )
    }

    fn version_with_provider<T: Serialize + ?Sized>(
        provider: &ProviderIdentity,
        auth_generation: Option<u64>,
        include_hidden: bool,
        models: &T,
    ) -> Result<String, JSONRPCErrorError> {
        let identity = serde_json::json!({
            "provider": provider,
            "authGeneration": auth_generation,
            "includeHidden": include_hidden,
            "models": models,
        });
        let bytes = serde_json::to_vec(&identity)
            .map_err(|_| internal_error("failed to version model catalog"))?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }
}

pub(super) fn invalidated() -> JSONRPCErrorError {
    invalid_request("model list cursor is no longer valid; restart pagination")
}

#[cfg(test)]
#[path = "model_list_cursor_tests.rs"]
mod tests;
