//! Configure a provider through the app-server credential boundary.

use super::*;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ModelListParams;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::ModelProviderConfigureParams;
use codex_app_server_protocol::ModelProviderConfigureResponse;

impl App {
    pub(super) fn configure_model_provider(
        &mut self,
        app_server: &mut AppServerSession,
        params: ModelProviderConfigureParams,
    ) {
        self.provider_transition_blocked = true;
        self.chat_widget.block_provider_transition();
        let request_id = app_server.next_request_id();
        let handle = app_server.request_handle();
        let events = self.app_event_tx.clone();
        self.chat_widget
            .add_info_message("Connecting API provider…".to_string(), /*hint*/ None);
        tokio::spawn(async move {
            let result = handle
                .request_typed::<ModelProviderConfigureResponse>(
                    ClientRequest::ModelProviderConfigure { request_id, params },
                )
                .await
                .map_err(|_| "Could not configure the API provider. Check the endpoint and credential store, then try again.".to_string());
            events.send(AppEvent::ModelProviderConfigured(result));
        });
    }

    pub(super) async fn finish_model_provider_setup(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        result: Result<ModelProviderConfigureResponse, String>,
    ) {
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
                        self.chat_widget.add_error_message("Provider saved, but model discovery failed or returned no models. Retry /provider.".to_string());
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
                    self.chat_widget.open_model_popup_with_presets(models);
                } else {
                    self.model_catalog = previous_models;
                    app_server.set_available_models(self.model_catalog.models.clone());
                    self.chat_widget.block_provider_transition();
                    self.chat_widget.add_error_message(
                        "Provider saved, but a new session could not be started with it. Retry /provider.".to_string(),
                    );
                }
            }
            Err(message) => self
                .chat_widget
                .add_error_message(format!("{message} Retry /provider.")),
        }
    }
}
