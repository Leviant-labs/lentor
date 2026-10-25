use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::built_in_model_providers;
use codex_protocol::provider::CommandCodeRoute;
use pretty_assertions::assert_eq;

use crate::catalog::CatalogState;
use crate::catalog::CommandCodeCatalog;
use crate::selection::ProviderSelection;
use crate::selection::SelectionError;
use crate::selection::route_endpoint;
use crate::selection::route_path;
use crate::selection::select_provider;

const CATALOG: &[u8] = br#"{
  "models": [
    {"id": "commandcode-pro", "supported_endpoints": ["messages", "responses"]},
    {"id": "commandcode-chat", "supported_endpoints": ["chat/completions"]},
    {"id": "commandcode-embeddings", "supported_endpoints": ["embeddings"]}
  ]
}"#;

fn providers() -> std::collections::HashMap<String, ModelProviderInfo> {
    built_in_model_providers(/*openai_base_url*/ None)
}

fn catalog() -> CommandCodeCatalog {
    CommandCodeCatalog::parse(CATALOG).expect("catalog should parse")
}

#[test]
fn unset_provider_selects_commandcode_with_the_discovered_route() {
    let providers = providers();

    let selection = select_provider(
        /*configured_provider*/ None,
        Some("commandcode-pro"),
        &providers,
        &catalog(),
    )
    .expect("selection should resolve");

    assert_eq!(
        selection,
        ProviderSelection {
            provider_id: "commandcode".to_string(),
            info: ModelProviderInfo::create_command_code_provider(),
            model: Some("commandcode-pro".to_string()),
            route: Some(CommandCodeRoute::Responses),
            catalog: CatalogState::Loaded { models: 3 },
        }
    );
    assert_eq!(
        selection.info.base_url.as_deref(),
        Some("https://api.commandcode.ai/provider/v1")
    );
}

#[test]
fn blank_provider_and_model_are_treated_as_unset() {
    let providers = providers();

    let selection = select_provider(
        Some("  "),
        Some(""),
        &providers,
        &CommandCodeCatalog::default(),
    )
    .expect("selection should resolve");

    assert_eq!(
        selection,
        ProviderSelection {
            provider_id: "commandcode".to_string(),
            info: ModelProviderInfo::create_command_code_provider(),
            model: None,
            route: None,
            catalog: CatalogState::NotLoaded,
        }
    );
}

#[test]
fn explicit_provider_is_authoritative_without_commandcode_state() {
    let providers = providers();

    let selection = select_provider(
        Some("openai"),
        Some("gpt-5.5"),
        &providers,
        &CommandCodeCatalog::default(),
    )
    .expect("explicit provider should resolve without a catalog");

    assert_eq!(
        selection,
        ProviderSelection {
            provider_id: "openai".to_string(),
            info: providers
                .get("openai")
                .expect("openai provider should be built in")
                .clone(),
            model: Some("gpt-5.5".to_string()),
            route: None,
            catalog: CatalogState::NotLoaded,
        }
    );
}

#[test]
fn explicit_commandcode_provider_uses_the_catalog() {
    let providers = providers();

    let selection = select_provider(
        Some("commandcode"),
        Some("commandcode-chat"),
        &providers,
        &catalog(),
    )
    .expect("explicit commandcode provider should resolve");

    assert_eq!(
        (
            selection.provider_id.as_str(),
            selection.route,
            selection.catalog,
        ),
        (
            "commandcode",
            Some(CommandCodeRoute::ChatCompletions),
            CatalogState::Loaded { models: 3 },
        )
    );
}

#[test]
fn unknown_provider_is_rejected() {
    assert_eq!(
        select_provider(
            Some("mystery"),
            /*configured_model*/ None,
            &providers(),
            &catalog(),
        ),
        Err(SelectionError::UnknownProvider("mystery".to_string()))
    );
}

#[test]
fn unknown_model_is_rejected() {
    assert_eq!(
        select_provider(
            /*configured_provider*/ None,
            Some("missing-model"),
            &providers(),
            &catalog(),
        ),
        Err(SelectionError::UnknownModel("missing-model".to_string()))
    );
}

#[test]
fn unsupported_route_names_the_model_and_endpoints() {
    let error = select_provider(
        /*configured_provider*/ None,
        Some("commandcode-embeddings"),
        &providers(),
        &catalog(),
    )
    .expect_err("unsupported route should fail before any request");

    let SelectionError::UnsupportedRoute(route_error) = error else {
        panic!("expected an unsupported route error");
    };
    assert_eq!(
        (
            route_error.model(),
            route_error.supported_endpoints(),
            route_error.to_string(),
        ),
        (
            "commandcode-embeddings",
            &["embeddings".to_string()][..],
            "model `commandcode-embeddings` does not support a Codex request route; \
supported endpoints: embeddings"
                .to_string(),
        )
    );
}

#[test]
fn routes_map_to_provider_relative_paths_and_endpoints() {
    for (route, path, endpoint) in [
        (
            CommandCodeRoute::Responses,
            "/provider/v1/responses",
            "https://api.commandcode.ai/provider/v1/responses",
        ),
        (
            CommandCodeRoute::ChatCompletions,
            "/provider/v1/chat/completions",
            "https://api.commandcode.ai/provider/v1/chat/completions",
        ),
        (
            CommandCodeRoute::AnthropicMessages,
            "/provider/v1/messages",
            "https://api.commandcode.ai/provider/v1/messages",
        ),
    ] {
        assert_eq!(
            (
                route_path(route),
                route_endpoint("https://api.commandcode.ai/provider/v1/", route),
            ),
            (path, endpoint.to_string())
        );
    }
}
