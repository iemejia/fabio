//! Narrow teaching errors for cloud-connection allowlists, never policy mutation.
use crate::errors::{FabioError, HintType};

fn policy_kind(text: &str) -> Option<&'static str> {
    let text = text.to_ascii_lowercase();
    if text.contains("dmts_blockedbyauthenticationkindpolicies") {
        Some("Authentication allowlist")
    } else if text.contains("blocked due to inter-tenant data loss prevention policies") {
        Some("Tenant allowlist")
    } else {
        None
    }
}

pub(super) fn enrich(error: anyhow::Error) -> anyhow::Error {
    let Some(fabio) = error.downcast_ref::<FabioError>() else {
        return error;
    };
    // Include structured nested service errors without discarding request/submission evidence.
    let Some(kind) = policy_kind(&crate::output::error_details(fabio).to_string()) else {
        return error;
    };
    fabio.with_replaced_hint(format!(
        "Cloud connection policy blocked this operation. Ask a Fabric tenant administrator to review Admin portal > Data Policies > Data connection policies > {kind} for the intended authentication method and target Microsoft Entra tenant. Workspace roles, connection ownership and refreshing credentials do not override tenant policy. This is separate from workspace outbound connection rules and PolicySet activation. Policies affect existing and new cloud connections and approved changes may take up to one hour to propagate. Do not retry automatically, switch identities/credential kinds/tenants, weaken an allowlist or grant roles. No policy was read or changed by this diagnostic; no supported management endpoint is assumed. See: fabio context best-practices connection-policies."
    ), Some(HintType::SemanticCorrection)).set_retriable(Some(false)).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{ErrorCode, ErrorDetail};

    #[test]
    fn only_documented_policy_signals_are_classified() {
        assert_eq!(
            policy_kind("DMTS_BlockedByAuthenticationKindPolicies"),
            Some("Authentication allowlist")
        );
        assert_eq!(
            policy_kind(
                "This operation was blocked due to inter-tenant data loss prevention policies."
            ),
            Some("Tenant allowlist")
        );
        for generic in [
            "Forbidden",
            "Policy Evaluation Failure",
            "invalid credentials",
            "TenantSwitchDisabled",
            "tenant not found",
            "outbound policy blocked",
            "authentication kind not supported",
        ] {
            assert_eq!(policy_kind(generic), None);
        }
    }

    #[test]
    fn nested_policy_signals_preserve_status_provenance_and_request_metadata() {
        let error = FabioError::new(ErrorCode::ApiError, "Connection operation failed")
            .set_request_id(Some("synthetic-request".to_string()))
            .set_more_details(Some(vec![ErrorDetail {
                error_code: "DMTS_BlockedByAuthenticationKindPolicies".to_string(),
                message: "synthetic denial".to_string(),
            }]));
        let error = enrich(error.into());
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::ApiError);
        assert_eq!(error.request_id.as_deref(), Some("synthetic-request"));
        assert_eq!(error.more_details.as_ref().unwrap().len(), 1);
        assert_eq!(error.retriable, Some(false));
        assert_eq!(error.hint_type, Some(HintType::SemanticCorrection));
        assert!(
            error
                .hint
                .as_deref()
                .unwrap()
                .contains("Authentication allowlist")
        );
    }

    #[test]
    fn unrelated_forbidden_errors_keep_their_existing_hint() {
        let error = enrich(
            FabioError::with_hint(
                crate::errors::ErrorCode::Forbidden,
                "Forbidden",
                "existing role guidance",
            )
            .into(),
        );
        assert_eq!(
            error.downcast_ref::<FabioError>().unwrap().hint.as_deref(),
            Some("existing role guidance")
        );
    }
}
