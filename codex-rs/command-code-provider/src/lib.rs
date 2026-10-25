//! Direct CommandCode model discovery and route selection.
//!
//! Discovery reads the provider's authoritative `/models` catalog, parses sparse
//! model metadata, and normalizes `supported_endpoints` into the shared
//! [`CommandCodeRoute`] type before any request is constructed.

mod catalog;
mod discovery;
mod selection;

pub use catalog::CatalogError;
pub use catalog::CatalogState;
pub use catalog::CommandCodeCatalog;
pub use catalog::CommandCodeModel;
pub use catalog::MAX_MODELS;
pub use catalog::RouteError;
pub use discovery::CommandCodeDiscovery;
pub use discovery::DiscoveryError;
pub use discovery::MAX_MODEL_CATALOG_BYTES;
pub use discovery::MODELS_TIMEOUT;
pub use selection::CommandCodeProviderStatus;
pub use selection::ProviderSelection;
pub use selection::SelectionError;
pub use selection::route_endpoint;
pub use selection::route_path;
pub use selection::select_provider;

#[cfg(test)]
#[path = "catalog_tests.rs"]
mod catalog_tests;

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod discovery_tests;

#[cfg(test)]
#[path = "selection_tests.rs"]
mod selection_tests;
