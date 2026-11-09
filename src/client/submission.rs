//! POST retry policy is semantic, never inferred from action-name substrings.
//! Ordinary mutation errors retain bounded evidence without authorizing replay.

use anyhow::Result;
use reqwest::{
    Response, StatusCode,
    header::{AUTHORIZATION, CONTENT_LENGTH, HeaderMap},
};
use serde_json::Value;

use super::FabricClient;
use crate::errors::{ErrorCode, FabioError, HintType, SubmissionEvidence};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PostSemantics {
    Mutation,
    ReadOnly,
}

impl FabricClient {
    /// One HTTP attempt. Mutation transport errors are deliberately not retry-safe.
    pub(super) async fn send_json_post(
        &self,
        url: &str,
        path: &str,
        body: Option<&Value>,
        semantics: PostSemantics,
        service: &'static str,
    ) -> Result<Response> {
        let token = self.require_auth().await?;
        let body_text = if crate::verbose::is_enabled() {
            body.map(Value::to_string)
        } else {
            None
        };
        crate::verbose::trace_request("POST", url, body_text.as_deref());
        let start = std::time::Instant::now();
        let request = self.http.post(url).header(AUTHORIZATION, token);
        let request = match body {
            Some(body) => request.json(body),
            None => request.header(CONTENT_LENGTH, "0"),
        };
        let response = request
            .send()
            .await
            .map_err(|error| network_error(&error, semantics, service, path))?;
        crate::verbose::trace_response(
            response.status().as_u16(),
            url,
            start.elapsed().as_millis(),
        );
        Ok(response)
    }

    /// Auth refresh is permitted only after an unambiguous authentication rejection.
    pub(super) async fn send_json_post_with_auth_refresh(
        &self,
        url: &str,
        path: &str,
        body: Option<&Value>,
        semantics: PostSemantics,
        service: &'static str,
    ) -> Result<Response> {
        let response = self
            .send_json_post(url, path, body, semantics, service)
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED
            && can_retry_rejection(semantics, response.headers())
        {
            self.invalidate_fabric_token().await;
            return self
                .send_json_post(url, path, body, semantics, service)
                .await;
        }
        Ok(response)
    }
}

const NO_REPLAY_HINT: &str = "Preserve the original request scope and this submission evidence. Do not automatically resubmit: inspect the existing operation/resource with read-only commands. An acknowledgement is not completion; a missing operation ID is not rejection.";

pub(super) fn has_acknowledgement(headers: &HeaderMap) -> bool {
    headers.contains_key("x-ms-operation-id") || headers.contains_key("location")
}

/// Known rejection responses can be retried only without contradictory ACK
/// markers. This does not grant 5xx replay to any mutation.
pub(super) fn can_retry_rejection(semantics: PostSemantics, headers: &HeaderMap) -> bool {
    semantics == PostSemantics::ReadOnly || !has_acknowledgement(headers)
}

pub(super) fn evidence(
    semantics: PostSemantics,
    service: &'static str,
    path: &str,
    status: Option<StatusCode>,
    headers: &HeaderMap,
) -> Option<SubmissionEvidence> {
    if semantics == PostSemantics::ReadOnly {
        return None;
    }
    let state = match status {
        Some(StatusCode::ACCEPTED) => "accepted",
        Some(status) if status.is_success() => "acknowledged",
        Some(status)
            if status.is_client_error()
                && status.as_u16() != 408
                && !has_acknowledgement(headers) =>
        {
            return None;
        }
        _ => "unknown",
    };
    let segments: Vec<_> = path
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .trim_start_matches('/')
        .split('/')
        .collect();
    // These are requested route IDs, not inferred result scope or an inventory
    // match. Never echo arbitrary names, bodies, query strings or returned URLs.
    let (workspace_id, target_id) = if matches!(segments.first(), Some(&"workspaces" | &"groups")) {
        (
            segments.get(1).and_then(|id| uuid(id)),
            segments.get(3).and_then(|id| uuid(id)),
        )
    } else {
        (None, None)
    };
    let mut operation_values = headers.get_all("x-ms-operation-id").iter();
    let operation_id = operation_values
        .next()
        .and_then(|value| value.to_str().ok())
        .and_then(uuid)
        .filter(|_| operation_values.next().is_none());
    Some(SubmissionEvidence {
        service,
        state,
        http_status: status.map(|value| value.as_u16()),
        workspace_id,
        target_id,
        operation_id,
        location_present: headers.contains_key("location"),
        server_retriable: None,
    })
}

fn uuid(value: &str) -> Option<String> {
    (value.len() == 36)
        .then(|| uuid::Uuid::parse_str(value).ok())
        .flatten()
        .filter(|id| !id.is_nil())
        .map(|id| id.to_string())
}

pub(super) fn finish<T>(result: Result<T>, evidence: Option<SubmissionEvidence>) -> Result<T> {
    result.map_err(|error| {
        let Some(mut evidence) = evidence else { return error; };
        let mut error = error.downcast_ref::<FabioError>().map_or_else(
            || FabioError::with_typed_hint(ErrorCode::ApiError,
                "Submission was acknowledged or its outcome is unknown; response/observation failed.",
                NO_REPLAY_HINT, HintType::SemanticCorrection),
            |fabio| fabio.with_replaced_hint(NO_REPLAY_HINT, Some(HintType::SemanticCorrection)),
        );
        evidence.server_retriable = error.retriable;
        error.retriable = Some(false);
        // An older verification hint might refer to retrying the submission.
        error.verify_after = None;
        error.submission = Some(Box::new(evidence));
        error.into()
    })
}

pub(super) fn network_error(
    error: &reqwest::Error,
    semantics: PostSemantics,
    service: &'static str,
    path: &str,
) -> anyhow::Error {
    if semantics == PostSemantics::ReadOnly {
        return FabioError::new(ErrorCode::NetworkError, error.to_string()).into();
    }
    let code = if error.is_timeout() {
        ErrorCode::Timeout
    } else {
        ErrorCode::NetworkError
    };
    finish::<()>(
        Err(FabioError::new(code, "Submission transport failed; acceptance is unknown.").into()),
        evidence(semantics, service, path, None, &HeaderMap::new()),
    )
    .unwrap_err()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const OP: &str = "cccccccc-1111-2222-3333-444444444444";

    #[test]
    fn rejection_retries_are_semantic_and_conflicting_markers_block_mutation_replay() {
        let mut headers = HeaderMap::new();
        assert!(can_retry_rejection(PostSemantics::Mutation, &headers));
        for marker in ["location", "x-ms-operation-id"] {
            headers.insert(marker, "malformed-private-value".parse().unwrap());
            assert!(!can_retry_rejection(PostSemantics::Mutation, &headers));
            assert!(can_retry_rejection(PostSemantics::ReadOnly, &headers));
        }
        assert!(
            evidence(
                PostSemantics::ReadOnly,
                "fabric",
                "/query",
                Some(StatusCode::SERVICE_UNAVAILABLE),
                &headers
            )
            .is_none()
        );
        // A redirect without its Location or an unrecognized status cannot
        // establish rejection: the original mutation may already have run.
        for status in [302, 600] {
            let context = evidence(
                PostSemantics::Mutation,
                "fabric",
                "/create",
                Some(StatusCode::from_u16(status).unwrap()),
                &HeaderMap::new(),
            )
            .unwrap();
            assert_eq!(context.state, "unknown");
        }
        assert!(
            evidence(
                PostSemantics::Mutation,
                "fabric",
                "/create",
                Some(StatusCode::UNAUTHORIZED),
                &HeaderMap::new()
            )
            .is_none()
        );
    }

    #[test]
    fn only_a_unique_nonzero_operation_header_is_identity_never_location_or_trace() {
        for operation in [
            None,
            Some("not-a-uuid"),
            Some("00000000-0000-0000-0000-000000000000"),
            Some(OP),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(
                "location",
                format!("https://example.org/private?secret={OP}")
                    .parse()
                    .unwrap(),
            );
            headers.insert("x-ms-request-id", OP.parse().unwrap());
            if let Some(operation) = operation {
                headers.insert("x-ms-operation-id", operation.parse().unwrap());
            }
            let scope = format!("/workspaces/{WS}/items/{ITEM}/action?credential=private");
            let value = evidence(
                PostSemantics::Mutation,
                "fabric",
                &scope,
                Some(StatusCode::ACCEPTED),
                &headers,
            )
            .unwrap();
            assert_eq!(value.state, "accepted");
            assert_eq!(value.workspace_id.as_deref(), Some(WS));
            assert_eq!(value.target_id.as_deref(), Some(ITEM));
            assert_eq!(
                value.operation_id.as_deref(),
                operation.filter(|value| *value == OP)
            );
            let text = serde_json::to_string(&value).unwrap();
            assert!(!text.contains("private"));
            assert!(!text.contains("example.org"));
            headers.append("x-ms-operation-id", OP.parse().unwrap());
            headers.append("x-ms-operation-id", OP.parse().unwrap());
            assert!(
                evidence(
                    PostSemantics::Mutation,
                    "fabric",
                    &scope,
                    Some(StatusCode::ACCEPTED),
                    &headers
                )
                .unwrap()
                .operation_id
                .is_none()
            );
        }
    }

    #[test]
    fn ambiguous_errors_preserve_metadata_and_api_retriability_without_authorizing_replay() {
        let original = FabioError::from_status(503, "unavailable")
            .set_retriable(Some(true))
            .set_request_id(Some("mock-trace".to_string()))
            .set_parameters(Some(vec![crate::errors::ErrorParameter {
                name: Some("mock".to_string()),
                value: Some("value".to_string()),
                message: None,
            }]))
            .set_verify_after("retry the mutation");
        let context = evidence(
            PostSemantics::Mutation,
            "powerbi",
            &format!("/groups/{WS}/datasets/{ITEM}/refreshes"),
            Some(StatusCode::SERVICE_UNAVAILABLE),
            &HeaderMap::new(),
        );
        let error = finish::<()>(Err(original.into()), context).unwrap_err();
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::ApiError);
        assert_eq!(error.retriable, Some(false));
        assert_eq!(error.hint_type, Some(HintType::SemanticCorrection));
        assert!(error.verify_after.is_none());
        assert_eq!(error.request_id.as_deref(), Some("mock-trace"));
        assert_eq!(error.parameters.as_ref().unwrap().len(), 1);
        assert!(!crate::errors::can_retry_submission_rejection(
            &error
                .with_replaced_hint("Inspect only", Some(HintType::SemanticCorrection))
                .into()
        ));
        assert!(crate::errors::can_retry_submission_rejection(
            &FabioError::new(ErrorCode::Conflict, "Named rejection").into()
        ));
        assert!(!crate::errors::can_retry_submission_rejection(
            &FabioError::new(ErrorCode::NetworkError, "transport").into()
        ));
        assert!(!crate::errors::can_retry_submission_rejection(
            &anyhow::anyhow!("Unknown error containing a retry marker")
        ));
        let copy = error.with_replaced_hint(
            "Inspect existing work, never resubmit.",
            Some(HintType::SemanticCorrection),
        );
        assert_eq!(
            serde_json::to_value(&copy.submission).unwrap(),
            json!({
                "service":"powerbi","state":"unknown","httpStatus":503,"workspaceId":WS,"targetId":ITEM,"locationPresent":false,"serverRetriable":true
            })
        );
    }

    #[tokio::test]
    async fn delayed_submission_response_is_unknown_and_never_automatically_retried() {
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(wiremock::matchers::method("POST"))
            .respond_with(ResponseTemplate::new(202).set_delay(std::time::Duration::from_secs(3)))
            .expect(1)
            .mount(&server)
            .await;
        let mut client = FabricClient::new();
        client.http = super::super::http_client_builder()
            .timeout(std::time::Duration::from_secs(1))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap();
        *client.fabric_token.write().await = Some(super::super::CachedToken::new(
            "fake".to_string(),
            std::time::SystemTime::now() + std::time::Duration::from_hours(1),
        ));
        let error = client
            .send_json_post(
                &server.uri(),
                "/create",
                Some(&json!({})),
                PostSemantics::Mutation,
                "fabric",
            )
            .await
            .unwrap_err();
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::Timeout);
        assert_eq!(error.retriable, Some(false));
        assert_eq!(error.submission.as_ref().unwrap().state, "unknown");
        assert!(error.submission.as_ref().unwrap().http_status.is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }
}
