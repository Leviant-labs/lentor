use std::collections::BTreeMap;

use respan_gateway_conversation::Block;
use respan_gateway_conversation::Delta;
use respan_gateway_conversation::Output;
use respan_gateway_conversation::block_fields;
use serde_json::Map;
use serde_json::Value;
use serde_json::json;

use crate::NormalizeError;

const MAX_NATIVE_BYTES: usize = 1024 * 1024;
const MAX_NATIVE_BLOCKS: usize = 64;
const MAX_RECORD_BYTES: usize = 32 * 1024;

/// An Anthropic response's native thinking blocks, assembled without losing
/// fragmented signatures. Persist this as opaque JSON with the response item.
#[derive(Default)]
pub struct AnthropicNativeState {
    blocks: BTreeMap<u32, NativeBlock>,
    bytes: usize,
    pending_fields: BTreeMap<u32, Map<String, Value>>,
    failed: bool,
}

#[derive(Default)]
struct NativeBlock {
    thinking: String,
    signature: Option<String>,
    redacted: Option<Value>,
    fields: Map<String, Value>,
}

impl AnthropicNativeState {
    /// Incorporate one event returned by the pinned Respan decoder.
    pub fn push(&mut self, event: &Output) -> Result<(), NormalizeError> {
        if self.failed {
            return Err(NormalizeError::new("native state already failed"));
        }
        let result = self.push_inner(event);
        self.failed |= result.is_err();
        result
    }

    fn push_inner(&mut self, event: &Output) -> Result<(), NormalizeError> {
        match event {
            Output::BlockStart {
                block,
                kind: Block::Reasoning,
            } => {
                let fields = self.pending_fields.remove(block).unwrap_or_default();
                self.block(*block)?.fields.extend(fields);
            }
            Output::Delta {
                block,
                delta: Delta::Reasoning(text),
            } => {
                self.add_bytes(text.len())?;
                self.block(*block)?.thinking.push_str(text);
            }
            Output::Native(native) if native.format.as_ref() == "anthropic-messages" => {
                if let Some((block, fields)) = block_fields(&native.value) {
                    if fields.contains_key("type") || fields.contains_key("thinking") {
                        return Err(NormalizeError::new(
                            "native thinking field conflicts with canonical field",
                        ));
                    }
                    let size = serde_json::to_vec(fields)
                        .map_err(|_| NormalizeError::new("invalid native fields"))?
                        .len();
                    self.add_bytes(size)?;
                    if let Some(signature) = fields.get("signature") {
                        let fragment = signature
                            .as_str()
                            .ok_or_else(|| NormalizeError::new("invalid thinking signature"))?;
                        self.block(block)?
                            .signature
                            .get_or_insert_default()
                            .push_str(fragment);
                    }
                    let extra = fields
                        .iter()
                        .filter(|(key, _)| *key != "signature")
                        .map(|(key, value)| (key.clone(), value.clone()));
                    if let Some(existing) = self.blocks.get_mut(&block) {
                        for (key, value) in extra {
                            if existing.fields.insert(key, value).is_some() {
                                return Err(NormalizeError::new("duplicate native thinking field"));
                            }
                        }
                    } else {
                        if !self.pending_fields.contains_key(&block)
                            && self.pending_fields.len() >= MAX_NATIVE_BLOCKS
                        {
                            return Err(NormalizeError::new("too many native field blocks"));
                        }
                        let pending = self.pending_fields.entry(block).or_default();
                        for (key, value) in extra {
                            if pending.insert(key, value).is_some() {
                                return Err(NormalizeError::new("duplicate native thinking field"));
                            }
                        }
                    }
                } else if native.value["type"] == "content_block_start"
                    && native.value["content_block"]["type"] == "redacted_thinking"
                {
                    let block = native.value["index"]
                        .as_u64()
                        .and_then(|i| u32::try_from(i).ok())
                        .ok_or_else(|| NormalizeError::new("invalid redacted thinking index"))?;
                    let value = native.value["content_block"].clone();
                    let bytes = serde_json::to_vec(&value)
                        .map_err(|_| NormalizeError::new("invalid native thinking"))?;
                    self.add_bytes(bytes.len())?;
                    let block = self.block(block)?;
                    if block.redacted.replace(value).is_some() {
                        return Err(NormalizeError::new("duplicate redacted thinking block"));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Return native content blocks in provider order, suitable for validated
    /// same-provider replay. Incomplete signed thinking is rejected.
    pub fn finish(self) -> Result<Vec<Value>, NormalizeError> {
        if self.failed {
            return Err(NormalizeError::new("native state already failed"));
        }
        if !self.pending_fields.is_empty() {
            return Err(NormalizeError::new("native fields have no reasoning block"));
        }
        let blocks: Vec<Value> = self
            .blocks
            .into_values()
            .map(|block| {
                if let Some(redacted) = block.redacted {
                    if !block.thinking.is_empty()
                        || !block.signature.as_deref().unwrap_or_default().is_empty()
                    {
                        return Err(NormalizeError::new("conflicting native thinking block"));
                    }
                    Ok(redacted)
                } else {
                    let mut value = block.fields;
                    value.insert("type".into(), json!("thinking"));
                    value.insert("thinking".into(), json!(block.thinking));
                    if let Some(signature) = block.signature {
                        if signature.is_empty() {
                            return Err(NormalizeError::new("missing thinking signature"));
                        }
                        value.insert("signature".into(), json!(signature));
                    }
                    Ok(Value::Object(value))
                }
            })
            .collect::<Result<_, _>>()?;
        if serde_json::to_vec(&blocks)
            .map_err(|_| NormalizeError::new("invalid native state"))?
            .len()
            > MAX_RECORD_BYTES
        {
            return Err(NormalizeError::new("native state exceeds 32 KiB"));
        }
        Ok(blocks)
    }

    fn block(&mut self, index: u32) -> Result<&mut NativeBlock, NormalizeError> {
        if !self.blocks.contains_key(&index) && self.blocks.len() >= MAX_NATIVE_BLOCKS {
            return Err(NormalizeError::new("too many native thinking blocks"));
        }
        Ok(self.blocks.entry(index).or_default())
    }

    fn add_bytes(&mut self, size: usize) -> Result<(), NormalizeError> {
        self.bytes = self
            .bytes
            .checked_add(size)
            .ok_or_else(|| NormalizeError::new("native state size overflow"))?;
        if self.bytes > MAX_NATIVE_BYTES {
            return Err(NormalizeError::new("native state exceeds 1 MiB"));
        }
        Ok(())
    }
}
