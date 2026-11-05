//! Normalize translated API endpoints to the common base used for requests and credentials.

use crate::WireApi;

/// Returns the API base for a Chat or Messages endpoint.
pub fn normalize_translated_base_url(input: &str, protocol: WireApi) -> Result<String, String> {
    let suffix = match protocol {
        WireApi::Chat => "/chat/completions",
        WireApi::Messages => "/messages",
        WireApi::Responses => {
            return Err("Responses endpoints do not use translated base normalization".to_string());
        }
    };
    codex_client::normalize_translated_base_url(input, suffix)
}

#[cfg(test)]
#[path = "translated_base_url_tests.rs"]
mod tests;
