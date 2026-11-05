//! Submit once, retain the receipt, and correlate only a validated exact-scope
//! Location. A transport trace UUID alone is never promoted to a refresh ID.

use anyhow::Result;
use reqwest::header::HeaderMap;
use reqwest::{StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::client::{
    FabricClient, PowerBiAcknowledgement, is_secure_or_loopback, validate_trusted_url,
};
use crate::commands::diagnostics::{error_evidence, uuid_target};
use crate::errors::{ErrorCode, FabioError, HintType};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Submission {
    pub state: &'static str,
    pub http_status: Option<u16>,
    pub refresh_id: Option<String>,
    pub correlation_status: &'static str,
    pub correlation_reason: Option<&'static str>,
    pub response_body_status: &'static str,
    pub transport_request_id: Option<String>,
    pub completion_status: &'static str,
    pub error: Option<Value>,
    pub warnings: Vec<&'static str>,
}

impl Submission {
    pub const fn skipped() -> Self {
        Self {
            state: "skipped",
            http_status: None,
            refresh_id: None,
            correlation_status: "not_requested",
            correlation_reason: None,
            response_body_status: "not_requested",
            transport_request_id: None,
            completion_status: "not_checked",
            error: None,
            warnings: Vec::new(),
        }
    }

    pub fn failed(error: &anyhow::Error) -> Self {
        let rejected = error.downcast_ref::<FabioError>().is_some_and(|error| {
            matches!(
                error.code,
                ErrorCode::InvalidInput
                    | ErrorCode::ReadonlyMode
                    | ErrorCode::AuthRequired
                    | ErrorCode::Forbidden
                    | ErrorCode::NotFound
                    | ErrorCode::RateLimited
                    | ErrorCode::CapacityInactive
                    | ErrorCode::Conflict
            )
        });
        Self {
            state: if rejected { "rejected" } else { "unknown" },
            http_status: None,
            refresh_id: None,
            correlation_status: "unavailable",
            correlation_reason: Some(if rejected {
                "submission_rejected"
            } else {
                "submission_outcome_unknown"
            }),
            response_body_status: "unavailable",
            transport_request_id: None,
            completion_status: "not_checked",
            error: Some(error_evidence(error)),
            warnings: vec![
                "Do not automatically resubmit. An unknown transport outcome may already have started work; inspect existing refresh evidence before deciding what to do.",
            ],
        }
    }

    pub fn acknowledged(&self) -> bool {
        matches!(self.state, "accepted" | "acknowledged")
    }
}

fn one_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> std::result::Result<Option<&'a str>, &'static str> {
    let mut values = headers.get_all(name).iter();
    let Some(value) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err("duplicate_header");
    }
    let value = value
        .to_str()
        .ok()
        .filter(|value| !value.is_empty() && value.len() <= 8192)
        .ok_or("malformed_header")?;
    Ok(Some(value))
}

fn nonzero_uuid(value: &str) -> Option<Uuid> {
    (value.len() == 36)
        .then(|| uuid_target(value, "acknowledgement UUID").ok())
        .flatten()
}

fn location_id(location: &str, endpoint: &str, workspace: Uuid, model: Uuid) -> Option<Uuid> {
    // Reject normalization tricks, credentials and arbitrary origins rather
    // than extracting the final segment from a plausible-looking URL.
    if location.trim() != location
        || location.chars().any(char::is_control)
        || location.contains(['%', '\\'])
        || location.split('/').any(|part| matches!(part, "." | ".."))
    {
        return None;
    }
    let endpoint = Url::parse(endpoint).ok()?;
    let path = format!(
        "{}/groups/{workspace}/datasets/{model}/refreshes",
        endpoint.path().trim_end_matches('/')
    );
    let request = endpoint.join(&path).ok()?;
    let location = request.join(location).ok()?;
    let regional_service = endpoint.scheme() == "https"
        && endpoint.host_str() == Some("api.powerbi.com")
        && endpoint.port_or_known_default() == Some(443)
        && location.scheme() == "https"
        && location.port_or_known_default() == Some(443)
        && location
            .host_str()
            .is_some_and(|host| host.ends_with(".analysis.windows.net"))
        && validate_trusted_url(location.as_str(), "refresh acknowledgement").is_ok();
    if !is_secure_or_loopback(location.as_str())
        || (location.origin() != endpoint.origin() && !regional_service)
        || !location.username().is_empty()
        || location.password().is_some()
        || location.query().is_some()
        || location.fragment().is_some()
    {
        return None;
    }
    let segments: Vec<_> = location.path_segments()?.collect();
    let base: Vec<_> = endpoint
        .path_segments()?
        .filter(|segment| !segment.is_empty())
        .collect();
    if segments.len() != base.len() + 6 || segments[..base.len()] != base {
        return None;
    }
    let tail = &segments[base.len()..];
    if tail[0] != "groups"
        || nonzero_uuid(tail[1]) != Some(workspace)
        || tail[2] != "datasets"
        || nonzero_uuid(tail[3]) != Some(model)
        || tail[4] != "refreshes"
    {
        return None;
    }
    nonzero_uuid(tail[5])
}

fn receipt(
    ack: &PowerBiAcknowledgement,
    endpoint: &str,
    workspace: Uuid,
    model: Uuid,
) -> Submission {
    let mut result = Submission {
        state: if ack.status == StatusCode::ACCEPTED {
            "accepted"
        } else {
            "acknowledged"
        },
        http_status: Some(ack.status.as_u16()),
        refresh_id: None,
        correlation_status: "unavailable",
        correlation_reason: Some("missing_location"),
        response_body_status: match &ack.body {
            None => "unreadable",
            Some(Value::Null) => "empty",
            Some(_) => "json",
        },
        transport_request_id: None,
        completion_status: "not_checked",
        error: None,
        warnings: Vec::new(),
    };
    let location = one_header(&ack.headers, "location");
    let trace = one_header(&ack.headers, "x-ms-request-id");
    match (location, trace) {
        (Err(reason), _) | (_, Err(reason)) => {
            result.correlation_status = "invalid";
            result.correlation_reason = Some(reason);
        }
        (Ok(location), Ok(trace)) => {
            let trace_id = trace.and_then(nonzero_uuid);
            result.transport_request_id = trace_id.map(|value| value.to_string());
            if trace.is_some() && trace_id.is_none() {
                result.correlation_status = "invalid";
                result.correlation_reason = Some("malformed_transport_request_id");
            } else if let Some(location) = location {
                if let Some(refresh) = location_id(location, endpoint, workspace, model) {
                    if trace_id.is_some_and(|trace| trace != refresh) {
                        result.correlation_status = "conflicting";
                        result.correlation_reason = Some("header_identity_conflict");
                    } else {
                        result.refresh_id = Some(refresh.to_string());
                        result.correlation_status = "verified";
                        result.correlation_reason = None;
                    }
                } else {
                    result.correlation_status = "invalid";
                    result.correlation_reason = Some("location_scope_or_origin_invalid");
                }
            }
        }
    }
    if ack
        .body
        .as_ref()
        .is_some_and(|body| body.get("error").is_some_and(|error| !error.is_null()))
    {
        result.state = "unknown";
        result.refresh_id = None;
        result.correlation_status = "conflicting";
        result.correlation_reason = Some("error_bearing_acknowledgement");
        result.response_body_status = "error";
    }
    if result.refresh_id.is_none() {
        result.warnings.push("No verified refresh identity was established. Preserve this acknowledgement; do not resubmit accepted or ambiguously acknowledged work. Correlate existing history against a pre-submission baseline when available.");
    }
    if ack.body.is_none() {
        result.warnings.push("The server acknowledged the request but its response body was unreadable. This is not permission to retry; completion remains unverified.");
    }
    result
}

pub(super) async fn submit(
    client: &FabricClient,
    workspace: &str,
    model: &str,
    body: &Value,
) -> Result<Submission> {
    let workspace = uuid_target(workspace, "--workspace")?;
    let model = uuid_target(model, "--id")?;
    let response = client
        .post_powerbi_with_metadata(
            &format!("/groups/{workspace}/datasets/{model}/refreshes"),
            body,
        )
        .await
        .map_err(submission_error)?;
    Ok(receipt(
        &response,
        FabricClient::powerbi_endpoint(),
        workspace,
        model,
    ))
}

fn submission_error(error: anyhow::Error) -> anyhow::Error {
    if let Some(fabio) = error.downcast_ref::<FabioError>()
        && matches!(
            fabio.code,
            ErrorCode::NetworkError | ErrorCode::Timeout | ErrorCode::ApiError
        )
    {
        return fabio.with_replaced_hint("Refresh submission outcome may be unknown. Do not retry automatically: inspect existing refresh evidence and correlate a request before submitting any further work.",Some(HintType::SemanticCorrection)).set_retriable(Some(false)).into();
    }
    error
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const MODEL: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const REFRESH: &str = "cccccccc-1111-2222-3333-444444444444";
    const OTHER: &str = "dddddddd-1111-2222-3333-444444444444";
    const ENDPOINT: &str = "https://api.powerbi.com/v1.0/myorg";

    fn ack(location: Option<&str>, trace: Option<&str>) -> PowerBiAcknowledgement {
        let mut headers = HeaderMap::new();
        if let Some(value) = location {
            headers.insert("location", value.parse().unwrap());
        }
        if let Some(value) = trace {
            headers.insert("x-ms-request-id", value.parse().unwrap());
        }
        PowerBiAcknowledgement {
            status: StatusCode::ACCEPTED,
            headers,
            body: Some(Value::Null),
        }
    }

    #[test]
    fn exact_location_is_verified_but_transport_trace_alone_is_not() {
        let path = format!("/v1.0/myorg/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}");
        for location in [
            path.clone(),
            format!("https://api.powerbi.com{path}"),
            format!("https://example.analysis.windows.net{path}"),
        ] {
            let result = receipt(
                &ack(Some(&location), Some(REFRESH)),
                ENDPOINT,
                WS.parse().unwrap(),
                MODEL.parse().unwrap(),
            );
            assert_eq!(result.refresh_id.as_deref(), Some(REFRESH));
            assert_eq!(result.correlation_status, "verified");
            assert_eq!(result.state, "accepted");
            assert_eq!(result.completion_status, "not_checked");
        }
        let result = receipt(
            &ack(None, Some(REFRESH)),
            ENDPOINT,
            WS.parse().unwrap(),
            MODEL.parse().unwrap(),
        );
        assert_eq!(result.refresh_id, None);
        assert_eq!(result.correlation_reason, Some("missing_location"));
    }

    #[test]
    fn malformed_conflicting_and_wrong_scope_locations_are_never_followed_or_echoed() {
        let path = format!("/v1.0/myorg/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}");
        for location in [
            format!("https://example.org{path}"),
            format!("{path}?secret=private"),
            format!("{path}#private"),
            format!("/v1.0/myorg/groups/{OTHER}/datasets/{MODEL}/refreshes/{REFRESH}"),
            format!("{path}/extra"),
            format!("{path}/../{REFRESH}"),
            format!("{path}%2fprivate"),
            format!("https://private@api.powerbi.com{path}"),
            format!("https://example.analysis.windows.net:444{path}"),
            format!("https://example.analysis.windows.net.example.org{path}"),
        ] {
            let result = receipt(
                &ack(Some(&location), Some(REFRESH)),
                ENDPOINT,
                WS.parse().unwrap(),
                MODEL.parse().unwrap(),
            );
            assert_eq!(result.refresh_id, None);
            assert_eq!(result.correlation_status, "invalid");
            assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        }
        let result = receipt(
            &ack(Some(&path), Some(OTHER)),
            ENDPOINT,
            WS.parse().unwrap(),
            MODEL.parse().unwrap(),
        );
        assert_eq!(result.correlation_status, "conflicting");
        assert_eq!(result.refresh_id, None);
        assert_eq!(
            location_id(
                &format!("https://example.analysis.windows.net{path}"),
                "https://example.org/v1.0/myorg",
                WS.parse().unwrap(),
                MODEL.parse().unwrap()
            ),
            None
        );
    }

    #[test]
    fn unreadable_success_and_duplicate_headers_do_not_invite_resubmission() {
        let path = format!("/v1.0/myorg/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}");
        let mut response = ack(Some(&path), None);
        response.body = None;
        let result = receipt(
            &response,
            ENDPOINT,
            WS.parse().unwrap(),
            MODEL.parse().unwrap(),
        );
        assert_eq!(result.state, "accepted");
        assert_eq!(result.refresh_id.as_deref(), Some(REFRESH));
        assert_eq!(result.response_body_status, "unreadable");
        let mut response = ack(Some(&path), None);
        response.headers.append("location", path.parse().unwrap());
        let result = receipt(
            &response,
            ENDPOINT,
            WS.parse().unwrap(),
            MODEL.parse().unwrap(),
        );
        assert_eq!(result.refresh_id, None);
        assert_eq!(result.correlation_reason, Some("duplicate_header"));
    }

    #[test]
    fn generator_failure_disposition_keeps_unknown_outcomes_and_errors_sanitized() {
        let error = anyhow::Error::from(FabioError::new(
            ErrorCode::NetworkError,
            "private endpoint and payload",
        ));
        let result = Submission::failed(&error);
        assert_eq!(result.state, "unknown");
        assert!(!result.acknowledged());
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
        let error = submission_error(FabioError::new(ErrorCode::NetworkError, "private").into());
        let typed = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(typed.retriable, Some(false));
        assert_eq!(typed.hint_type, Some(HintType::SemanticCorrection));
        let error = anyhow::Error::from(FabioError::new(ErrorCode::Forbidden, "private"));
        assert_eq!(Submission::failed(&error).state, "rejected");
        assert_eq!(Submission::skipped().state, "skipped");
        let path = format!("/v1.0/myorg/groups/{WS}/datasets/{MODEL}/refreshes/{REFRESH}");
        let mut response = ack(Some(&path), None);
        response.body = Some(json!({"error":{"message":"private"}}));
        let result = receipt(
            &response,
            ENDPOINT,
            WS.parse().unwrap(),
            MODEL.parse().unwrap(),
        );
        assert_eq!(result.state, "unknown");
        assert_eq!(result.refresh_id, None);
        assert!(!serde_json::to_string(&result).unwrap().contains("private"));
    }
}
