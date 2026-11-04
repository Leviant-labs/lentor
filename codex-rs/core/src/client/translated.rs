//! Translated HTTP execution using the reviewed Chat and Messages codec boundary.

use super::*;
use codex_api::TranslatedApi;
use codex_api::TranslatedStreamClient;
use codex_history::NativeProviderState;
use codex_provider_codecs::CodecStream;
use codex_provider_codecs::Protocol;
use codex_provider_codecs::encode_request;

const MAX_STAGED_EVENTS: usize = 16_384;

impl ModelClientSession {
    #[allow(clippy::too_many_arguments)]
    #[tracing::instrument(name = "model_client.stream_translated", skip_all)]
    pub(super) async fn stream_translated(
        &self,
        prompt: &Prompt,
        model_info: &ModelInfo,
        session_telemetry: &SessionTelemetry,
        effort: Option<ReasoningEffortConfig>,
        summary: ReasoningSummaryConfig,
        service_tier: Option<String>,
        responses_metadata: &CodexResponsesMetadata,
        inference_trace: &InferenceTraceContext,
    ) -> Result<ResponseStream> {
        let (protocol, api, path) = match self.client.state.provider.info().wire_api {
            WireApi::Chat => (Protocol::Chat, TranslatedApi::Chat, "/chat/completions"),
            WireApi::Messages => (Protocol::Messages, TranslatedApi::Messages, "/messages"),
            WireApi::Responses => {
                return Err(CodexErr::InvalidRequest(
                    "translated protocol required".to_string(),
                ));
            }
        };
        let (native_protocol, identity) = self.client.native_state_scope().ok_or_else(|| {
            CodexErr::InvalidRequest("translated provider state has not been hydrated".to_string())
        })?;
        if !matches!(
            (protocol, native_protocol),
            (Protocol::Chat, codex_history::NativeProviderProtocol::Chat)
                | (
                    Protocol::Messages,
                    codex_history::NativeProviderProtocol::Messages
                )
        ) {
            return Err(CodexErr::InvalidRequest(
                "translated provider state protocol changed".to_string(),
            ));
        }
        let auth_manager = self.client.state.provider.auth_manager();
        let mut auth_recovery = auth_manager
            .as_ref()
            .map(AuthManager::unauthorized_recovery);
        let mut provider_auth_recovery_attempted = false;
        let mut pending_retry = PendingUnauthorizedRetry::default();
        loop {
            let setup = self
                .client
                .current_client_setup(ClientRouting::Workspace)
                .await?;
            let transport = self.client.build_api_transport(
                &setup.api_provider,
                path,
                setup.redirect_policy,
            )?;
            let auth_context = AuthRequestTelemetryContext::new(
                setup.auth.as_ref().map(CodexAuth::auth_mode),
                setup.api_auth.as_ref(),
                setup.agent_identity_telemetry.clone(),
                pending_retry,
            )
            .map_err(|error| self.client.state.provider.map_api_error(error))?;
            let (request_telemetry, _sse_telemetry) = Self::build_streaming_telemetry(
                session_telemetry,
                auth_context,
                RequestRouteTelemetry::for_endpoint(path),
                self.client.state.auth_env_telemetry.clone(),
                self.client.state.provider.include_response_debug(),
            );
            let include_internal = self
                .client
                .state
                .provider
                .include_internal_metadata(&setup.api_provider);
            let mut request = self.client.build_responses_request(
                prompt,
                model_info,
                effort.clone(),
                summary,
                service_tier.clone(),
                responses_metadata,
                include_internal,
            )?;
            request.access_programs = cyber_access_program::for_auth(
                setup.auth.as_ref(),
                prompt.cyber_access_program,
                self.client.api_key_cyber_access_programs,
            )?;
            self.client
                .prepare_response_items_for_request(&mut request.input);
            let request_session_telemetry =
                session_telemetry_for_request(session_telemetry, &request);
            let responses_headers = self
                .client
                .responses_headers(setup.auth.as_ref(), &model_info.slug);
            self.client.set_guardian_metadata(
                &mut request.client_metadata,
                responses_metadata.parent_response_id.as_deref(),
                setup.auth.as_ref(),
                &responses_headers,
            );
            let mut options = ResponsesHttpExecutor::new(&self.client)
                .build_responses_options(
                    &self.turn_state,
                    responses_metadata,
                    Compression::None,
                    model_info.use_responses_lite,
                )
                .await;
            if let Some(value) = self.client.build_routing_hint_header(
                setup.auth.as_ref(),
                &responses_headers,
                &request.model,
                request.service_tier.as_deref(),
            ) {
                options
                    .extra_headers
                    .insert(X_CODEX_ROUTING_HINT_HEADER, value);
            }
            options.extra_headers.extend(responses_headers);
            if let Some(thread_id) = options.thread_id.as_deref()
                && let Ok(value) = HeaderValue::from_str(thread_id)
            {
                options.extra_headers.insert("x-client-request-id", value);
            }
            options
                .extra_headers
                .extend(build_session_headers(options.session_id, options.thread_id));
            if let Some(subagent) = subagent_header_value(&self.client.state.session_source)
                && let Ok(value) = HeaderValue::from_str(&subagent)
            {
                options
                    .extra_headers
                    .insert(X_OPENAI_SUBAGENT_HEADER, value);
            }
            let interceptors = crate::model_request::prepare(
                &self.client.request_contributors,
                &self.client.state.thread_id.to_string(),
                &model_info.slug,
                codex_extension_api::ModelRequestKind::Generation,
                &mut request.client_metadata,
            );
            let mut headers = options.extra_headers;
            let attempt = inference_trace.start_attempt();
            attempt.add_request_headers(&mut headers);
            if let Some(input) = tool_metadata::bounded_input(&request, &request.input) {
                if let Some(recorder) = &self.client.executed_tool_calls {
                    recorder.invalidate_wire_inventory_loss(&request.input, &input);
                }
                request.input = input;
            }
            let native_records = self
                .client
                .native_records()
                .into_iter()
                .filter(|record| {
                    request
                        .input
                        .iter()
                        .any(|item| item.id().is_some_and(|id| id.as_str() == record.item_id))
                })
                .collect::<Vec<_>>();
            let encoded =
                encode_request(protocol, &identity, &request, &native_records).map_err(|error| {
                    let detail = error.to_string();
                    // Classify known static failures without echoing provider
                    // content, tool descriptions or opaque native payloads.
                    let message = match detail.as_str() {
                        "strict function tools cannot be preserved across protocols" => "strict function tools are unsupported by the selected provider protocol",
                        "unsupported tool declaration field" => "deferred or unsupported tool declarations are unsupported by the selected provider protocol",
                        "unsupported reasoning or text control" => "reasoning or text controls are unsupported by the selected provider protocol",
                        "unsupported provider request control" => "service tier, storage or stream controls are unsupported by the selected provider protocol",
                        "unsupported include field" => "encrypted output replay is unsupported by the selected provider protocol",
                        "encoded tools exceed context limit" | "tool declaration exceeds limit" | "namespace description exceeds tool limit" => "tool definitions exceed the selected provider protocol's context limit",
                        "invalid or unsupported tool field" | "unsupported freeform format" => "a tool declaration uses an unsupported format for the selected provider protocol",
                        "encrypted input cannot be translated" => "this history contains encrypted content that the selected provider protocol cannot replay",
                        "native reasoning state missing" | "Chat reasoning provider state missing" => "provider-native reasoning state is missing from this session",
                        "native state does not match input" => "provider-native reasoning state has no matching conversation item",
                        "invalid native reasoning block" | "invalid native reasoning blocks" | "unsupported native reasoning block" => "this session contains an invalid provider-native reasoning block",
                        "native state provider mismatch" | "invalid native state envelope" => "provider-native reasoning state does not match the selected provider",
                        _ if detail.contains("Anthropic has no place for a system message after the conversation starts") => "Messages cannot replay a system or developer update after the conversation starts",
                        _ if detail.starts_with("unsupported tool type ") => "a requested tool type is unsupported by the selected provider protocol",
                        _ if detail.starts_with("unsupported input item ") => "this history contains an item that the selected provider protocol cannot replay",
                        _ => "provider request cannot be translated",
                    };
                    CodexErr::InvalidRequest(message.to_string())
                })?;
            attempt.record_started(&request);
            let idle_timeout = setup.api_provider.stream_idle_timeout;
            let client = TranslatedStreamClient::new(transport, setup.api_provider, setup.api_auth)
                .with_telemetry(Some(request_telemetry));
            match client.stream(api, &encoded.body, headers).await {
                Ok(raw) => {
                    let upstream_id = raw
                        .headers
                        .get("x-request-id")
                        .and_then(|value| value.to_str().ok())
                        .map(str::to_string);
                    let (tx, rx) = mpsc::channel(RESPONSE_STREAM_CHANNEL_CAPACITY);
                    let model_client = self.client.clone();
                    let identity = identity.clone();
                    tokio::spawn(async move {
                        let mut bytes = raw.bytes;
                        let mut codec = CodecStream::new(protocol, &identity, &encoded);
                        let mut events = Vec::new();
                        let mut states = Vec::new();
                        loop {
                            let chunk = tokio::select! {
                                _ = tx.closed() => return,
                                chunk = tokio::time::timeout(idle_timeout, bytes.next()) => chunk,
                            };
                            let completed = matches!(&chunk, Ok(None));
                            let batch = match chunk {
                                Ok(Some(Ok(chunk))) => codec.push(&chunk),
                                Ok(None) => codec.finish(),
                                Ok(Some(Err(_))) | Err(_) => {
                                    let _ = tx
                                        .send(Err(ApiError::Stream(
                                            "provider stream interrupted".to_string(),
                                        )))
                                        .await;
                                    return;
                                }
                            };
                            let batch = match batch {
                                Ok(batch) => batch,
                                Err(_) => {
                                    let _ = tx
                                        .send(Err(ApiError::Stream(
                                            "invalid provider stream".to_string(),
                                        )))
                                        .await;
                                    return;
                                }
                            };
                            for event in batch.events {
                                let safe_to_stream = matches!(
                                    &event,
                                    ResponseEvent::Created { .. }
                                        | ResponseEvent::ServerModel(_)
                                        | ResponseEvent::OutputTextDelta(_)
                                        | ResponseEvent::OutputItemAdded(
                                            ResponseItem::Message { .. }
                                        )
                                );
                                if safe_to_stream && !completed && events.is_empty() {
                                    if tx.send(Ok(event)).await.is_err() {
                                        return;
                                    }
                                } else {
                                    events.push(event);
                                }
                            }
                            states.extend(batch.native_state);
                            if events.len() > MAX_STAGED_EVENTS {
                                let _ = tx
                                    .send(Err(ApiError::Stream(
                                        "provider stream exceeds event limit".to_string(),
                                    )))
                                    .await;
                                return;
                            }
                            if completed {
                                break;
                            }
                        }
                        let mut records = Vec::new();
                        for state in states {
                            if !events.iter().any(|event| {
                                matches!(event, ResponseEvent::OutputItemDone(item) if item.id().is_some_and(|id| id.as_str() == state.item_id))
                            }) {
                                let _ = tx.send(Err(ApiError::Stream("provider native state has no matching item".to_string()))).await;
                                return;
                            }
                            let record = NativeProviderState::new(
                                native_protocol,
                                identity.clone(),
                                state.item_id,
                                state.payload,
                            );
                            let Ok(record) = record else {
                                let _ = tx
                                    .send(Err(ApiError::Stream(
                                        "invalid provider native state".to_string(),
                                    )))
                                    .await;
                                return;
                            };
                            records.push(record);
                        }
                        if model_client.insert_native_states(records).is_err() {
                            let _ = tx
                                .send(Err(ApiError::Stream(
                                    "invalid provider native state".to_string(),
                                )))
                                .await;
                            return;
                        }
                        for event in events {
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    });
                    let stream = codex_api::ResponseStream {
                        rx_event: rx,
                        upstream_request_id: upstream_id,
                        interrupt: None,
                    };
                    let (stream, _) = map_response_stream(
                        stream,
                        request_session_telemetry,
                        attempt,
                        Arc::clone(&self.client.state.provider),
                        interceptors,
                    );
                    return Ok(stream);
                }
                Err(ApiError::Transport(error))
                    if self.client.state.provider.is_recoverable_auth_error(&error) =>
                {
                    attempt.record_failed(
                        "provider authentication failed",
                        /*upstream_request_id*/ None,
                        /*output_items*/ &[],
                    );
                    pending_retry = PendingUnauthorizedRetry::from_recovery(
                        handle_unauthorized(
                            error,
                            &mut auth_recovery,
                            &mut provider_auth_recovery_attempted,
                            session_telemetry,
                            &self.client.state.provider,
                            self.client.event_sender.as_ref(),
                            responses_metadata.turn_id.as_deref(),
                        )
                        .await?,
                    );
                }
                Err(error) => {
                    let error = self.client.state.provider.map_api_error(error);
                    attempt.record_failed(
                        &error,
                        /*upstream_request_id*/ None,
                        /*output_items*/ &[],
                    );
                    return Err(error);
                }
            }
        }
    }
}
