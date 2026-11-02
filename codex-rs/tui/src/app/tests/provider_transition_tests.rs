//! Provider transition coverage through the TUI and embedded app-server.

use super::*;
use crate::app_event::ProviderSetupResult;
use app_test_support::MockResponsesConfig;
use codex_app_server_client::TypedRequestError;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ConfigReadParams;
use codex_app_server_protocol::ConfigReadResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ModelProviderConfigureParams;
use core_test_support::responses;
use futures::FutureExt;
use pretty_assertions::assert_eq;
use serde_json::json;

fn disable_fixture_plugins(app: &mut App) -> Result<()> {
    app.config.features.disable(Feature::Plugins)?;
    let path = app.config.codex_home.join("config.toml");
    let mut config: toml::Table = match std::fs::read_to_string(&path) {
        Ok(contents) => toml::from_str(&contents)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => toml::Table::new(),
        Err(error) => return Err(error.into()),
    };
    let features = config
        .entry("features")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()))
        .as_table_mut()
        .ok_or_else(|| color_eyre::eyre::eyre!("features must be a TOML table"))?;
    features.insert("plugins".into(), toml::Value::Boolean(false));
    std::fs::write(path, toml::to_string(&config)?)?;
    Ok(())
}

async fn finish_provider_transition_test(
    mut app: Box<App>,
    mut server: AppServerSession,
    outcome: std::thread::Result<Result<()>>,
) -> Result<()> {
    let detached = tokio::time::timeout(Duration::from_secs(10), async {
        app.shutdown_side_threads(&mut server).await;
        app.shutdown_current_thread(&mut server).await;
    })
    .await;
    app.abort_all_thread_event_listeners();
    // The runtime still uses this home while flushing and shutting down threads.
    let test_home = app._test_codex_home.take();
    drop(app);
    let shutdown = server.shutdown().await;
    drop(test_home);
    match outcome {
        Ok(result) => {
            detached?;
            shutdown?;
            result
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlapping_setup_is_rejected_until_the_active_request_completes() -> Result<()> {
    let endpoint = wiremock::MockServer::start().await;
    let home = tempdir()?;
    MockResponsesConfig::new(&endpoint.uri()).write(home.path())?;
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    app.config = ConfigBuilder::default()
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await?;
    disable_fixture_plugins(&mut app)?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let started = server.start_thread(&app.config).await?;
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            started,
            super::super::session_lifecycle::ThreadAttachPresentation::Fresh,
            /*initial_user_message*/ None,
        )
        .await?;
        while events.try_recv().is_ok() {}

        for name in ["first", "second"] {
            app.configure_model_provider(
                &mut server,
                ModelProviderConfigureParams {
                    provider_id: "openai".into(),
                    name: name.into(),
                    base_url: "https://example.test/v1".into(),
                    api_key: "synthetic".into(),
                    model: None,
                },
            );
        }

        let completion = loop {
            let event = tokio::time::timeout(Duration::from_secs(10), events.recv())
                .await?
                .expect("TUI event stream remains open");
            if matches!(event, AppEvent::ModelProviderConfigured(_)) {
                break event;
            }
        };
        assert!(app.provider_transition_blocked);
        Box::pin(app.handle_event(&mut tui, &mut server, completion)).await?;
        assert!(matches!(
            app.provider_setup_state,
            super::super::provider_setup::ProviderSetupState::Idle
        ));
        let extra = tokio::time::timeout(Duration::from_millis(250), async {
            while let Some(event) = events.recv().await {
                if matches!(event, AppEvent::ModelProviderConfigured(_)) {
                    return true;
                }
            }
            false
        })
        .await;
        assert!(
            extra.is_err(),
            "the overlapping attempt must not start an RPC"
        );
        app.configure_model_provider(
            &mut server,
            ModelProviderConfigureParams {
                provider_id: "openai".into(),
                name: "retry".into(),
                base_url: "https://example.test/v1".into(),
                api_key: "synthetic".into(),
                model: None,
            },
        );
        let retry = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Some(event @ AppEvent::ModelProviderConfigured(_)) = events.recv().await {
                    break event;
                }
            }
        })
        .await?;
        Box::pin(app.handle_event(&mut tui, &mut server, retry)).await?;
        Ok::<(), color_eyre::Report>(())
    })
    .catch_unwind()
    .await;
    finish_provider_transition_test(app, server, outcome).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_setup_restores_original_thread_and_sends_turn() -> Result<()> {
    let endpoint = wiremock::MockServer::start().await;
    let bodies = (0..3)
        .map(|index| {
            responses::sse(vec![
                responses::ev_response_created(&format!("response-{index}")),
                responses::ev_assistant_message(
                    &format!("message-{index}"),
                    "sent on original thread",
                ),
                responses::ev_completed(&format!("response-{index}")),
            ])
        })
        .collect();
    let response = responses::mount_sse_sequence(&endpoint, bodies).await;
    let home = tempdir()?;
    MockResponsesConfig::new(&endpoint.uri()).write(home.path())?;
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    app.config = ConfigBuilder::default()
        .loader_overrides(LoaderOverrides::without_managed_config_for_tests())
        .codex_home(home.path().to_path_buf())
        .fallback_cwd(Some(home.path().to_path_buf()))
        .build()
        .await?;
    disable_fixture_plugins(&mut app)?;
    let mut server = Box::pin(crate::start_embedded_app_server_for_picker(&app.config)).await?;
    let outcome = std::panic::AssertUnwindSafe(async {
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let started = server.start_thread(&app.config).await?;
    let thread_id = started.session.thread_id;
    app.replace_chat_widget_with_app_server_thread(
        &mut tui,
        started,
        super::super::session_lifecycle::ThreadAttachPresentation::Fresh,
        /*initial_user_message*/ None,
    )
    .await?;
    while events.try_recv().is_ok() {}

    // Exercise the real validation RPC and its effective-config reconciliation.
    app.configure_model_provider(
        &mut server,
        ModelProviderConfigureParams {
            provider_id: "openai".into(),
            name: "Invalid".into(),
            base_url: "https://example.test/v1".into(),
            api_key: "synthetic".into(),
            model: None,
        },
    );
    let completed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match events.recv().await {
                Some(event @ AppEvent::ModelProviderConfigured(_)) => break event,
                Some(AppEvent::InsertHistoryCell(cell)) => {
                    app.transcript_cells.push(Arc::from(cell))
                }
                Some(_) => {}
                None => panic!("TUI event stream closed"),
            }
        }
    })
    .await?;
    if let AppEvent::ModelProviderConfigured(ProviderSetupResult { baseline, .. }) = &completed {
        let baseline = baseline.as_ref().expect("baseline config read succeeds");
        assert!(
            baseline.config.model_provider.as_deref()
                == Some(app.chat_widget.config_ref().model_provider_id.as_str()),
            "baseline provider differs from original thread"
        );
        assert!(
            baseline.config.model.as_deref() == Some(app.chat_widget.current_model()),
            "baseline model differs from the active original thread model"
        );
        assert!(
            baseline.config.model != app.chat_widget.config_ref().model,
            "fixture must exercise the stale display-model config after thread attachment"
        );
        let configured = baseline.config.additional.get("model_providers")
            .and_then(|providers| providers.get(&app.chat_widget.config_ref().model_provider_id))
            .and_then(|provider| serde_json::from_value::<codex_model_provider_info::ModelProviderInfo>(provider.clone()).ok());
        assert!(
            configured.as_ref() == Some(&app.chat_widget.config_ref().model_provider),
            "baseline provider definition differs from original thread"
        );
    }
    assert_app_snapshot!(
        "provider_transition_connecting",
        app.render_transcript_lines_for_reflow(/*width*/ 80)
            .lines
            .iter()
            .map(rendered_line_text)
            .collect::<Vec<_>>()
            .join("\n")
    );
    Box::pin(app.handle_event(&mut tui, &mut server, completed)).await?;
    assert!(!app.provider_transition_blocked, "validation failure restores the original thread");
    assert_eq!(app.primary_thread_id, Some(thread_id));
    while let Ok(event) = events.try_recv() {
        if let AppEvent::InsertHistoryCell(cell) = event {
            app.transcript_cells.push(Arc::from(cell));
        }
    }
    assert_app_snapshot!(
        "provider_transition_recovered",
        app.render_transcript_lines_for_reflow(/*width*/ 80)
            .lines
            .iter()
            .map(rendered_line_text)
            .collect::<Vec<_>>()
            .join("\n")
    );
    for failure in ["validation", "keyring", "config"] {
        if failure != "validation" {
            let id = server.next_request_id();
            let baseline = server
                .request_handle()
                .request_typed::<ConfigReadResponse>(ClientRequest::ConfigRead {
                    request_id: id,
                    params: ConfigReadParams {
                        include_layers: false,
                        cwd: Some(app.config.cwd.to_string_lossy().into_owned()),
                    },
                })
                .await?;
            app.provider_transition_blocked = true;
            app.chat_widget.block_provider_transition();
            let error = TypedRequestError::Server {
                method: "modelProvider/configure".into(),
                source: JSONRPCErrorError {
                    code: -32600,
                    message: format!("{failure} failed"),
                    data: Some(
                        json!({"providerConfigurationOutcome": if failure == "keyring" { "unchanged" } else { "unknown" }}),
                    ),
                },
            };
            app.finish_model_provider_setup(
                &mut tui,
                &mut server,
                ProviderSetupResult {
                    baseline: Some(baseline),
                    result: Err(error),
                },
            )
            .await;
        }
        assert!(!app.provider_transition_blocked, "{failure}");
        assert_eq!(app.primary_thread_id, Some(thread_id));
        while events.try_recv().is_ok() {}
        app.chat_widget
            .restore_user_message_to_composer(format!("continue after {failure}").into());
        app.chat_widget
            .handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));
        let turn = std::iter::from_fn(|| events.try_recv().ok())
            .find_map(|event| match event {
                AppEvent::CodexOp(turn @ Op::UserTurn { .. }) => Some(turn),
                _ => None,
            })
            .expect("both gates permit the original thread's turn");
        app.submit_thread_op(&mut server, thread_id, turn).await?;
        tokio::time::timeout(Duration::from_secs(15), async {
            loop {
                let event = server.next_event().await.expect("server remains connected");
                let completed = matches!(&event.event, codex_app_server_client::AppServerEvent::ServerNotification(notification)
                    if matches!(notification.as_ref(), ServerNotification::TurnCompleted(_)));
                app.handle_backend_event(&server, event).await;
                app.drain_active_thread_events(&mut tui).await?;
                if completed { break Ok::<_, color_eyre::Report>(()); }
            }
        }).await??;
    }
    assert_eq!(response.requests().len(), 3);
    let request_id = server.next_request_id();
    let baseline = server.request_handle().request_typed::<ConfigReadResponse>(
        ClientRequest::ConfigRead {
            request_id,
            params: ConfigReadParams {
                include_layers: false,
                cwd: Some(app.config.cwd.to_string_lossy().into_owned()),
            },
        },
    ).await?;
    let request_id = server.next_request_id();
    server.request_handle().request_typed::<codex_app_server_protocol::ConfigWriteResponse>(
        ClientRequest::ConfigValueWrite {
            request_id,
            params: codex_app_server_protocol::ConfigValueWriteParams {
                key_path: "model".into(),
                value: json!("saved-alternative-model"),
                merge_strategy: codex_app_server_protocol::MergeStrategy::Replace,
                file_path: None,
                expected_version: None,
            },
        },
    ).await?;
    app.provider_transition_blocked = true;
    app.chat_widget.block_provider_transition();
    app.finish_model_provider_setup(&mut tui, &mut server, ProviderSetupResult {
        baseline: Some(baseline),
        result: Err(TypedRequestError::Server {
            method: "modelProvider/configure".into(),
            source: JSONRPCErrorError {
                code: -32600,
                message: "saved model changed".into(),
                data: Some(json!({"providerConfigurationOutcome":"saved"})),
            },
        }),
    }).await;
    assert!(app.provider_transition_blocked, "a changed saved model must prevent restoration");
    assert_eq!(app.primary_thread_id, Some(thread_id));
    Ok::<(), color_eyre::Report>(())
    }).catch_unwind().await;
    finish_provider_transition_test(app, server, outcome).await
}

#[tokio::test]
async fn changed_or_uncertain_configuration_keeps_original_thread_blocked() -> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    disable_fixture_plugins(&mut app)?;
    let mut server = start_config_write_test_app_server(&app).await?;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let id = server.next_request_id();
        let mut baseline = server
            .request_handle()
            .request_typed::<ConfigReadResponse>(ClientRequest::ConfigRead {
                request_id: id,
                params: ConfigReadParams {
                    include_layers: false,
                    cwd: Some(app.config.cwd.to_string_lossy().into_owned()),
                },
            })
            .await?;
        let unchanged = baseline.clone();
        baseline.config.model_provider = Some("different-provider".into());
        app.provider_transition_blocked = true;
        app.chat_widget.block_provider_transition();
        let error = TypedRequestError::Server {
            method: "modelProvider/configure".into(),
            source: JSONRPCErrorError {
                code: -32600,
                message: "saved but overridden".into(),
                data: Some(json!({"providerConfigurationOutcome":"saved"})),
            },
        };
        app.finish_model_provider_setup(
            &mut tui,
            &mut server,
            ProviderSetupResult {
                baseline: Some(baseline),
                result: Err(error),
            },
        )
        .await;
        assert!(app.provider_transition_blocked);
        while let Ok(event) = events.try_recv() {
            if let AppEvent::InsertHistoryCell(cell) = event {
                app.transcript_cells.push(Arc::from(cell));
            }
        }
        assert_app_snapshot!(
            "provider_transition_blocked",
            app.render_transcript_lines_for_reflow(/*width*/ 80)
                .lines
                .iter()
                .map(rendered_line_text)
                .collect::<Vec<_>>()
                .join("\n")
        );
        app.chat_widget
            .restore_user_message_to_composer("must stay queued".into());
        app.chat_widget
            .handle_key_event(crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE,
            ));
        assert!(
            !std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::CodexOp(Op::UserTurn { .. })))
        );

        let error = TypedRequestError::Transport {
            method: "modelProvider/configure".into(),
            source: std::io::Error::other("connection lost"),
        };
        app.finish_model_provider_setup(
            &mut tui,
            &mut server,
            ProviderSetupResult {
                baseline: None,
                result: Err(error),
            },
        )
        .await;
        assert!(app.provider_transition_blocked);
        assert!(matches!(
            app.provider_setup_state,
            super::super::provider_setup::ProviderSetupState::OutcomeUnknown
        ));
        let stale = TypedRequestError::Server {
            method: "modelProvider/configure".into(),
            source: JSONRPCErrorError {
                code: -32600,
                message: "late completion".into(),
                data: Some(json!({"providerConfigurationOutcome":"unchanged"})),
            },
        };
        app.finish_model_provider_setup(
            &mut tui,
            &mut server,
            ProviderSetupResult {
                baseline: Some(unchanged),
                result: Err(stale),
            },
        )
        .await;
        assert!(matches!(
            app.provider_setup_state,
            super::super::provider_setup::ProviderSetupState::OutcomeUnknown
        ));
        assert!(app.provider_transition_blocked);
        app.configure_model_provider(
            &mut server,
            ModelProviderConfigureParams {
                provider_id: "openai".into(),
                name: "retry after transport loss".into(),
                base_url: "https://example.test/v1".into(),
                api_key: "synthetic".into(),
                model: None,
            },
        );
        assert!(matches!(
            app.provider_setup_state,
            super::super::provider_setup::ProviderSetupState::OutcomeUnknown
        ));
        Box::pin(app.handle_event(&mut tui, &mut server, AppEvent::OpenProviderSetup)).await?;
        assert!(
            !app.chat_widget.no_modal_or_popup_active(),
            "provider retry remains available"
        );
        Ok::<(), color_eyre::Report>(())
    })
    .catch_unwind()
    .await;
    finish_provider_transition_test(app, server, outcome).await
}

#[tokio::test]
async fn successful_transition_opens_model_picker_snapshot() -> Result<()> {
    let (mut app, mut events, _) = make_test_app_with_channels().await;
    disable_fixture_plugins(&mut app)?;
    let mut server = start_config_write_test_app_server(&app).await?;
    let outcome = std::panic::AssertUnwindSafe(async {
        let mut tui = crate::tui::test_support::make_test_tui()?;
        let started = server.start_thread(&app.config).await?;
        app.replace_chat_widget_with_app_server_thread(
            &mut tui,
            started,
            super::super::session_lifecycle::ThreadAttachPresentation::Fresh,
            /*initial_user_message*/ None,
        )
        .await?;
        let old_thread_id = app.current_displayed_thread_id().expect("attached thread");
        assert!(
            !app.agents_overview
                .blank_sessions
                .contains_key(&old_thread_id)
        );
        app.chat_widget
            .set_queue_autosend_suppressed(/*suppressed*/ true);
        for text in ["first queued", "second queued"] {
            app.chat_widget.queue_user_message_with_options(
                text.into(),
                crate::bottom_pane::QueuedInputAction::Plain,
                Vec::new(),
            );
        }
        app.chat_widget
            .restore_user_message_to_composer("old draft".into());
        let old_input = app
            .chat_widget
            .capture_thread_input_state()
            .expect("old input");
        while events.try_recv().is_ok() {}
        let provider_id = app.chat_widget.config_ref().model_provider_id.clone();
        app.provider_transition_blocked = true;
        app.chat_widget.block_provider_transition();
        app.finish_model_provider_setup(
            &mut tui,
            &mut server,
            ProviderSetupResult {
                baseline: None,
                result: Ok(codex_app_server_protocol::ModelProviderConfigureResponse {
                    provider_id,
                    base_url: "https://example.test/v1".to_string(),
                    model: None,
                }),
            },
        )
        .await;
        assert!(!app.provider_transition_blocked);
        assert_app_snapshot!(
            "provider_transition_success",
            render_bottom_popup(&app.chat_widget, /*width*/ 80)
        );
        assert_ne!(app.current_displayed_thread_id(), Some(old_thread_id));
        assert_eq!(
            app.agents_overview.input_states.get(&old_thread_id),
            Some(&old_input)
        );
        assert!(app.chat_widget.queued_user_message_texts().is_empty());
        assert_eq!(app.chat_widget.composer_text_with_pending(), "");
        assert!(
            !std::iter::from_fn(|| events.try_recv().ok())
                .any(|event| matches!(event, AppEvent::CodexOp(Op::UserTurn { .. })))
        );
        app.chat_widget.restore_thread_input_state(
            Some(old_input.clone()),
            crate::chatwidget::ThreadInputStateRestoreMode {
                preserve_in_flight_turn: false,
            },
        );
        assert_eq!(
            app.chat_widget.queued_user_message_texts(),
            vec!["first queued", "second queued"]
        );
        assert_eq!(app.chat_widget.composer_text_with_pending(), "old draft");
        assert_eq!(
            app.chat_widget.capture_thread_input_state(),
            Some(old_input)
        );
        Ok::<(), color_eyre::Report>(())
    })
    .catch_unwind()
    .await;
    finish_provider_transition_test(app, server, outcome).await
}
