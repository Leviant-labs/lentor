//! Bounded host-only state for replaying translated provider messages.

use schemars::JsonSchema;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

const MAX_STATE_BYTES: usize = 32 * 1024;
const MAX_ITEM_ID_BYTES: usize = 512;

/// Protocol which produced a private native record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NativeProviderProtocol {
    Chat,
    Messages,
}

/// A native record scoped to a protocol, hashed endpoint identity and response item.
/// The payload is persisted beside the canonical item, never in its public contents.
#[derive(Clone, PartialEq, Eq, Serialize, JsonSchema)]
pub struct NativeProviderState {
    protocol: NativeProviderProtocol,
    endpoint_hash: String,
    item_id: String,
    payload: Value,
}

impl fmt::Debug for NativeProviderState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeProviderState")
            .field("protocol", &self.protocol)
            .field("endpoint_hash", &self.endpoint_hash)
            .field("item_id", &self.item_id)
            .field("payload", &"[redacted]")
            .finish()
    }
}

impl NativeProviderState {
    /// Validates all bounds before a provider record is retained or replayed.
    pub fn new(
        protocol: NativeProviderProtocol,
        endpoint_hash: String,
        item_id: String,
        payload: Value,
    ) -> Result<Self, String> {
        if endpoint_hash.len() != 64 || !endpoint_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err("invalid native provider endpoint identity".into());
        }
        if item_id.is_empty() || item_id.len() > MAX_ITEM_ID_BYTES {
            return Err("invalid native provider item ID".into());
        }
        if serde_json::to_vec(&payload)
            .map_err(|_| "invalid native provider payload")?
            .len()
            > MAX_STATE_BYTES
        {
            return Err("native provider payload exceeds size limit".into());
        }
        Ok(Self {
            protocol,
            endpoint_hash,
            item_id,
            payload,
        })
    }

    pub fn protocol(&self) -> NativeProviderProtocol {
        self.protocol
    }
    pub fn endpoint_hash(&self) -> &str {
        &self.endpoint_hash
    }
    pub fn item_id(&self) -> &str {
        &self.item_id
    }
    pub fn payload(&self) -> &Value {
        &self.payload
    }
}

impl<'de> Deserialize<'de> for NativeProviderState {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            protocol: NativeProviderProtocol,
            endpoint_hash: String,
            item_id: String,
            payload: Value,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.protocol,
            wire.endpoint_hash,
            wire.item_id,
            wire.payload,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[path = "native_provider_state_tests.rs"]
mod tests;
