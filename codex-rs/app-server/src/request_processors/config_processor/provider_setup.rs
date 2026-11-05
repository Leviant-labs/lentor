use super::ConfigRequestProcessor;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::ModelProviderConfigureParams;
use codex_app_server_protocol::ModelProviderConfigureResponse;
use codex_app_server_protocol::ModelProviderWireApi;
use codex_app_server_protocol::WriteStatus;
use codex_features::Feature;
use codex_model_provider::store_provider_api_key;
use codex_model_provider_info::built_in_model_providers;
use serde_json::json;

use crate::error_code::invalid_request;

impl ConfigRequestProcessor {
    pub(crate) async fn configure_model_provider(
        &self,
        params: ModelProviderConfigureParams,
    ) -> Result<ModelProviderConfigureResponse, JSONRPCErrorError> {
        let ModelProviderConfigureParams {
            provider_id,
            name,
            base_url,
            api_key,
            model,
            wire_api,
        } = params;
        let wire_api = match wire_api.unwrap_or(ModelProviderWireApi::Responses) {
            ModelProviderWireApi::Responses => codex_model_provider_info::WireApi::Responses,
            ModelProviderWireApi::Chat => codex_model_provider_info::WireApi::Chat,
            ModelProviderWireApi::Messages => codex_model_provider_info::WireApi::Messages,
        };
        if provider_id.is_empty()
            || provider_id.len() > 64
            || !provider_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || built_in_model_providers(/*openai_base_url*/ None).contains_key(&provider_id)
        {
            return Err(with_outcome(
                invalid_request("choose a non-reserved provider identifier"),
                "unchanged",
            ));
        }
        if name.trim().is_empty() || name.len() > 128 {
            return Err(with_outcome(
                invalid_request("provider name must contain 1 to 128 bytes"),
                "unchanged",
            ));
        }
        let base_url = if wire_api == codex_model_provider_info::WireApi::Responses {
            base_url.trim().trim_end_matches('/').to_string()
        } else {
            codex_model_provider_info::normalize_translated_base_url(&base_url, wire_api)
                .map_err(|error| with_outcome(invalid_request(error), "unchanged"))?
        };
        let model = model.filter(|value| !value.trim().is_empty());
        if model.as_ref().is_some_and(|value| value.len() > 512) {
            return Err(with_outcome(
                invalid_request("model identifier is too long"),
                "unchanged",
            ));
        }
        // Resolve managed configuration before creating a new credential.
        self.load_latest_config(/*fallback_cwd*/ None)
            .await
            .map_err(|error| with_outcome(error, "unchanged"))?;
        let reference = store_provider_api_key(&provider_id, &base_url, &api_key)
            .map_err(|error| with_outcome(invalid_request(error.to_string()), "unchanged"))?;
        // Edit only setup-owned fields in the user layer. Replacing the provider
        // table discards custom settings; copying effective config could persist
        // managed secrets into the user file.
        let mut edits: Vec<ConfigEdit> = [
            ("name", json!(name)),
            ("base_url", json!(base_url)),
            ("keyring_key", json!(reference)),
            ("model_catalog_url", json!(format!("{base_url}/models"))),
            ("model_catalog_format", json!("openai")),
            ("wire_api", json!(wire_api)),
            ("requires_openai_auth", json!(false)),
            ("env_key", serde_json::Value::Null),
            ("env_key_instructions", serde_json::Value::Null),
            ("experimental_bearer_token", serde_json::Value::Null),
            ("auth", serde_json::Value::Null),
            ("aws", serde_json::Value::Null),
        ]
        .into_iter()
        .map(|(field, value)| ConfigEdit {
            key_path: format!("model_providers.{provider_id}.{field}"),
            value,
            merge_strategy: MergeStrategy::Replace,
        })
        .collect();
        edits.extend([
            ConfigEdit {
                key_path: "model_provider".to_string(),
                value: json!(provider_id),
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "model".to_string(),
                value: json!(model),
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "model_reasoning_effort".to_string(),
                value: serde_json::Value::Null,
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "plan_mode_reasoning_effort".to_string(),
                value: serde_json::Value::Null,
                merge_strategy: MergeStrategy::Replace,
            },
            ConfigEdit {
                key_path: "features.api_key_model_discovery".to_string(),
                value: json!(true),
                merge_strategy: MergeStrategy::Replace,
            },
        ]);
        let write = self
            .batch_write_inner(ConfigBatchWriteParams {
                edits,
                file_path: None,
                expected_version: None,
                reload_user_config: false,
            })
            .await;
        // A failed batch write can occur after persistence. Keep the credential
        // until the persisted reference can be proven absent.
        let write = write.map_err(|error| with_reference(error, "unknown", &reference))?;
        self.handle_config_mutation().await;
        self.model_catalog.invalidate().await;
        // A successful write is durable even when a higher layer overrides it. Keep
        // its credential: deleting it would leave the saved reference dangling.
        // Previous references also remain available to existing and resumed threads;
        // safe reclamation requires tracking references across processes and sessions.
        let effective = self
            .load_latest_config(/*fallback_cwd*/ None)
            .await
            .map_err(|error| with_reference(error, "saved", &reference))?;
        if write.status == WriteStatus::OkOverridden
            || effective.model_provider_id != provider_id
            || effective.model_provider.base_url.as_deref() != Some(base_url.as_str())
            || effective.model_provider.keyring_key.as_deref() != Some(reference.as_str())
            || effective.model_provider.wire_api != wire_api
            || effective.model != model
            || !effective.features.enabled(Feature::ApiKeyModelDiscovery)
        {
            return Err(with_reference(
                invalid_request(
                    "provider settings were saved but are not effective; check managed configuration or command-line overrides before retrying",
                ),
                "saved",
                &reference,
            ));
        }
        Ok(ModelProviderConfigureResponse {
            provider_id,
            base_url,
            model,
        })
    }
}

fn with_outcome(mut error: JSONRPCErrorError, outcome: &str) -> JSONRPCErrorError {
    error.data = Some(json!({"providerConfigurationOutcome": outcome, "cause": error.data}));
    error
}

fn with_reference(error: JSONRPCErrorError, outcome: &str, reference: &str) -> JSONRPCErrorError {
    let mut error = with_outcome(error, outcome);
    if let Some(data) = error
        .data
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
    {
        data.insert("providerCredentialReference".to_string(), json!(reference));
    }
    error
}
