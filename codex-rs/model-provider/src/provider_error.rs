use codex_api::ApiError;
use codex_api::TransportError;
use codex_protocol::error::CodexErr;
use http::StatusCode;
use serde_json::Value;

const WITHHELD: &str = "Provider response details withheld";
const INVALID_IMAGE: &str = "The image data you provided does not represent a valid image";

// Reconstruct only codes used by the existing mapper; no response text or metadata
// is forwarded. Keep both allowed fields for 429 so its established precedence applies.
fn safe_http_body(status: StatusCode, body: Option<&str>) -> String {
    if let Some(body) = body {
        if let Ok(parsed) = serde_json::from_str::<Value>(body)
            && let Some(error) = parsed.get("error")
        {
            let code = error.get("code").and_then(Value::as_str);
            let kind = error.get("type").and_then(Value::as_str);
            let code = match (status, code) {
                (
                    StatusCode::BAD_REQUEST,
                    Some(
                        code @ ("invalid_prompt"
                        | "cyber_policy"
                        | "bio_policy"
                        | "misalignment_policy_violation"),
                    ),
                )
                | (StatusCode::FORBIDDEN, Some(code @ "misalignment_policy_violation"))
                | (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Some(code @ ("server_is_overloaded" | "slow_down")),
                )
                | (
                    StatusCode::TOO_MANY_REQUESTS,
                    Some(
                        code @ ("flex_unavailable"
                        | "insufficient_quota"
                        | "credit_balance_exhausted"
                        | "organization_spend_limit_exceeded"
                        | "project_spend_limit_exceeded"
                        | "organization_usage_limit_exceeded"),
                    ),
                ) => Some(code),
                _ => None,
            };
            let kind = match (status, kind) {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Some(
                        kind
                        @ ("usage_limit_reached" | "usage_not_included" | "insufficient_quota"),
                    ),
                ) => Some(kind),
                _ => None,
            };
            if code.is_some() || kind.is_some() {
                let message = (status == StatusCode::SERVICE_UNAVAILABLE
                    && code == Some("slow_down"))
                .then_some("Provider rate limit exceeded.");
                return serde_json::json!({"error": {"code": code, "type": kind, "message": message}}).to_string();
            }
        }
        if status == StatusCode::BAD_REQUEST && body.contains(INVALID_IMAGE) {
            return INVALID_IMAGE.to_owned();
        }
    }
    format!("{WITHHELD} (HTTP {status})")
}

// Preserve the existing error categories and retry advice, but discard text and
// metadata controlled by an untrusted endpoint before the standard mapper sees them.
pub(crate) fn map_untrusted_error(error: ApiError) -> CodexErr {
    let error = match error {
        ApiError::Transport(TransportError::Http {
            status,
            body,
            retry_after,
            ..
        }) => ApiError::Transport(TransportError::Http {
            status,
            url: None,
            headers: None,
            body: Some(safe_http_body(status, body.as_deref())),
            retry_after,
        }),
        ApiError::Api { status, .. } => ApiError::Api {
            status,
            message: WITHHELD.into(),
        },
        ApiError::Stream(_) => ApiError::Stream(WITHHELD.into()),
        ApiError::Retryable { retry_after, .. } => ApiError::Retryable {
            message: WITHHELD.into(),
            retry_after,
        },
        ApiError::RateLimitExceeded { retry_after, .. } => ApiError::RateLimitExceeded {
            message: WITHHELD.into(),
            retry_after,
        },
        ApiError::RateLimit(_) => ApiError::RateLimit(WITHHELD.into()),
        ApiError::InvalidRequest { .. } => ApiError::InvalidRequest {
            message: WITHHELD.into(),
        },
        ApiError::InvalidPrompt { .. } => ApiError::InvalidPrompt {
            message: WITHHELD.into(),
        },
        ApiError::CyberPolicy { .. } => ApiError::CyberPolicy {
            message: WITHHELD.into(),
        },
        ApiError::BioPolicy { .. } => ApiError::BioPolicy {
            message: WITHHELD.into(),
        },
        ApiError::MisalignmentPolicyViolation { .. } => ApiError::MisalignmentPolicyViolation {
            message: WITHHELD.into(),
            misalignment: None,
        },
        ApiError::ResponseIncomplete(mut incomplete) => {
            incomplete.reason = WITHHELD.into();
            incomplete.response_id = None;
            ApiError::ResponseIncomplete(incomplete)
        }
        ApiError::Transport(TransportError::Network(_)) => {
            ApiError::Transport(TransportError::Network(WITHHELD.into()))
        }
        ApiError::Transport(TransportError::Build(_)) => {
            ApiError::Transport(TransportError::Build(WITHHELD.into()))
        }
        safe @ (ApiError::ContentFilter
        | ApiError::ContextWindowExceeded
        | ApiError::QuotaExceeded
        | ApiError::UsageNotIncluded
        | ApiError::FlexUnavailable
        | ApiError::ServerOverloaded { .. }
        | ApiError::Transport(
            TransportError::Policy(_)
            | TransportError::RetryLimit
            | TransportError::Timeout
            | TransportError::Connection(_)
            | TransportError::ResponseTooLarge { .. },
        )) => safe,
    };
    codex_api::map_api_error(error)
}

#[cfg(test)]
#[path = "provider_error_tests.rs"]
mod tests;
