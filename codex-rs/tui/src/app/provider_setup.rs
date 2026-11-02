//! Configure a provider through the app-server credential boundary.

use super::*;
use crate::app_event::ProviderSetupResult;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::ModelProviderConfigureParams;
use codex_app_server_protocol::ModelProviderConfigureResponse;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ProviderSetupState {
    #[default]
    Idle,
    InFlight,
    OutcomeUnknown,
}

impl App {
    pub(super) fn configure_model_provider(
        &mut self,
        app_server: &mut AppServerSession,
        params: ModelProviderConfigureParams,
    ) {
        match self.provider_setup_state {
            ProviderSetupState::Idle => {}
            ProviderSetupState::InFlight => {
                self.chat_widget.add_error_message(
                    "Provider setup is still in progress. Wait for it to finish before retrying."
                        .to_string(),
                );
                return;
            }
            ProviderSetupState::OutcomeUnknown => {
                self.chat_widget.add_error_message("Provider setup may still be running. Confirm it has finished, then restart Lentor before retrying.".to_string());
                return;
            }
        }
        self.provider_setup_state = ProviderSetupState::InFlight;
        self.provider_transition_blocked = true;
        self.chat_widget.block_provider_transition();
        let read_id = app_server.next_request_id();
        let request_id = app_server.next_request_id();
        let cwd = Some(
            self.chat_widget
                .config_ref()
                .cwd
                .to_string_lossy()
                .into_owned(),
        );
        let handle = app_server.request_handle();
        let events = self.app_event_tx.clone();
        self.chat_widget
            .add_info_message("Connecting API provider…".to_string(), /*hint*/ None);
        tokio::spawn(async move {
            let baseline = tokio::time::timeout(
                Duration::from_secs(10),
                handle.request_typed::<ConfigReadResponse>(ClientRequest::ConfigRead {
                    request_id: read_id,
                    params: ConfigReadParams {
                        include_layers: false,
                        cwd,
                    },
                }),
            )
            .await
            .ok()
            .and_then(Result::ok);
            let result = tokio::time::timeout(
                Duration::from_secs(60),
                handle.request_typed::<ModelProviderConfigureResponse>(
                    ClientRequest::ModelProviderConfigure { request_id, params },
                ),
            )
            .await
            .unwrap_or_else(|_| {
                Err(TypedRequestError::Transport {
                    method: "modelProvider/configure".to_string(),
                    source: std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "provider setup timed out",
                    ),
                })
            });
            events.send(AppEvent::ModelProviderConfigured(ProviderSetupResult {
                baseline,
                result,
            }));
        });
    }

    pub(super) async fn finish_model_provider_setup(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        ProviderSetupResult { baseline, result }: ProviderSetupResult,
    ) {
        if self.provider_setup_state == ProviderSetupState::OutcomeUnknown {
            return;
        }
        self.provider_setup_state = match &result {
            Ok(_) | Err(TypedRequestError::Server { .. }) => ProviderSetupState::Idle,
            Err(TypedRequestError::Transport { .. } | TypedRequestError::Deserialize { .. }) => {
                ProviderSetupState::OutcomeUnknown
            }
        };
        match result {
            Ok(configured) => {
                self.provider_transition_blocked = true;
                self.chat_widget.block_provider_transition();
                let request_id = app_server.next_request_id();
                let response = app_server
                    .request_handle()
                    .request_typed::<ModelListResponse>(ClientRequest::ModelList {
                        request_id,
                        params: ModelListParams {
                            cursor: None,
                            limit: None,
                            include_hidden: Some(true),
                        },
                    })
                    .await;
                let models = match response {
                    Ok(response) if response.data.iter().any(|model| !model.hidden) => response
                        .data
                        .into_iter()
                        .map(crate::app_server_session::model_preset_from_api_model)
                        .collect::<Vec<_>>(),
                    _ => {
                        self.chat_widget.add_error_message("Provider saved, but model discovery failed or returned no models. Retry /provider or restart Lentor to reload the effective settings.".to_string());
                        return;
                    }
                };
                let previous_models = self.model_catalog.clone();
                let previous_thread_id = self.primary_thread_id;
                Arc::make_mut(&mut self.model_catalog).models = models.clone();
                app_server.set_available_models(models.clone());
                self.start_fresh_session(
                    tui, app_server, /*session_start_source*/ None,
                    /*initial_user_message*/ None, /*new_thread_name*/ None,
                )
                .await;
                if self.primary_thread_id.is_some()
                    && self.primary_thread_id != previous_thread_id
                    && self.chat_widget.config_ref().model_provider_id == configured.provider_id
                {
                    self.provider_transition_blocked = false;
                    self.chat_widget.unblock_provider_transition();
                    self.chat_widget.open_model_popup_with_presets(models);
                } else {
                    self.model_catalog = previous_models;
                    app_server.set_available_models(self.model_catalog.models.clone());
                    self.chat_widget.block_provider_transition();
                    self.chat_widget.add_error_message(
                        "Provider saved, but a new session could not be started with it. Retry /provider or restart Lentor to reload the effective settings.".to_string(),
                    );
                }
            }
            Err(error) => {
                // A server error completes the request, so compare the effective
                // settings from both sides of the write before restoring the thread.
                // A transport error may leave a write in flight and cannot prove this.
                let can_reconcile = matches!(&error, TypedRequestError::Server { .. });
                if can_reconcile && let Some(baseline) = baseline {
                    let request_id = app_server.next_request_id();
                    let handle = app_server.request_handle();
                    let current = tokio::time::timeout(
                        Duration::from_secs(10),
                        handle.request_typed::<ConfigReadResponse>(ClientRequest::ConfigRead {
                            request_id,
                            params: ConfigReadParams {
                                include_layers: false,
                                cwd: Some(
                                    self.chat_widget
                                        .config_ref()
                                        .cwd
                                        .to_string_lossy()
                                        .into_owned(),
                                ),
                            },
                        }),
                    )
                    .await;
                    if let Ok(Ok(current)) = current
                        && current.config == baseline.config
                        && effective_provider_matches_thread(
                            &current.config,
                            self.chat_widget.config_ref(),
                        )
                    {
                        self.provider_transition_blocked = false;
                        self.chat_widget.unblock_provider_transition();
                        self.chat_widget.add_error_message(
                            "Provider setup failed; the original session is available again."
                                .to_string(),
                        );
                        return;
                    }
                }
                let message = match error {
                    TypedRequestError::Server { source, .. } => match source
                        .data
                        .as_ref()
                        .and_then(|data| data.get("providerConfigurationOutcome"))
                        .and_then(serde_json::Value::as_str)
                    {
                        Some("unchanged") => "Provider setup failed before changing configuration.",
                        Some("saved") => "Provider settings were saved but could not be activated.",
                        _ => "Provider setup failed; the configuration outcome is unknown.",
                    },
                    TypedRequestError::Transport { .. } | TypedRequestError::Deserialize { .. } => {
                        "Provider setup was interrupted; the configuration outcome is unknown."
                    }
                };
                let guidance = if self.provider_setup_state == ProviderSetupState::OutcomeUnknown {
                    "Confirm the previous setup has finished, then restart Lentor before retrying."
                } else {
                    "Retry /provider or restart Lentor to reload the effective settings."
                };
                self.chat_widget
                    .add_error_message(format!("{message} {guidance}"));
            }
        }
    }
}

// Compare the effective provider definition as well as its ID: an existing thread
// can retain an older endpoint or credential even when the provider ID is unchanged.
// The thread's model may intentionally differ from the saved default, and the
// widget config can still hold the model displayed before thread attachment.
fn effective_provider_matches_thread(
    effective: &codex_app_server_protocol::Config,
    thread: &Config,
) -> bool {
    let provider_id = effective.model_provider.as_deref().unwrap_or("openai");
    if provider_id != thread.model_provider_id {
        return false;
    }
    let configured = effective
        .additional
        .get("model_providers")
        .and_then(|providers| providers.get(provider_id));
    let provider = match configured {
        Some(value) => serde_json::from_value(value.clone()).ok(),
        None => codex_model_provider_info::built_in_model_providers(/*openai_base_url*/ None)
            .remove(provider_id),
    };
    provider.as_ref() == Some(&thread.model_provider)
}
