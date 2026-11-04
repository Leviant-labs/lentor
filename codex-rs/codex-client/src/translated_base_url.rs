//! API base normalization shared by provider configuration and transport.

use url::Url;

/// Normalize a base URL, optionally removing the selected translated endpoint suffix.
pub fn normalize_translated_base_url(input: &str, endpoint_suffix: &str) -> Result<String, String> {
    let mut url = Url::parse(input.trim()).map_err(|_| "invalid provider base URL".to_string())?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(
            "provider base URL must not contain credentials, query or fragment".to_string(),
        );
    }
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix(endpoint_suffix).unwrap_or(path);
    let path = if path.is_empty() { "/v1" } else { path }.to_string();
    url.set_path(&path);
    Ok(url.as_str().trim_end_matches('/').to_string())
}
