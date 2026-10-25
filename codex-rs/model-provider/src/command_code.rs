//! Model-provider boundary for direct CommandCode discovery and route selection.

use std::collections::HashMap;
use std::sync::RwLock;
use std::sync::RwLockReadGuard;
use std::sync::RwLockWriteGuard;

use codex_command_code_provider::CatalogState;
use codex_command_code_provider::CommandCodeCatalog;
use codex_command_code_provider::CommandCodeDiscovery;
use codex_command_code_provider::CommandCodeProviderStatus;
use codex_command_code_provider::ProviderSelection;
use codex_command_code_provider::SelectionError;
use codex_command_code_provider::route_endpoint;
use codex_command_code_provider::select_provider;
use codex_model_provider_info::COMMAND_CODE_PROVIDER_ID;
use codex_model_provider_info::ModelProviderInfo;

/// Discovery and route-selection state for the direct CommandCode provider.
///
/// The boundary owns the last parsed catalog so the selected provider, model,
/// route, and catalog state can be reported without another network request.
/// Constructing it never contacts CommandCode; only [refresh](Self::refresh)
/// performs I/O.
#[derive(Debug)]
pub struct CommandCodeProviderBoundary {
    info: ModelProviderInfo,
    discovery: CommandCodeDiscovery,
    state: RwLock<BoundaryState>,
}

#[derive(Debug, Default)]
struct BoundaryState {
    catalog: CommandCodeCatalog,
    catalog_state: CatalogState,
    selection: Option<ProviderSelection>,
}

impl CommandCodeProviderBoundary {
    /// Creates a boundary for provider metadata whose base URL hosts the model catalog.
    pub(crate) fn new(info: ModelProviderInfo) -> Self {
        let discovery = info
            .base_url
            .as_deref()
            .map(CommandCodeDiscovery::for_base_url)
            .unwrap_or_default();
        Self {
            info,
            discovery,
            state: RwLock::default(),
        }
    }

    /// Returns the configured CommandCode provider metadata.
    pub fn info(&self) -> &ModelProviderInfo {
        &self.info
    }

    /// Returns the observed state of CommandCode model discovery.
    pub fn catalog_state(&self) -> CatalogState {
        read(&self.state).catalog_state.clone()
    }

    /// Returns the last successful selection, when one was resolved.
    pub fn selection(&self) -> Option<ProviderSelection> {
        read(&self.state).selection.clone()
    }

    /// Returns the provider, model, route, and catalog state exposed to clients.
    pub fn status(&self) -> CommandCodeProviderStatus {
        let state = read(&self.state);
        match state.selection.as_ref() {
            Some(selection) => selection.status(),
            None => CommandCodeProviderStatus {
                provider_id: COMMAND_CODE_PROVIDER_ID.to_string(),
                model: None,
                route: None,
                catalog: state.catalog_state.clone(),
            },
        }
    }

    /// Fetches the model catalog and records an observable state for the result.
    ///
    /// A refresh invalidates the previous selection because a catalog can change
    /// which routes a model supports. Call [select](Self::select) again before
    /// using a route.
    pub async fn refresh(&self, credential: Option<&str>) -> CatalogState {
        let (catalog, catalog_state) = match self.discovery.discover(credential).await {
            Ok(catalog) => {
                let catalog_state = catalog.state();
                (catalog, catalog_state)
            }
            Err(error) => (
                CommandCodeCatalog::default(),
                CatalogState::Unavailable {
                    reason: error.to_string(),
                },
            ),
        };
        *write(&self.state) = BoundaryState {
            catalog,
            catalog_state: catalog_state.clone(),
            selection: None,
        };
        catalog_state
    }

    /// Resolves the route for a configured model against the discovered catalog.
    ///
    /// This performs no network I/O. A failed resolution clears the previous
    /// selection so callers never observe a stale route.
    pub fn select(&self, model: Option<&str>) -> Result<CommandCodeProviderStatus, SelectionError> {
        let providers = HashMap::from([(COMMAND_CODE_PROVIDER_ID.to_string(), self.info.clone())]);
        let mut state = write(&self.state);
        let selection = match select_provider(
            Some(COMMAND_CODE_PROVIDER_ID),
            model,
            &providers,
            &state.catalog,
        ) {
            Ok(selection) => selection,
            Err(error) => {
                state.selection = None;
                return Err(error);
            }
        };
        let status = selection.status();
        state.selection = Some(selection);
        Ok(status)
    }

    /// Returns the absolute endpoint for the selected route, when one is selected.
    pub fn selected_endpoint(&self) -> Option<String> {
        let base_url = self.info.base_url.as_deref()?;
        let route = read(&self.state).selection.as_ref()?.route?;
        Some(route_endpoint(base_url, route))
    }
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
#[path = "command_code_tests.rs"]
mod tests;
