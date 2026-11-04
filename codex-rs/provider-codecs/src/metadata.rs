use serde_json::Value;

const MAX_METADATA_BYTES: usize = 32 * 1024;

// Gateway accounting/routing metadata is transport information, not a
// conversation item. Normalize only its named extension; reasoning, signatures,
// tools and unknown content fields retain their normal validation paths.
pub(crate) fn strip_provider_metadata(value: &mut Value) -> Result<(), &'static str> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    if let Some(metadata) = object.get("provider_metadata") {
        if !metadata.is_object() && !metadata.is_null() {
            return Err("invalid provider metadata");
        }
        let bytes = serde_json::to_vec(metadata).map_err(|_| "invalid provider metadata")?;
        if bytes.len() > MAX_METADATA_BYTES {
            return Err("provider metadata exceeds limit");
        }
        object.remove("provider_metadata");
    }
    Ok(())
}
