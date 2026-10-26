use super::ConfigRequestProcessor;
use codex_app_server_protocol::ConfigBatchWriteParams;
use codex_app_server_protocol::ConfigEdit;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::MergeStrategy;
use codex_app_server_protocol::ModelProviderConfigureParams;
use codex_app_server_protocol::ModelProviderConfigureResponse;
use codex_model_provider::delete_provider_api_key;
use codex_model_provider::store_provider_api_key;
use codex_model_provider_info::built_in_model_providers;
use serde_json::json;

use crate::error_code::internal_error;
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
        } = params;
        if provider_id.is_empty()
            || provider_id.len() > 64
            || !provider_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            || built_in_model_providers(/*openai_base_url*/ None).contains_key(&provider_id)
        {
            return Err(invalid_request("choose a non-reserved provider identifier"));
        }
        if name.trim().is_empty() || name.len() > 128 {
            return Err(invalid_request("provider name must contain 1 to 128 bytes"));
        }
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        let model = model.filter(|value| !value.trim().is_empty());
        if model.as_ref().is_some_and(|value| value.len() > 512) {
            return Err(invalid_request("model identifier is too long"));
        }
        // Resolve managed configuration before creating a new credential.
        self.load_latest_config(/*fallback_cwd*/ None).await?;
        let reference = store_provider_api_key(&provider_id, &base_url, &api_key)
            .map_err(|error| invalid_request(error.to_string()))?;
        let edits = vec![
            ConfigEdit {
                key_path: format!("model_providers.{provider_id}"),
                value: json!({
                    "name": name,
                    "base_url": base_url,
                    "keyring_key": reference,
                    "model_catalog_url": format!("{base_url}/models"),
                    "wire_api": "responses",
                    "requires_openai_auth": false,
                    "supports_websockets": false,
                }),
                merge_strategy: MergeStrategy::Replace,
            },
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
                key_path: "features.api_key_model_discovery".to_string(),
                value: json!(true),
                merge_strategy: MergeStrategy::Replace,
            },
        ];
        let write = self
            .batch_write_inner(ConfigBatchWriteParams {
                edits,
                file_path: None,
                expected_version: None,
                reload_user_config: false,
            })
            .await;
        if let Err(error) = write {
            if delete_provider_api_key(&reference).is_err() {
                return Err(internal_error(
                    "provider configuration failed and its new credential could not be removed",
                ));
            }
            return Err(error);
        }
        self.handle_config_mutation().await;
        Ok(ModelProviderConfigureResponse {
            provider_id,
            base_url,
            model,
        })
    }
}
