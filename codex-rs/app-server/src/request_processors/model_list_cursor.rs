//! Versioned offsets reject continuation against a changed catalog or identity.

use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::Model;
use codex_app_server_protocol::ModelListParams;
use codex_core::config::Config;
use codex_models_manager::manager::SharedModelsManager;
use sha2::Digest;
use sha2::Sha256;

use crate::error_code::internal_error;
use crate::error_code::invalid_request;

pub(super) struct ModelListCursor {
    pub(super) version: String,
    pub(super) offset: usize,
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
                return Ok(Self { version: (*version).to_string(), offset });
            }
        }
        Err(invalid_request(format!("invalid cursor: {}", cursor.chars().take(96).collect::<String>())))
    }

    pub(super) fn version(
        config: &Config,
        manager: &SharedModelsManager,
        models: &[Model],
        params: &ModelListParams,
    ) -> Result<String, JSONRPCErrorError> {
        let auth_generation = manager.auth_manager()
            .map(|auth| *auth.auth_change_receiver().borrow());
        // Keyring references and endpoint configuration bind provider credentials;
        // the manager/auth generation also invalidates account transitions. Hash
        // serialized config directly: never return or log its credential fields.
        let identity = serde_json::json!({
            "providerId": config.model_provider_id,
            "provider": config.model_provider,
            "manager": format!("{:p}", std::sync::Arc::as_ptr(manager)),
            "authGeneration": auth_generation,
            "includeHidden": params.include_hidden.unwrap_or(false),
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
