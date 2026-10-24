use crate::manifest::PluginManifestFormat;
use crate::remote::REMOTE_GLOBAL_MARKETPLACE_NAME;
use codex_plugin::AppDeclaration;
use codex_plugin::PluginId;
use codex_protocol::auth::AuthMode;
use std::collections::HashMap;

pub fn apps_route_available(_auth_mode: Option<AuthMode>) -> bool {
    // This build preserves app declarations but has no ChatGPT Apps runtime.
    false
}

pub(crate) fn plugin_supports_apps(format: PluginManifestFormat, plugin_id: &PluginId) -> bool {
    // The remote curated installer overwrites app declarations with plugin-service metadata.
    // Keep portable app declarations inactive for other Agent Plugins.
    format == PluginManifestFormat::Legacy
        || plugin_id.marketplace_name == REMOTE_GLOBAL_MARKETPLACE_NAME
}

pub(crate) fn apply_app_mcp_routing_policy<M>(
    _apps: &mut Vec<AppDeclaration>,
    _mcp_servers: &mut HashMap<String, M>,
    _auth_mode: Option<AuthMode>,
    _plugin_active: bool,
) {
}

#[cfg(test)]
#[path = "app_mcp_routing_tests.rs"]
mod tests;
