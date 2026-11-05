//! Manual-model recovery after an unavailable provider catalog.

use super::*;
use codex_app_server_protocol::ModelListResponse;
use codex_app_server_protocol::ModelProviderConfigureResponse;
use pretty_assertions::assert_eq;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn manual_model_recovers_from_empty_or_failed_discovery() -> Result<()> {
    for discovery_fails in [false, true] {
        let endpoint = wiremock::MockServer::start().await;
        let home = tempdir()?;
        let model = "Vendor/Exact.Model:V2";
        MockResponsesConfig::new(&endpoint.uri())
            .with_model("Previous/Model")
            .with_extra_config("[tui]\nstatus_line = [\"model-name\"]")
            .write(home.path())?;
        let (mut app, mut events, _) = make_test_app_with_channels().await;
        app.config = ConfigBuilder::default()
            .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
            .codex_home(home.path().to_path_buf())
            .fallback_cwd(Some(home.path().to_path_buf()))
            .build()
            .await?;
        disable_fixture_plugins(&mut app)?;
        app.local_settings.tui.status_line = Some(vec!["model-name".to_string()]);
        let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
        let outcome = std::panic::AssertUnwindSafe(async {
            let mut tui = crate::tui::test_support::make_test_tui()?;
            let started = server.start_thread(&app.config).await?;
            app.replace_chat_widget_with_app_server_thread(
                &mut tui,
                started,
                super::super::super::session_lifecycle::ThreadAttachPresentation::Fresh,
                /*initial_user_message*/ None,
            )
            .await?;
            let old_thread = app.current_displayed_thread_id().expect("attached thread");
            app.chat_widget
                .restore_user_message_to_composer("saved draft".into());
            let old_input = app.chat_widget.capture_thread_input_state().expect("input");
            while events.try_recv().is_ok() {}
            let provider_id = app.chat_widget.config_ref().model_provider_id.clone();
            assert_eq!(app.chat_widget.current_model(), "Previous/Model");
            let request_id = server.next_request_id();
            server
                .request_handle()
                .request_typed::<codex_app_server_protocol::ConfigWriteResponse>(
                    ClientRequest::ConfigValueWrite {
                        request_id,
                        params: codex_app_server_protocol::ConfigValueWriteParams {
                            key_path: "model".into(),
                            value: json!(model),
                            merge_strategy: codex_app_server_protocol::MergeStrategy::Replace,
                            file_path: None,
                            expected_version: None,
                        },
                    },
                )
                .await?;
            app.provider_transition_blocked = true;
            app.chat_widget.block_provider_transition();
            let discovery = if discovery_fails {
                Err(TypedRequestError::Server {
                    method: "model/list".into(),
                    source: JSONRPCErrorError {
                        code: -32603,
                        message: "discovery failed".into(),
                        data: None,
                    },
                })
            } else {
                Ok(ModelListResponse {
                    data: vec![],
                    next_cursor: None,
                })
            };
            app.finish_provider_discovery(
                &mut tui,
                &mut server,
                ModelProviderConfigureResponse {
                    provider_id,
                    base_url: format!("{}/v1", endpoint.uri()),
                    model: Some(model.into()),
                },
                discovery,
            )
            .await;
            assert!(!app.provider_transition_blocked);
            assert_ne!(app.current_displayed_thread_id(), Some(old_thread));
            assert_eq!(app.chat_widget.current_model(), model);
            assert!(app.model_catalog.models.is_empty());
            assert!(app.chat_widget.no_modal_or_popup_active());
            assert_eq!(
                app.agents_overview.input_states.get(&old_thread),
                Some(&old_input)
            );
            while let Ok(event) = events.try_recv() {
                if let AppEvent::InsertHistoryCell(cell) = event {
                    app.transcript_cells.push(Arc::from(cell));
                }
            }
            if discovery_fails {
                assert_app_snapshot!(
                    "provider_manual_model_discovery_error",
                    render_bottom_popup(&app.chat_widget, /*width*/ 80)
                );
            } else {
                assert_app_snapshot!(
                    "provider_manual_model_empty_discovery",
                    render_bottom_popup(&app.chat_widget, /*width*/ 80)
                );
            }
            Ok::<(), color_eyre::Report>(())
        })
        .catch_unwind()
        .await;
        finish_provider_transition_test(app, server, outcome).await?;
    }
    Ok(())
}

#[tokio::test]
async fn no_manual_model_keeps_failed_discovery_blocked() -> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    disable_fixture_plugins(&mut app)?;
    let mut server = start_config_write_test_app_server(&app).await?;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let old_thread = app.primary_thread_id;
        app.provider_transition_blocked = true;
        app.chat_widget.block_provider_transition();
        app.finish_provider_discovery(
            &mut tui,
            &mut server,
            ModelProviderConfigureResponse {
                provider_id: "mock_provider".into(),
                base_url: "http://127.0.0.1/v1".into(),
                model: None,
            },
            Ok(ModelListResponse {
                data: vec![],
                next_cursor: None,
            }),
        )
        .await;
        assert!(app.provider_transition_blocked);
        assert_eq!(app.primary_thread_id, old_thread);
        while let Ok(event) = events.try_recv() {
            if let AppEvent::InsertHistoryCell(cell) = event {
                app.transcript_cells.push(Arc::from(cell));
            }
        }
        assert_app_snapshot!(
            "provider_manual_model_missing_blocked",
            app.render_transcript_lines_for_reflow(/*width*/ 80)
                .lines
                .iter()
                .map(rendered_line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );
        Ok::<(), color_eyre::Report>(())
    })
    .catch_unwind()
    .await;
    finish_provider_transition_test(app, server, outcome).await
}
