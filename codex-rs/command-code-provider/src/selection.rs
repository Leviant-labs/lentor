//! Provider precedence and model-route selection for CommandCode.

use std::collections::HashMap;

use codex_model_provider_info::COMMAND_CODE_PROVIDER_ID;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::provider::CommandCodeRoute;
use thiserror::Error;

use crate::catalog::CatalogState;
use crate::catalog::CommandCodeCatalog;
use crate::catalog::RouteError;

/// Selected provider, model, route, and catalog state.
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderSelection {
    /// Registry key of the selected provider.
    pub provider_id: String,
    /// Provider metadata that will be used for requests.
    pub info: ModelProviderInfo,
    /// Configured or discovered model identifier.
    pub model: Option<String>,
    /// Route selected for the model, when discovery supplied it.
    pub route: Option<CommandCodeRoute>,
    /// State of the CommandCode catalog used for this selection.
    pub catalog: CatalogState,
}

impl ProviderSelection {
    /// Returns the provider status exposed through the model-provider boundary.
    pub fn status(&self) -> CommandCodeProviderStatus {
        CommandCodeProviderStatus {
            provider_id: self.provider_id.clone(),
            model: self.model.clone(),
            route: self.route,
            catalog: self.catalog.clone(),
        }
    }
}

/// Provider identity and catalog state safe to expose to clients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCodeProviderStatus {
    /// Registry key of the selected provider.
    pub provider_id: String,
    /// Configured or discovered model identifier.
    pub model: Option<String>,
    /// Route selected for the model, when discovery supplied it.
    pub route: Option<CommandCodeRoute>,
    /// State of the CommandCode catalog used for this selection.
    pub catalog: CatalogState,
}

/// Errors from resolving provider precedence or a model route.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SelectionError {
    #[error("model provider `{0}` is not configured")]
    UnknownProvider(String),
    #[error("model `{0}` is not present in the CommandCode catalog")]
    UnknownModel(String),
    #[error(transparent)]
    UnsupportedRoute(#[from] RouteError),
}

/// Selects the configured provider, or CommandCode when no provider is set.
///
/// This function performs no network I/O. Explicit providers other than
/// `commandcode` are authoritative and ignore the supplied catalog.
pub fn select_provider(
    configured_provider: Option<&str>,
    configured_model: Option<&str>,
    providers: &HashMap<String, ModelProviderInfo>,
    catalog: &CommandCodeCatalog,
) -> Result<ProviderSelection, SelectionError> {
    let model = configured_model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(ToOwned::to_owned);
    if let Some(provider_id) = configured_provider
        .map(str::trim)
        .filter(|provider_id| !provider_id.is_empty())
    {
        let info = providers
            .get(provider_id)
            .cloned()
            .ok_or_else(|| SelectionError::UnknownProvider(provider_id.to_string()))?;
        if provider_id == COMMAND_CODE_PROVIDER_ID {
            let route = resolve_route(model.as_deref(), catalog)?;
            return Ok(ProviderSelection {
                provider_id: provider_id.to_string(),
                info,
                model,
                route,
                catalog: catalog.state(),
            });
        }
        return Ok(ProviderSelection {
            provider_id: provider_id.to_string(),
            info,
            model,
            route: None,
            catalog: CatalogState::NotLoaded,
        });
    }

    let info = providers
        .get(COMMAND_CODE_PROVIDER_ID)
        .cloned()
        .ok_or_else(|| SelectionError::UnknownProvider(COMMAND_CODE_PROVIDER_ID.to_string()))?;
    let route = resolve_route(model.as_deref(), catalog)?;
    Ok(ProviderSelection {
        provider_id: COMMAND_CODE_PROVIDER_ID.to_string(),
        info,
        model,
        route,
        catalog: catalog.state(),
    })
}

fn resolve_route(
    model: Option<&str>,
    catalog: &CommandCodeCatalog,
) -> Result<Option<CommandCodeRoute>, SelectionError> {
    let Some(model) = model else {
        return Ok(None);
    };
    let candidate = catalog
        .model(model)
        .ok_or_else(|| SelectionError::UnknownModel(model.to_string()))?;
    candidate.select_route().map(Some).map_err(Into::into)
}

/// Returns the provider-relative path for a CommandCode route.
pub fn route_path(route: CommandCodeRoute) -> &'static str {
    match route {
        CommandCodeRoute::Responses => "/provider/v1/responses",
        CommandCodeRoute::ChatCompletions => "/provider/v1/chat/completions",
        CommandCodeRoute::AnthropicMessages => "/provider/v1/messages",
    }
}

/// Joins a CommandCode provider base URL with the selected route.
pub fn route_endpoint(base_url: &str, route: CommandCodeRoute) -> String {
    let base_url = base_url.trim_end_matches('/');
    match route {
        CommandCodeRoute::Responses => format!("{base_url}/responses"),
        CommandCodeRoute::ChatCompletions => format!("{base_url}/chat/completions"),
        CommandCodeRoute::AnthropicMessages => format!("{base_url}/messages"),
    }
}
