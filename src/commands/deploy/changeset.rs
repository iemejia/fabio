use serde::{Deserialize, Serialize};

/// Action to take for an item during deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeAction {
    Create,
    Update,
    /// Rename + optional definition update (matched by logical ID, name differs).
    Rename,
    Delete,
    Skip,
}

impl std::fmt::Display for ChangeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Create => write!(f, "CREATE"),
            Self::Update => write!(f, "UPDATE"),
            Self::Rename => write!(f, "RENAME"),
            Self::Delete => write!(f, "DELETE"),
            Self::Skip => write!(f, "SKIP"),
        }
    }
}

/// A single change to apply during deployment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    /// Item display name (the TARGET name from source).
    pub name: String,
    /// Fabric item type (e.g., "Notebook", "`DataPipeline`").
    pub item_type: String,
    /// What to do with this item.
    pub action: ChangeAction,
    /// Human-readable reason for this action.
    pub reason: String,
    /// Logical ID from `.platform` config (stable cross-environment ID).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_id: Option<String>,
    /// Deployed item GUID (present for update/delete/skip/rename).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployed_id: Option<String>,
    /// Content hash of the local definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    /// Previous name of the item (only set for rename actions).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
}

/// The complete set of changes to deploy.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Changeset {
    /// Ordered list of changes.
    pub changes: Vec<Change>,
    /// Non-fatal warnings (e.g., items without logical IDs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// Fatal errors that prevent deployment (e.g., unresolved references).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// Summary counts for display.
#[derive(Debug, Clone, Serialize)]
pub struct ChangesetSummary {
    pub create: usize,
    pub update: usize,
    pub rename: usize,
    pub delete: usize,
    pub skip: usize,
}

impl Changeset {
    /// Definition replacement, rename/republication and deletion can destroy
    /// existing content. Creation and unchanged items do not overwrite it.
    pub fn has_destructive_actions(&self) -> bool {
        self.changes.iter().any(|change| {
            matches!(
                change.action,
                ChangeAction::Update | ChangeAction::Rename | ChangeAction::Delete
            )
        })
    }

    pub const fn new() -> Self {
        Self {
            changes: Vec::new(),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn summary(&self) -> ChangesetSummary {
        let mut s = ChangesetSummary {
            create: 0,
            update: 0,
            rename: 0,
            delete: 0,
            skip: 0,
        };
        for c in &self.changes {
            match c.action {
                ChangeAction::Create => s.create += 1,
                ChangeAction::Update => s.update += 1,
                ChangeAction::Rename => s.rename += 1,
                ChangeAction::Delete => s.delete += 1,
                ChangeAction::Skip => s.skip += 1,
            }
        }
        s
    }

    /// Returns true if there are fatal errors preventing deployment.
    #[inline]
    pub const fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns true if there are actionable changes (create/update/delete).
    #[inline]
    pub fn has_changes(&self) -> bool {
        self.changes.iter().any(|c| c.action != ChangeAction::Skip)
    }
}

/// Result of executing a deployment.
#[derive(Debug, Serialize)]
pub struct DeployResult {
    pub succeeded: Vec<Change>,
    pub failed: Vec<DeployFailure>,
    pub skipped: Vec<Change>,
    pub duration_ms: u64,
}

#[derive(Debug, Serialize)]
pub struct DeployFailure {
    pub change: Change,
    pub error: String,
    pub code: String,
    #[serde(rename = "errorDetails", skip_serializing_if = "Option::is_none")]
    pub error_details: Option<Box<serde_json::Value>>,
    /// An action can contain earlier successful substeps; a failure is never
    /// permission to replay the action or its containing batch automatically.
    #[serde(rename = "retryAllowed")]
    pub retry_allowed: bool,
}

impl DeployFailure {
    pub fn from_error(change: Change, error: &anyhow::Error) -> Self {
        let code = error
            .downcast_ref::<crate::errors::FabioError>()
            .map_or_else(
                || "UNKNOWN".to_string(),
                |error| format!("{:?}", error.code),
            );
        Self::from_error_with_code(change, error, code)
    }

    pub fn from_error_with_code(
        change: Change,
        error: &anyhow::Error,
        code: impl Into<String>,
    ) -> Self {
        let mut failure = Self::local(change, error.to_string(), code);
        failure.error_details = error
            .downcast_ref::<crate::errors::FabioError>()
            .map(|error| Box::new(crate::output::error_details(error)));
        failure
    }

    pub fn local(change: Change, error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            change,
            error: error.into(),
            code: code.into(),
            error_details: None,
            retry_allowed: false,
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn destructive_actions_include_definition_updates_and_renames_not_create_or_skip() {
        for (action, expected) in [
            (super::ChangeAction::Create, false),
            (super::ChangeAction::Skip, false),
            (super::ChangeAction::Update, true),
            (super::ChangeAction::Rename, true),
            (super::ChangeAction::Delete, true),
        ] {
            let mut changeset = super::Changeset::new();
            changeset
                .changes
                .push(make_change(action, "Fixture", "Notebook"));
            assert_eq!(changeset.has_destructive_actions(), expected);
        }
    }

    use super::*;

    fn make_change(action: ChangeAction, name: &str, item_type: &str) -> Change {
        Change {
            name: name.to_owned(),
            item_type: item_type.to_owned(),
            action,
            reason: "test".to_owned(),
            logical_id: None,
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        }
    }

    #[test]
    fn failure_preserves_legacy_fields_and_canonical_metadata_through_context() {
        use crate::errors::{
            ErrorCode, ErrorDetail, ErrorParameter, FabioError, HintType, RelatedResource,
            SubmissionEvidence,
        };
        let error = FabioError::with_typed_hint(
            ErrorCode::ApiError,
            "mock failure",
            "Inspect read-only",
            HintType::SemanticCorrection,
        )
        .set_retriable(Some(false))
        .set_request_id(Some("mock-trace".into()))
        .set_more_details(Some(vec![ErrorDetail {
            error_code: "MockDetail".into(),
            message: "detail".into(),
        }]))
        .set_related_resource(Some(RelatedResource {
            resource_id: "mock-resource".into(),
            resource_type: "Notebook".into(),
        }))
        .set_parameters(Some(vec![ErrorParameter {
            name: Some("mock".into()),
            value: Some("preserved".into()),
            message: None,
        }]))
        .set_verify_after("fabio item list --workspace <WS>");
        let mut error = error;
        error.submission = Some(Box::new(SubmissionEvidence {
            service: "fabric",
            state: "unknown",
            http_status: Some(503),
            workspace_id: None,
            target_id: None,
            operation_id: None,
            location_present: false,
            server_retriable: Some(true),
        }));
        let expected = crate::output::error_details(&error);
        let error = anyhow::Error::new(error).context("wrapped diagnostic");
        let change = make_change(ChangeAction::Create, "Mock", "Notebook");
        let failure = DeployFailure::from_error(change.clone(), &error);
        let value = serde_json::to_value(&failure).unwrap();
        assert_eq!(value["change"], serde_json::to_value(&change).unwrap());
        assert_eq!(value["error"], "wrapped diagnostic");
        assert_eq!(value["code"], "ApiError");
        assert_eq!(value["retryAllowed"], false);
        assert_eq!(value["errorDetails"], expected);
        assert_eq!(expected["code"], "API_ERROR");
        assert_eq!(expected["hintType"], "semantic_correction");
        assert_eq!(expected["hint"], "Inspect read-only");
        assert_eq!(expected["verifyAfter"], "fabio item list --workspace <WS>");
        assert_eq!(expected["requestId"], "mock-trace");
        assert_eq!(expected["moreDetails"][0]["errorCode"], "MockDetail");
        assert_eq!(expected["relatedResource"]["resourceId"], "mock-resource");
        assert_eq!(expected["submission"]["serverRetriable"], true);
        assert_eq!(expected["parameters"][0]["value"], "preserved");
        let override_failure =
            DeployFailure::from_error_with_code(change, &error, "BULK_IMPORT_FAILED");
        assert_eq!(override_failure.code, "BULK_IMPORT_FAILED");
        assert_eq!(override_failure.error_details, failure.error_details);
    }

    #[test]
    fn untyped_and_local_failures_do_not_fabricate_submission_evidence() {
        let error = anyhow::anyhow!("mock local failure");
        let change = make_change(ChangeAction::Create, "Mock", "Notebook");
        for failure in [
            DeployFailure::from_error(change.clone(), &error),
            DeployFailure::local(change, "mock local failure", "UNKNOWN"),
        ] {
            let value = serde_json::to_value(failure).unwrap();
            assert_eq!(value["code"], "UNKNOWN");
            assert_eq!(value["error"], "mock local failure");
            assert_eq!(value["retryAllowed"], false);
            assert!(value.get("errorDetails").is_none());
        }
    }

    #[test]
    fn failure_keeps_legacy_debug_code_and_does_not_promote_nested_retry_permission() {
        let error = crate::errors::FabioError::not_found("mock")
            .set_retriable(Some(true))
            .into();
        let failure = DeployFailure::from_error(
            make_change(ChangeAction::Update, "Mock", "Notebook"),
            &error,
        );
        let value = serde_json::to_value(failure).unwrap();
        assert_eq!(value["code"], "NotFound");
        assert_eq!(value["errorDetails"]["code"], "NOT_FOUND");
        assert_eq!(value["errorDetails"]["retriable"], true);
        assert_eq!(value["retryAllowed"], false);
    }

    #[test]
    fn test_changeset_new_is_empty() {
        let cs = Changeset::new();
        assert!(cs.changes.is_empty());
        assert_eq!(cs.warnings, [] as [std::string::String; 0]);
        assert_eq!(cs.errors, [] as [std::string::String; 0]);
    }

    #[test]
    fn test_changeset_summary_counts() {
        let mut cs = Changeset::new();
        cs.changes
            .push(make_change(ChangeAction::Create, "A", "Notebook"));
        cs.changes
            .push(make_change(ChangeAction::Create, "B", "Notebook"));
        cs.changes
            .push(make_change(ChangeAction::Update, "C", "Lakehouse"));
        cs.changes
            .push(make_change(ChangeAction::Delete, "D", "Report"));
        cs.changes
            .push(make_change(ChangeAction::Skip, "E", "Notebook"));

        let s = cs.summary();
        assert_eq!(s.create, 2);
        assert_eq!(s.update, 1);
        assert_eq!(s.delete, 1);
        assert_eq!(s.skip, 1);
    }

    #[test]
    fn test_changeset_has_errors() {
        let mut cs = Changeset::new();
        assert!(!cs.has_errors());

        cs.errors.push("unresolved ref".to_owned());
        assert!(cs.has_errors());
    }

    #[test]
    fn test_changeset_has_changes() {
        let mut cs = Changeset::new();
        assert!(!cs.has_changes()); // empty

        cs.changes
            .push(make_change(ChangeAction::Skip, "A", "Notebook"));
        assert!(!cs.has_changes()); // only skips

        cs.changes
            .push(make_change(ChangeAction::Create, "B", "Notebook"));
        assert!(cs.has_changes()); // has a create
    }

    #[test]
    fn test_change_action_display() {
        assert_eq!(ChangeAction::Create.to_string(), "CREATE");
        assert_eq!(ChangeAction::Update.to_string(), "UPDATE");
        assert_eq!(ChangeAction::Delete.to_string(), "DELETE");
        assert_eq!(ChangeAction::Skip.to_string(), "SKIP");
    }

    #[test]
    fn test_changeset_serialization_roundtrip() {
        let mut cs = Changeset::new();
        cs.changes.push(Change {
            name: "MyNotebook".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new item".to_owned(),
            logical_id: Some("lid-abc".to_owned()),
            deployed_id: None,
            source_hash: Some("sha256:abc123".to_owned()),
            previous_name: None,
        });
        cs.warnings.push("no logical id".to_owned());

        let json = serde_json::to_string(&cs).unwrap();
        let deserialized: Changeset = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.changes.len(), 1);
        assert_eq!(deserialized.changes[0].name, "MyNotebook");
        assert_eq!(deserialized.changes[0].action, ChangeAction::Create);
        assert_eq!(deserialized.warnings.len(), 1);
    }

    #[test]
    fn test_change_action_serde_snake_case() {
        let json = serde_json::to_string(&ChangeAction::Create).unwrap();
        assert_eq!(json, "\"create\"");

        let json = serde_json::to_string(&ChangeAction::Delete).unwrap();
        assert_eq!(json, "\"delete\"");

        let deserialized: ChangeAction = serde_json::from_str("\"update\"").unwrap();
        assert_eq!(deserialized, ChangeAction::Update);
    }

    #[test]
    fn test_optional_fields_skipped_in_serialization() {
        let change = Change {
            name: "Test".to_owned(),
            item_type: "Notebook".to_owned(),
            action: ChangeAction::Create,
            reason: "new".to_owned(),
            logical_id: None,
            deployed_id: None,
            source_hash: None,
            previous_name: None,
        };
        let json = serde_json::to_string(&change).unwrap();
        assert!(!json.contains("logical_id"));
        assert!(!json.contains("deployed_id"));
        assert!(!json.contains("source_hash"));
        assert!(!json.contains("previous_name"));
    }
}
