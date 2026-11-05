use super::*;
use crate::create_model_provider;
use codex_model_provider_info::ModelProviderCapabilities;
use pretty_assertions::assert_eq;

#[test]
fn capability_overrides_preserve_unspecified_provider_defaults() {
    for info in [
        ModelProviderInfo::default(),
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        ModelProviderInfo {
            name: "Azure".to_string(),
            ..Default::default()
        },
    ] {
        let defaults = create_model_provider(info.clone(), /*auth_manager*/ None).capabilities();
        let provider = create_model_provider(
            ModelProviderInfo {
                capabilities: Some(ModelProviderCapabilities {
                    external_web_access: Some(false),
                    ..Default::default()
                }),
                ..info
            },
            /*auth_manager*/ None,
        );
        assert_eq!(
            provider.capabilities(),
            ProviderCapabilities {
                external_web_access: false,
                ..defaults
            },
        );
    }
}

#[test]
fn translated_protocols_do_not_advertise_hosted_tools_or_remote_compaction() {
    for wire_api in [WireApi::Chat, WireApi::Messages] {
        let info = ModelProviderInfo {
            wire_api,
            capabilities: Some(ModelProviderCapabilities {
                remote_compaction: Some(RemoteCompactionSupport::V2),
                external_web_access: Some(true),
                ..Default::default()
            }),
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        };
        assert_eq!(
            ProviderCapabilities::from_config(&info),
            ProviderCapabilities {
                image_generation: false,
                web_search: false,
                external_web_access: false,
                remote_compaction: RemoteCompactionSupport::Unsupported,
            }
        );
    }
}
