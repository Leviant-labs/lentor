//! Bounded parsing of CommandCode's authoritative model catalog.

use std::collections::BTreeSet;
use std::fmt;

use codex_protocol::provider::CommandCodeRoute;
use serde_json::Value;
use thiserror::Error;

/// Maximum number of models accepted from one catalog response.
pub const MAX_MODELS: usize = 1024;

/// A CommandCode model with its normalized request-route capabilities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCodeModel {
    id: String,
    display_name: Option<String>,
    supported_endpoints: Vec<String>,
    routes: Vec<CommandCodeRoute>,
}

impl CommandCodeModel {
    /// Returns the model identifier used by requests.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the optional human-readable model name.
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }

    /// Returns the endpoint strings reported by CommandCode without rewriting them.
    pub fn supported_endpoints(&self) -> &[String] {
        &self.supported_endpoints
    }

    /// Returns the recognized routes in preference order.
    pub fn routes(&self) -> &[CommandCodeRoute] {
        &self.routes
    }

    /// Selects the most capable route this model supports.
    ///
    /// Responses takes precedence over Chat Completions, which takes precedence
    /// over Anthropic Messages.
    pub fn select_route(&self) -> Result<CommandCodeRoute, RouteError> {
        self.routes.first().copied().ok_or_else(|| RouteError {
            model: self.id.clone(),
            supported_endpoints: self.supported_endpoints.clone(),
        })
    }
}

/// A model catalog parsed from `GET /provider/v1/models`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandCodeCatalog {
    models: Vec<CommandCodeModel>,
    loaded: bool,
}

impl CommandCodeCatalog {
    /// Parses a catalog while enforcing the model-count cap.
    pub fn parse(bytes: &[u8]) -> Result<Self, CatalogError> {
        let value: Value = serde_json::from_slice(bytes)
            .map_err(|error| CatalogError::InvalidJson(error.to_string()))?;
        let entries = value
            .get("models")
            .and_then(Value::as_array)
            .or_else(|| value.get("data").and_then(Value::as_array))
            .ok_or(CatalogError::MissingModelsArray)?;
        if entries.len() > MAX_MODELS {
            return Err(CatalogError::TooManyModels {
                count: entries.len(),
                limit: MAX_MODELS,
            });
        }

        let mut models = Vec::with_capacity(entries.len());
        let mut seen = BTreeSet::new();
        for (index, entry) in entries.iter().enumerate() {
            let model = parse_model(index, entry)?;
            if !seen.insert(model.id.clone()) {
                return Err(CatalogError::DuplicateModel { id: model.id });
            }
            models.push(model);
        }
        Ok(Self {
            models,
            loaded: true,
        })
    }

    /// Returns every parsed model in catalog order.
    pub fn models(&self) -> &[CommandCodeModel] {
        &self.models
    }

    /// Looks up a model by its exact identifier.
    pub fn model(&self, id: &str) -> Option<&CommandCodeModel> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Returns the number of parsed models.
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Returns whether the catalog contains no models.
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }

    /// Describes the state of this catalog.
    ///
    /// A default catalog has never been parsed, so it reports
    /// [`CatalogState::NotLoaded`]. A parsed catalog that happened to contain no
    /// models still reports a loaded state with a zero model count.
    pub fn state(&self) -> CatalogState {
        if self.loaded {
            CatalogState::Loaded {
                models: self.models.len(),
            }
        } else {
            CatalogState::NotLoaded
        }
    }
}

/// Observable state of CommandCode model discovery.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CatalogState {
    /// Discovery has not run yet.
    #[default]
    NotLoaded,
    /// Discovery returned a catalog with the reported number of models.
    Loaded { models: usize },
    /// Discovery failed for the reported, non-secret reason.
    Unavailable { reason: String },
}

/// Errors from parsing a model catalog response.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum CatalogError {
    #[error("model catalog response was not valid JSON: {0}")]
    InvalidJson(String),
    #[error("model catalog response must contain a `models` or `data` array")]
    MissingModelsArray,
    #[error("model catalog contains {count} models, exceeding the {limit}-model limit")]
    TooManyModels { count: usize, limit: usize },
    #[error("model catalog entry {index} is invalid: {reason}")]
    InvalidModel { index: usize, reason: String },
    #[error("model catalog contains duplicate model id `{id}`")]
    DuplicateModel { id: String },
}

/// Error returned when a model exposes no route Codex can implement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteError {
    model: String,
    supported_endpoints: Vec<String>,
}

impl RouteError {
    /// Returns the model that could not be routed.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the endpoints reported by CommandCode.
    pub fn supported_endpoints(&self) -> &[String] {
        &self.supported_endpoints
    }
}

impl fmt::Display for RouteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let endpoints = if self.supported_endpoints.is_empty() {
            "none".to_string()
        } else {
            self.supported_endpoints.join(", ")
        };
        write!(
            f,
            "model `{}` does not support a Codex request route; supported endpoints: {endpoints}",
            self.model
        )
    }
}

impl std::error::Error for RouteError {}

fn parse_model(index: usize, value: &Value) -> Result<CommandCodeModel, CatalogError> {
    let object = value
        .as_object()
        .ok_or_else(|| invalid_model(index, "entry must be an object"))?;
    let id = ["id", "slug", "model", "name"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .ok_or_else(|| invalid_model(index, "entry has no non-empty model id"))?
        .to_string();
    let display_name = ["display_name", "displayName", "name"]
        .iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToOwned::to_owned);

    let mut supported_endpoints = Vec::new();
    let mut routes = Vec::new();
    if let Some(endpoints) = ["supported_endpoints", "supportedEndpoints", "endpoints"]
        .iter()
        .find_map(|key| object.get(*key))
    {
        collect_endpoints(index, endpoints, &mut supported_endpoints, &mut routes)?;
    }
    let mut seen_endpoints = BTreeSet::new();
    supported_endpoints.retain(|endpoint| seen_endpoints.insert(endpoint.clone()));

    Ok(CommandCodeModel {
        id,
        display_name,
        supported_endpoints,
        routes,
    })
}

fn collect_endpoints(
    index: usize,
    value: &Value,
    supported_endpoints: &mut Vec<String>,
    routes: &mut Vec<CommandCodeRoute>,
) -> Result<(), CatalogError> {
    match value {
        Value::Null => Ok(()),
        Value::String(endpoint) => {
            push_endpoint(endpoint, supported_endpoints, routes);
            Ok(())
        }
        Value::Array(entries) => {
            for entry in entries {
                match entry {
                    Value::String(endpoint) => {
                        push_endpoint(endpoint, supported_endpoints, routes);
                    }
                    Value::Object(object) => {
                        let endpoint = ["endpoint", "name", "type", "path"]
                            .iter()
                            .find_map(|key| object.get(*key).and_then(Value::as_str))
                            .ok_or_else(|| {
                                invalid_model(
                                    index,
                                    "supported endpoint object has no endpoint name",
                                )
                            })?;
                        push_endpoint(endpoint, supported_endpoints, routes);
                    }
                    _ => {
                        return Err(invalid_model(
                            index,
                            "supported endpoint entry must be a string or object",
                        ));
                    }
                }
            }
            Ok(())
        }
        _ => Err(invalid_model(
            index,
            "supported_endpoints must be a string, array, or null",
        )),
    }
}

fn push_endpoint(
    endpoint: &str,
    supported_endpoints: &mut Vec<String>,
    routes: &mut Vec<CommandCodeRoute>,
) {
    let endpoint = endpoint.trim();
    if endpoint.is_empty() {
        return;
    }
    supported_endpoints.push(endpoint.to_string());
    if let Some(route) = normalize_route(endpoint)
        && !routes.contains(&route)
    {
        routes.push(route);
        routes.sort_unstable_by_key(|route| route_rank(*route));
    }
}

fn normalize_route(endpoint: &str) -> Option<CommandCodeRoute> {
    let mut normalized = endpoint.trim().trim_start_matches('/').to_ascii_lowercase();
    for prefix in ["provider/v1/", "v1/"] {
        if let Some(stripped) = normalized.strip_prefix(prefix) {
            normalized = stripped.to_string();
            break;
        }
    }
    let normalized = normalized.trim_end_matches('/');
    match normalized {
        "responses" => Some(CommandCodeRoute::Responses),
        "chat/completions" | "chat_completion" | "chat_completions" | "chat-completions" => {
            Some(CommandCodeRoute::ChatCompletions)
        }
        "messages" | "anthropic/messages" | "anthropic_messages" | "anthropic-messages" => {
            Some(CommandCodeRoute::AnthropicMessages)
        }
        _ => None,
    }
}

fn route_rank(route: CommandCodeRoute) -> u8 {
    match route {
        CommandCodeRoute::Responses => 0,
        CommandCodeRoute::ChatCompletions => 1,
        CommandCodeRoute::AnthropicMessages => 2,
    }
}

fn invalid_model(index: usize, reason: &str) -> CatalogError {
    CatalogError::InvalidModel {
        index,
        reason: reason.to_string(),
    }
}
