//! Private provider state retained alongside, but never inside, model-visible items.

use std::collections::HashMap;

use codex_history::NativeProviderProtocol;
use codex_history::NativeProviderState;
use codex_history::ResponseItemEnvelope;
use codex_model_provider_info::WireApi;
use codex_protocol::error::CodexErr;
use codex_protocol::error::Result;
use codex_protocol::models::ResponseItem;
use sha2::Digest;
use sha2::Sha256;
use url::Url;

use super::ModelClient;

const MAX_HISTORY_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, Default)]
pub(super) struct NativeStateStore {
    scope: Option<(NativeProviderProtocol, String)>,
    records: HashMap<String, NativeProviderState>,
}

fn invalid(message: &str) -> CodexErr {
    CodexErr::InvalidRequest(message.to_string())
}

fn scope(
    provider_id: &str,
    base_url: &str,
    wire_api: WireApi,
) -> Result<(NativeProviderProtocol, String)> {
    if provider_id.is_empty() {
        return Err(invalid("native provider state requires a provider ID"));
    }
    let protocol = match wire_api {
        WireApi::Chat => NativeProviderProtocol::Chat,
        WireApi::Messages => NativeProviderProtocol::Messages,
        WireApi::Responses => {
            return Err(invalid(
                "native provider state requires a translated protocol",
            ));
        }
    };
    let normalized = codex_model_provider_info::normalize_translated_base_url(base_url, wire_api)
        .map_err(CodexErr::InvalidRequest)?;
    let mut url = Url::parse(&normalized).map_err(|_| invalid("invalid provider base URL"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "provider base URL cannot contain credentials, query or fragment for native state",
        ));
    }
    let path = url.path().trim_end_matches('/').to_string();
    url.set_path(&path);
    let mut digest = Sha256::new();
    for field in [
        "lentor-native-state-v1",
        provider_id,
        url.as_str(),
        match protocol {
            NativeProviderProtocol::Chat => "chat",
            NativeProviderProtocol::Messages => "messages",
        },
    ] {
        digest.update((field.len() as u64).to_le_bytes());
        digest.update(field.as_bytes());
    }
    Ok((protocol, format!("{:x}", digest.finalize())))
}

impl NativeStateStore {
    fn hydrate(
        &mut self,
        scope: (NativeProviderProtocol, String),
        history: &[ResponseItemEnvelope],
    ) -> Result<()> {
        let mut records = HashMap::new();
        let mut size = 0usize;
        for envelope in history {
            let Some(state) = envelope
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.native_provider_state.as_ref())
            else {
                continue;
            };
            if envelope
                .item
                .id()
                .map(codex_protocol::ResponseItemId::as_str)
                != Some(state.item_id())
            {
                return Err(invalid(
                    "native provider state does not match the retained item",
                ));
            }
            if state.protocol() != scope.0 || state.endpoint_hash() != scope.1 {
                return Err(CodexErr::UnsupportedOperation(
                    "This session contains reasoning from another provider. Select the original provider to resume it, or start a new Lentor session with the selected provider.".to_string(),
                ));
            }
            size = size
                .checked_add(
                    serde_json::to_vec(state)
                        .map_err(|_| invalid("invalid native provider state"))?
                        .len(),
                )
                .ok_or_else(|| invalid("native provider history exceeds size limit"))?;
            if size > MAX_HISTORY_BYTES {
                return Err(invalid("native provider history exceeds size limit"));
            }
            if records
                .insert(state.item_id().to_string(), state.clone())
                .is_some()
            {
                return Err(invalid("duplicate native provider item identity"));
            }
        }
        self.scope = Some(scope);
        self.records = records;
        Ok(())
    }

    pub(super) fn insert(&mut self, state: NativeProviderState) -> Result<()> {
        let Some((protocol, hash)) = &self.scope else {
            return Err(invalid(
                "native provider state has no active provider scope",
            ));
        };
        if state.protocol() != *protocol || state.endpoint_hash() != hash {
            return Err(invalid(
                "native provider state does not match the active provider",
            ));
        }
        let mut records = self.records.clone();
        if records.insert(state.item_id().to_string(), state).is_some() {
            return Err(invalid("duplicate native provider item identity"));
        }
        let size = records
            .values()
            .try_fold(0usize, |total, state| {
                total.checked_add(serde_json::to_vec(state).ok()?.len())
            })
            .ok_or_else(|| invalid("invalid native provider state size"))?;
        if size > MAX_HISTORY_BYTES {
            return Err(invalid("native provider history exceeds size limit"));
        }
        self.records = records;
        Ok(())
    }

    fn insert_batch(&mut self, states: Vec<NativeProviderState>) -> Result<()> {
        let mut updated = self.clone();
        for state in states {
            updated.insert(state)?;
        }
        *self = updated;
        Ok(())
    }
}

impl ModelClient {
    pub(crate) fn native_records(&self) -> Vec<codex_provider_codecs::NativeRecord> {
        self.state
            .native_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .records
            .values()
            .map(|state| codex_provider_codecs::NativeRecord {
                item_id: state.item_id().to_string(),
                payload: state.payload().clone(),
            })
            .collect()
    }
    pub(crate) fn native_state_scope(&self) -> Option<(NativeProviderProtocol, String)> {
        self.state
            .native_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .scope
            .clone()
    }

    pub(crate) fn insert_native_states(&self, states: Vec<NativeProviderState>) -> Result<()> {
        self.state
            .native_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert_batch(states)
    }

    pub(crate) fn hydrate_native_state(
        &self,
        provider_id: &str,
        base_url: &str,
        wire_api: WireApi,
        history: &[ResponseItemEnvelope],
    ) -> Result<()> {
        let scope = scope(provider_id, base_url, wire_api)?;
        self.state
            .native_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .hydrate(scope, history)
    }

    pub(crate) fn native_state_for_item(
        &self,
        provider_id: &str,
        base_url: &str,
        wire_api: WireApi,
        item: &ResponseItem,
    ) -> Result<Option<NativeProviderState>> {
        let scope = scope(provider_id, base_url, wire_api)?;
        let store = self
            .state
            .native_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if store.scope.as_ref() != Some(&scope) {
            return Err(invalid(
                "native provider state scope changed during response",
            ));
        }
        Ok(item
            .id()
            .and_then(|id| store.records.get(id.as_str()).cloned()))
    }
}

#[cfg(test)]
#[path = "native_state_tests.rs"]
mod tests;
