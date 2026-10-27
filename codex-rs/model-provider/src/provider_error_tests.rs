use super::map_untrusted_error;
use codex_api::ApiError;
use codex_api::TransportError;
use codex_http_client::RetryAfter;
use codex_model_provider_info::ModelCatalogFormat;
use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::CodexErrorInfo;
use http::HeaderMap;
use http::HeaderValue;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::json;
use std::time::Duration;

#[test]
fn http_error_diagnostics_do_not_include_provider_controlled_content() {
    const SECRET: &str = "fixture-endpoint-secret";
    for (status, body) in [
        (StatusCode::BAD_REQUEST, format!("Bearer {SECRET}")),
        (StatusCode::FORBIDDEN, format!(r#"{{"error":{{"code":"cyber_policy","message":"{SECRET}"}}}}"#)),
        (StatusCode::TOO_MANY_REQUESTS, format!(r#"{{"error":{{"message":"{SECRET}"}}}}"#)),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static(SECRET));
        let error = map_untrusted_error(ApiError::Transport(TransportError::Http {
            status,
            url: Some(format!("https://example.test/{SECRET}")),
            headers: Some(headers),
            body: Some(body),
            retry_after: None,
        }));
        assert!(!format!("{error:?} {error}").contains(SECRET));
        assert!(error.to_string().contains(&status.as_u16().to_string()) || status == StatusCode::TOO_MANY_REQUESTS);
    }
    let error = map_untrusted_error(ApiError::Api {
        status: StatusCode::BAD_GATEWAY,
        message: SECRET.to_owned(),
    });
    assert!(!format!("{error:?} {error}").contains(SECRET));
}

#[test]
fn sanitizing_http_errors_preserves_classification_and_retry_advice() {
    let retry_after = RetryAfter::from_delay(Duration::from_secs(5)).unwrap();
    for status in [StatusCode::BAD_REQUEST, StatusCode::INTERNAL_SERVER_ERROR] {
        let error = map_untrusted_error(ApiError::Transport(TransportError::Http {
            status,
            url: None,
            headers: None,
            body: Some("secret".into()),
            retry_after: Some(retry_after),
        }));
        match status {
            StatusCode::BAD_REQUEST => {
                assert!(matches!(error.details(), CodexErrorDetails::InvalidRequest(_)));
                assert_eq!(error.retry_delay(/*retry_count*/ 1), None);
                assert!(error.to_string().contains("400"));
            }
            StatusCode::INTERNAL_SERVER_ERROR => {
                assert!(matches!(error.details(), CodexErrorDetails::InternalServerError));
                assert!(error.retry_delay(/*retry_count*/ 1).is_some());
            }
            _ => unreachable!(),
        }
        assert_eq!(error.retry_after(), Some(retry_after));
    }
}

#[test]
fn structured_http_errors_keep_classification_without_untrusted_details() {
    const SECRET: &str = "fixture-endpoint-secret";
    let retry_after = RetryAfter::from_delay(Duration::from_secs(30)).unwrap();
    for (status, code, kind, expected, retryable) in [
        (400, Some("invalid_prompt"), None, CodexErrorInfo::InvalidPrompt, false),
        (400, Some("cyber_policy"), None, CodexErrorInfo::CyberPolicy, false),
        (400, Some("bio_policy"), None, CodexErrorInfo::BioPolicy, false),
        (400, Some("misalignment_policy_violation"), None, CodexErrorInfo::MisalignmentPolicyViolation, false),
        (403, Some("misalignment_policy_violation"), None, CodexErrorInfo::MisalignmentPolicyViolation, false),
        (503, Some("server_is_overloaded"), None, CodexErrorInfo::ServerOverloaded, true),
        (503, Some("slow_down"), None, CodexErrorInfo::RateLimitExceeded, true),
        (429, Some("flex_unavailable"), Some("usage_limit_reached"), CodexErrorInfo::FlexUnavailable, false),
        (429, Some("insufficient_quota"), None, CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("credit_balance_exhausted"), None, CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("organization_spend_limit_exceeded"), None, CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("project_spend_limit_exceeded"), None, CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("organization_usage_limit_exceeded"), None, CodexErrorInfo::UsageLimitExceeded, false),
        (429, None, Some("insufficient_quota"), CodexErrorInfo::UsageLimitExceeded, false),
        (429, None, Some("usage_not_included"), CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("insufficient_quota"), Some("usage_limit_reached"), CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("insufficient_quota"), Some("usage_not_included"), CodexErrorInfo::UsageLimitExceeded, false),
        (429, Some("unknown"), Some("unknown"), CodexErrorInfo::ResponseTooManyFailedAttempts { http_status_code: Some(429) }, true),
    ] {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", HeaderValue::from_static(SECRET));
        headers.insert("x-openai-authorization-error", HeaderValue::from_static(SECRET));
        let body = json!({"error": {
            "code": code, "type": kind, "message": SECRET,
            "misalignment": {"explanation": SECRET}, "detail": SECRET
        }}).to_string();
        let error = map_untrusted_error(ApiError::Transport(TransportError::Http {
            status: StatusCode::from_u16(status).unwrap(),
            url: Some(format!("https://example.test/{SECRET}")),
            headers: Some(headers),
            body: Some(body),
            retry_after: Some(retry_after),
        }));
        assert_eq!(error.to_codex_protocol_error(), expected, "status {status}, code {code:?}, type {kind:?}");
        assert_eq!(error.retry_delay(/*retry_count*/ 1).is_some(), retryable);
        assert_eq!(error.retry_after(), Some(retry_after));
        assert!(!format!("{error:?} {error}").contains(SECRET));
        if status == 429 && code != Some("flex_unavailable") {
            match kind {
                Some("usage_limit_reached") => assert!(matches!(error.details(), CodexErrorDetails::UsageLimitReached(_))),
                Some("usage_not_included") => assert!(matches!(error.details(), CodexErrorDetails::UsageNotIncluded)),
                _ => {}
            }
        }
        if let CodexErrorDetails::MisalignmentPolicyViolation { misalignment, .. } = error.details() {
            assert!(misalignment.is_none());
        }
    }
}

#[test]
fn invalid_image_classification_does_not_forward_body() {
    let error = map_untrusted_error(ApiError::Transport(TransportError::Http {
        status: StatusCode::BAD_REQUEST,
        url: None,
        headers: None,
        body: Some("The image data you provided does not represent a valid image; fixture-secret".into()),
        retry_after: None,
    }));
    assert!(matches!(error.details(), CodexErrorDetails::InvalidImageRequest()));
    assert!(!format!("{error:?} {error}").contains("fixture-secret"));
}

#[test]
fn streamed_errors_keep_their_category_without_provider_text() {
    const SECRET: &str = "fixture-endpoint-secret";
    let retry_after = RetryAfter::from_delay(Duration::from_secs(5)).unwrap();
    let errors = [
        ApiError::Retryable { message: SECRET.into(), retry_after: Some(retry_after) },
        ApiError::RateLimitExceeded { message: SECRET.into(), retry_after: Some(retry_after) },
        ApiError::InvalidPrompt { message: SECRET.into() },
        ApiError::CyberPolicy { message: SECRET.into() },
        ApiError::Stream(SECRET.into()),
    ];
    for error in errors {
        let error = map_untrusted_error(error);
        assert!(!format!("{error:?} {error}").contains(SECRET));
    }
    let error = map_untrusted_error(ApiError::RateLimitExceeded {
        message: SECRET.into(),
        retry_after: Some(retry_after),
    });
    assert!(matches!(error.details(), CodexErrorDetails::RateLimitExceeded(_)));
    assert_eq!(error.retry_after(), Some(retry_after));
    let error = map_untrusted_error(ApiError::InvalidPrompt { message: SECRET.into() });
    assert!(matches!(error.details(), CodexErrorDetails::InvalidPrompt { .. }));
    assert_eq!(error.retry_delay(/*retry_count*/ 1), None);
}

#[test]
fn keyring_and_explicit_openai_catalog_use_the_same_untrusted_policy() {
    for info in [
        ModelProviderInfo {
            keyring_key: Some("a".repeat(64)),
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        },
        ModelProviderInfo {
            model_catalog_format: Some(ModelCatalogFormat::OpenAi),
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        },
    ] {
        let provider = crate::create_model_provider(info, /*auth_manager*/ None);
        assert!(!provider.include_response_debug());
        let mapped = provider.map_api_error(ApiError::InvalidPrompt { message: "fixture-secret".into() });
        assert!(matches!(mapped.details(), CodexErrorDetails::InvalidPrompt { .. }));
        assert!(!mapped.to_string().contains("fixture-secret"));
    }
    let trusted = crate::create_model_provider(
        ModelProviderInfo::create_openai_provider(/*base_url*/ None),
        /*auth_manager*/ None,
    );
    assert!(trusted.include_response_debug());
    let native = crate::create_model_provider(
        ModelProviderInfo {
            requires_openai_auth: false,
            model_catalog_format: Some(ModelCatalogFormat::Codex),
            ..ModelProviderInfo::create_openai_provider(/*base_url*/ None)
        },
        /*auth_manager*/ None,
    );
    assert!(native.include_response_debug());
}
