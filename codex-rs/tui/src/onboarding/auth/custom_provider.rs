//! First-run setup for a custom API provider.

use super::*;
use crate::provider_setup::ProviderSetupAction;
use crate::render::renderable::Renderable;

impl AuthModeWidget {
    pub(super) fn handle_custom_provider_key_event(&mut self, key_event: KeyEvent) -> bool {
        let selected_model = {
            let mut state = self.sign_in_state.write().unwrap();
            if let SignInState::CustomProviderModels { models, selected } = &mut *state {
                match key_event.code {
                    KeyCode::Up => *selected = selected.saturating_sub(1),
                    KeyCode::Down => *selected = (*selected + 1).min(models.len().saturating_sub(1)),
                    KeyCode::Enter => {
                        let model = models.get(*selected).cloned();
                        if model.is_some() {
                            *state = SignInState::CustomProviderConnecting;
                        }
                        model
                    }
                    _ => None,
                }
            } else {
                None
            }
        };
        if let Some(model) = selected_model {
            let handle = self.app_server_request_handle.clone();
            let state = self.sign_in_state.clone();
            let error = self.error.clone();
            let frame = self.request_frame.clone();
            tokio::spawn(async move {
                let edits = crate::config_update::build_model_selection_edits(
                    &model,
                    None::<codex_protocol::openai_models::ReasoningEffort>,
                );
                match crate::config_update::write_config_batch(handle, edits).await {
                    Ok(_) => *state.write().unwrap() = SignInState::CustomProviderConfigured,
                    Err(_) => {
                        *error.write().unwrap() = Some("Could not save the selected model. Configure the provider again.".to_string());
                        *state.write().unwrap() = SignInState::CustomProvider(ProviderSetupForm::new(String::new()));
                    }
                }
                frame.schedule_frame();
            });
            return true;
        }
        if matches!(&*self.sign_in_state.read().unwrap(), SignInState::CustomProviderModels { .. }) {
            self.request_frame.schedule_frame();
            return true;
        }
        let action = {
            let mut state = self.sign_in_state.write().unwrap();
            if let SignInState::CustomProvider(form) = &mut *state {
                Some(form.key(key_event))
            } else if matches!(&*state, SignInState::CustomProviderConnecting) {
                return true;
            } else {
                None
            }
        };
        if let Some(action) = action {
            match action {
                Some(ProviderSetupAction::Cancel) => {
                    *self.sign_in_state.write().unwrap() = SignInState::PickMode;
                }
                Some(ProviderSetupAction::Save(params)) => {
                    *self.sign_in_state.write().unwrap() = SignInState::CustomProviderConnecting;
                    let handle = self.app_server_request_handle.clone();
                    let state = self.sign_in_state.clone();
                    let error = self.error.clone();
                    let frame = self.request_frame.clone();
                    tokio::spawn(async move {
                        let result = handle.request_typed::<codex_app_server_protocol::ModelProviderConfigureResponse>(
                            ClientRequest::ModelProviderConfigure {
                                request_id: onboarding_request_id(),
                                params,
                            },
                        ).await;
                        match result {
                            Ok(_) => {
                                let models = handle.request_typed::<codex_app_server_protocol::ModelListResponse>(
                                    ClientRequest::ModelList {
                                        request_id: onboarding_request_id(),
                                        params: codex_app_server_protocol::ModelListParams {
                                            cursor: None, limit: None, include_hidden: Some(false),
                                        },
                                    },
                                ).await;
                                match models {
                                    Ok(response) if response.data.iter().any(|model| !model.hidden) => {
                                        *state.write().unwrap() = SignInState::CustomProviderModels {
                                            models: response.data.into_iter().filter(|model| !model.hidden).map(|model| model.model).collect(),
                                            selected: 0,
                                        };
                                    }
                                    _ => {
                                        *error.write().unwrap() = Some("Model discovery failed or returned no models. Configure the provider again.".to_string());
                                        *state.write().unwrap() = SignInState::CustomProvider(ProviderSetupForm::new(String::new()));
                                    }
                                }
                            }
                            Err(_) => {
                                *error.write().unwrap() = Some("Could not configure the API provider. Check the endpoint and credential store, then try again.".to_string());
                                *state.write().unwrap() = SignInState::CustomProvider(ProviderSetupForm::new(String::new()));
                            }
                        }
                        frame.schedule_frame();
                    });
                }
                None => {}
            }
            self.request_frame.schedule_frame();
            return true;
        }
        false
    }

    pub(super) fn paste_custom_provider(&mut self, pasted: &str) -> bool {
        let mut state = self.sign_in_state.write().unwrap();
        if let SignInState::CustomProvider(form) = &mut *state {
            form.paste(pasted);
            self.request_frame.schedule_frame();
            return true;
        }
        false
    }
    pub(super) fn render_custom_provider(&self, state: &SignInState, area: Rect, buf: &mut Buffer) {
        match state {
            SignInState::CustomProvider(form) => {
                form.render(area, buf);
                if let Some(error) = self.error_message() {
                    let error_area = Rect { y: area.y.saturating_add(11), height: area.height.saturating_sub(11), ..area };
                    Paragraph::new(error.red()).wrap(Wrap { trim: false }).render(error_area, buf);
                }
            }
            SignInState::CustomProviderConnecting => {
                Paragraph::new("Connecting API provider…").render(area, buf);
            }
            SignInState::CustomProviderModels { models, selected } => {
                let mut lines = vec![Line::from("Select a model".bold())];
                let capacity = usize::from(area.height.saturating_sub(1)).max(1);
                let start = selected.saturating_add(1).saturating_sub(capacity);
                lines.extend(models.iter().enumerate().skip(start).take(capacity).map(|(index, model)| {
                    let marker = if index == *selected { "> " } else { "  " };
                    Line::from(format!("{marker}{model}"))
                }));
                Paragraph::new(lines).render(area, buf);
            }
            SignInState::CustomProviderConfigured => {
                Paragraph::new("✓ API provider configured".green()).render(area, buf);
            }
            _ => {}
        }
    }
}
