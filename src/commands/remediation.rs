//! Pure, evidence-based recovery guidance. Builders never query a tenant or
//! execute their commands. Concrete policy changes are always guarded previews.

use anyhow::Error;
use serde::Serialize;
use std::fmt::Write as _;
use uuid::Uuid;

use crate::cli::{Cli, Command};
use crate::errors::{ErrorCode, FabioError, HintType};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Inspection,
    Review,
    Remediation,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryHint {
    pub kind: Kind,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub hint_type: HintType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verify_after: Option<String>,
    #[serde(skip_serializing_if = "is_false")]
    pub requires_approval: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command_template: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub missing_inputs: Vec<&'static str>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip callback requires a reference.
const fn is_false(value: &bool) -> bool {
    !*value
}

impl RecoveryHint {
    pub fn inspection(message: impl Into<String>, command: String) -> Self {
        Self {
            kind: Kind::Inspection,
            message: message.into(),
            command: Some(command),
            hint_type: HintType::RetrySafe,
            verify_after: None,
            requires_approval: false,
            destructive: None,
            command_template: None,
            missing_inputs: Vec::new(),
        }
    }

    pub fn review(message: impl Into<String>) -> Self {
        Self {
            kind: Kind::Review,
            command: None,
            hint_type: HintType::SemanticCorrection,
            ..Self::inspection(message, String::new())
        }
    }

    /// Text for the existing `FabioError.hint` contract; templates remain clearly
    /// distinct from runnable commands and never invent missing resource details.
    pub fn text(&self) -> String {
        let mut text = self.message.clone();
        if let Some(command) = &self.command {
            let _ = write!(text, " Command: `{command}`.");
        }
        if let Some(template) = &self.command_template {
            let _ = write!(
                text,
                " Template only (missing: {}): `{template}`.",
                self.missing_inputs.join(", ")
            );
        }
        if self.requires_approval {
            text.push_str(" Preview only: do not remove --dry-run or --readonly and execute the change without explicit user approval.");
        }
        text
    }

    /// Preserve API codes, retriability, correlation IDs, and structured details.
    /// A supplied earlier verification command remains authoritative.
    pub fn apply(&self, mut error: FabioError) -> FabioError {
        if protected_hint(&error) {
            return error;
        }
        error.hint = Some(self.text());
        error.hint_type = Some(HintType::SemanticCorrection);
        if error.verify_after.is_none() {
            error.verify_after.clone_from(&self.verify_after);
        }
        error
    }
}

pub fn diagnose(workspace: Uuid) -> String {
    format!("fabio --wrap-untrusted --readonly workspace diagnostics --id {workspace}")
}

pub fn connection_retirement(connection: Uuid) -> RecoveryHint {
    RecoveryHint {
        kind: Kind::Remediation,
        message: "Historical recency warrants review only, not proof that a connection is unused. Confirm ALL dependencies, owner consent and blast radius before considering retirement. This command is preview-only.".into(),
        command: Some(format!("fabio --wrap-untrusted --readonly admin delete-connection --id {connection} --dry-run")),
        hint_type: HintType::SemanticCorrection,
        verify_after: Some(format!("fabio --wrap-untrusted --readonly admin show-connection --id {connection}")),
        requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("admin delete-connection")),
        command_template: None,
        missing_inputs: Vec::new(),
    }
}

pub fn connection_owner_continuity(connection: Uuid) -> RecoveryHint {
    RecoveryHint {
        kind: Kind::Remediation,
        message: "Review an explicitly selected supported owner. Admin grants support User/ServicePrincipal, not Group; this service-principal example leaves its identity unset. Owner may remain PendingOwner until credential handoff; no ownership or credentials are certified.".into(),
        command: None,
        hint_type: HintType::SemanticCorrection,
        verify_after: Some(format!("fabio --wrap-untrusted --readonly admin list-connection-role-assignments --id {connection}")),
        requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("admin add-connection-role-assignment")),
        command_template: Some(format!("fabio --wrap-untrusted --readonly admin add-connection-role-assignment --id {connection} --principal-id \"$DURABLE_OWNER_SP_ID\" --principal-type ServicePrincipal --dry-run")),
        missing_inputs: vec!["durableOwnerServicePrincipalId"],
    }
}

pub fn remove_block(workspace: Uuid, capacity: Uuid, actor: Uuid) -> RecoveryHint {
    RecoveryHint {
        kind: Kind::Remediation,
        message: "An unexpired block record was observed without a conflicting mission-critical record in complete scans. Removing it changes policy; the record alone does not prove it caused a particular workload failure. Recheck the workspace/capacity scope before approval.".to_string(),
        command: Some(format!("fabio --wrap-untrusted --readonly capacity delete-blocked-actor --id {capacity} --actor-id {actor} --dry-run")),
        hint_type: HintType::SemanticCorrection,
        verify_after: Some(format!("{} --include-surge-protection", diagnose(workspace))),
        requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("capacity delete-blocked-actor")),
        command_template: None, missing_inputs: Vec::new(),
    }
}

pub fn assign_capacity(workspace: Uuid) -> RecoveryHint {
    RecoveryHint {
        kind: Kind::Remediation,
        message: "No capacity assignment was observed. Choose a target appropriate to the workload and review permissions/costs before previewing assignment. No target capacity was guessed.".to_string(),
        command: None, hint_type: HintType::SemanticCorrection,
        verify_after: Some(diagnose(workspace)), requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("workspace assign-capacity")),
        command_template: Some(format!("fabio --wrap-untrusted --readonly workspace assign-capacity --id {workspace} --capacity \"$TARGET_CAPACITY_ID\" --dry-run")),
        missing_inputs: vec!["targetCapacityId"],
    }
}

pub fn inactive_capacity(capacity: Uuid, sku: Option<&str>) -> RecoveryHint {
    if !sku.is_some_and(|sku| {
        sku.strip_prefix('F').is_some_and(|digits| {
            digits
                .parse::<u32>()
                .is_ok_and(|units| units >= 2 && units.is_power_of_two())
        })
    }) {
        return RecoveryHint::review(
            "Capacity metadata reports Inactive, but a resumable Fabric F-series resource was not established. Ask its administrator to review availability; no ARM resource or resume operation was inferred.",
        );
    }
    RecoveryHint {
        kind: Kind::Remediation,
        message: "An inactive F-series capacity was observed. Confirm it is a resumable ARM resource and supply its Azure identifiers before previewing resume. Resume can restart capacity consumption; no ARM lookup or resume was performed.".to_string(),
        command: None, hint_type: HintType::SemanticCorrection,
        verify_after: Some(format!("fabio --wrap-untrusted --readonly capacity show --id {capacity} --query state")),
        requires_approval: true,
        destructive: Some(crate::agent::is_destructive_operation("capacity resume")),
        command_template: Some("fabio --wrap-untrusted --readonly capacity resume --subscription \"$SUBSCRIPTION_ID\" --resource-group \"$RESOURCE_GROUP\" --name \"$CAPACITY_NAME\" --dry-run".to_string()),
        missing_inputs: vec!["subscription", "resourceGroup", "capacityName"],
    }
}

fn workspace_for_command(cli: &Cli) -> Option<Uuid> {
    use super::copy_job::CopyJobCommand;
    use super::data_pipeline::DataPipelineCommand;
    use super::dataflow::DataflowCommand;
    use super::job_scheduler::JobSchedulerCommand;
    use super::notebook::NotebookCommand;
    use super::semantic_model::SemanticModelCommand;
    use super::spark_job_definition::SparkJobDefinitionCommand;
    let (Command::CopyJob {
        command: CopyJobCommand::Run { workspace, .. },
    }
    | Command::DataPipeline {
        command: DataPipelineCommand::Run { workspace, .. },
    }
    | Command::Dataflow {
        command: DataflowCommand::Run { workspace, .. },
    }
    | Command::Notebook {
        command: NotebookCommand::Run { workspace, .. },
    }
    | Command::SparkJobDefinition {
        command: SparkJobDefinitionCommand::Run { workspace, .. },
    }
    | Command::JobScheduler {
        command: JobSchedulerCommand::RunOnDemand { workspace, .. },
    }
    | Command::SemanticModel {
        command:
            SemanticModelCommand::Refresh { workspace, .. }
            | SemanticModelCommand::Query { workspace, .. },
    }) = &cli.command
    else {
        return None;
    };
    Uuid::parse_str(workspace).ok().filter(|id| !id.is_nil())
}

fn capacity_signal(error: &FabioError) -> bool {
    if error.code == ErrorCode::CapacityInactive {
        return true;
    }
    if !matches!(
        error.code,
        ErrorCode::ApiError | ErrorCode::RateLimited | ErrorCode::Forbidden
    ) {
        return false;
    }
    let message = &error.message[..error.message.floor_char_boundary(2_048)];
    let mut message = message.to_ascii_lowercase();
    for detail in error.more_details.iter().flatten().take(16) {
        message.push_str(
            &detail.error_code[..detail.error_code.floor_char_boundary(256)].to_ascii_lowercase(),
        );
    }
    [
        "capacitynotactive",
        "toomanyrequestsforcapacity",
        "rejectedsurgeprotection",
        "workspaceblocked",
        "capacity is inactive",
        "capacity is paused",
        "workspace is blocked",
    ]
    .iter()
    .any(|marker| message.contains(marker))
}

fn protected_hint(error: &FabioError) -> bool {
    error.hint_type == Some(HintType::SafetyBypass)
        || error
            .hint
            .as_deref()
            .is_some_and(crate::agent::hint_suggests_dangerous_flag)
}

/// Used once by the command dispatcher. No additional network probes, grants,
/// actor-ID parsing from free text, or mutation previews for ambiguous failures.
pub fn enrich_command_error(cli: &Cli, error: Error) -> Error {
    let Some(fabio) = error.downcast_ref::<FabioError>() else {
        return error;
    };
    if protected_hint(fabio)
        || super::tenant_gate::is_feature_disabled(&error)
        || !capacity_signal(fabio)
    {
        return error;
    }
    let Some(workspace) = workspace_for_command(cli) else {
        return error;
    };
    let hint = RecoveryHint::inspection(
        "The failure may involve capacity availability or workspace policy. Inspect scoped configuration before retrying; this error does not identify a removable block. Surge-policy reads are separately opt-in with --include-surge-protection and need capacity-admin access. Do not change policies based on this error alone.",
        diagnose(workspace),
    );
    match error.downcast::<FabioError>() {
        Ok(error) => hint.apply(error).into(),
        Err(error) => error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::errors::{ErrorDetail, ErrorParameter, RelatedResource};
    use clap::{CommandFactory as _, Parser as _};

    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const CAP: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    const ACTOR: &str = "cccccccc-1111-2222-3333-444444444444";

    fn with_cli(test: impl FnOnce(clap::Command) + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(move || test(Cli::command()))
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn concrete_previews_and_templates_use_real_cli_syntax_without_bypass_flags() {
        with_cli(|cli| {
            let hints = [
                remove_block(
                    WS.parse().unwrap(),
                    CAP.parse().unwrap(),
                    ACTOR.parse().unwrap(),
                ),
                assign_capacity(WS.parse().unwrap()),
                inactive_capacity(CAP.parse().unwrap(), Some("F2")),
                connection_retirement(WS.parse().unwrap()),
                connection_owner_continuity(WS.parse().unwrap()),
            ];
            for hint in hints {
                assert_eq!(hint.kind, Kind::Remediation);
                assert!(hint.requires_approval);
                assert_eq!(
                    serde_json::to_value(hint.hint_type).unwrap(),
                    "semantic_correction"
                );
                let text = hint
                    .command
                    .as_ref()
                    .or(hint.command_template.as_ref())
                    .unwrap();
                let args = shlex::split(text).unwrap();
                cli.clone().try_get_matches_from(args).unwrap();
                assert!(text.contains("--readonly"));
                assert!(text.contains("--dry-run"));
                assert!(!crate::agent::hint_suggests_dangerous_flag(text));
                let verify = hint.verify_after.as_ref().unwrap();
                cli.clone()
                    .try_get_matches_from(shlex::split(verify).unwrap())
                    .unwrap();
                assert!(verify.contains("--readonly"));
                assert!(hint.text().contains("explicit user approval"));
            }
        });
    }

    #[test]
    fn missing_inputs_and_unknown_capacity_types_do_not_create_runnable_commands() {
        let owner = connection_owner_continuity(WS.parse().unwrap());
        assert!(owner.command.is_none());
        assert_eq!(owner.missing_inputs, vec!["durableOwnerServicePrincipalId"]);
        assert_eq!(owner.destructive, Some(false));
        assert_eq!(
            connection_retirement(WS.parse().unwrap()).destructive,
            Some(true)
        );
        let assignment = assign_capacity(WS.parse().unwrap());
        assert!(assignment.command.is_none());
        assert_eq!(assignment.missing_inputs, vec!["targetCapacityId"]);
        let resume = inactive_capacity(CAP.parse().unwrap(), Some("F2"));
        assert!(resume.command.is_none());
        assert_eq!(
            resume.missing_inputs,
            vec!["subscription", "resourceGroup", "capacityName"]
        );
        for sku in [
            None,
            Some("Trial"),
            Some("P1"),
            Some("FutureSku"),
            Some("F0"),
            Some("F3"),
        ] {
            let hint = inactive_capacity(CAP.parse().unwrap(), sku);
            assert_eq!(hint.kind, Kind::Review);
            assert!(hint.command.is_none());
            assert!(hint.command_template.is_none());
        }
        assert_eq!(
            remove_block(
                WS.parse().unwrap(),
                CAP.parse().unwrap(),
                ACTOR.parse().unwrap()
            )
            .destructive,
            Some(true)
        );
    }

    #[test]
    fn hint_application_preserves_api_metadata_and_existing_verification() {
        let error = FabioError::with_hint(
            ErrorCode::RateLimited,
            "TooManyRequestsForCapacity",
            "old hint",
        )
        .set_retriable(Some(true))
        .set_request_id(Some("fixture-request".to_string()))
        .set_more_details(Some(vec![ErrorDetail {
            error_code: "CapacityDetail".to_string(),
            message: "detail".to_string(),
        }]))
        .set_related_resource(Some(RelatedResource {
            resource_id: CAP.to_string(),
            resource_type: "Capacity".to_string(),
        }))
        .set_parameters(Some(vec![ErrorParameter {
            name: Some("workspaceId".to_string()),
            value: Some(WS.to_string()),
            message: None,
        }]))
        .set_verify_after("earlier read-only verification".to_string());
        let updated = remove_block(
            WS.parse().unwrap(),
            CAP.parse().unwrap(),
            ACTOR.parse().unwrap(),
        )
        .apply(error);
        assert_eq!(updated.code, ErrorCode::RateLimited);
        assert_eq!(updated.retriable, Some(true));
        assert_eq!(updated.request_id.as_deref(), Some("fixture-request"));
        assert_eq!(
            updated.more_details.as_ref().unwrap()[0].error_code,
            "CapacityDetail"
        );
        assert_eq!(updated.related_resource.as_ref().unwrap().resource_id, CAP);
        assert_eq!(
            updated.parameters.as_ref().unwrap()[0].value.as_deref(),
            Some(WS)
        );
        assert_eq!(
            updated.verify_after.as_deref(),
            Some("earlier read-only verification")
        );
        assert_eq!(
            serde_json::to_value(updated.hint_type).unwrap(),
            "semantic_correction"
        );
    }

    #[test]
    fn safety_bypass_hints_cannot_be_overwritten_by_remediation_or_inspection() {
        for typed in [None, Some(HintType::SafetyBypass)] {
            let mut error = FabioError::with_hint(
                ErrorCode::ApiError,
                "CapacityNotActive",
                "Use --force only after approval",
            );
            error.hint_type = typed;
            let updated = remove_block(
                WS.parse().unwrap(),
                CAP.parse().unwrap(),
                ACTOR.parse().unwrap(),
            )
            .apply(error);
            assert_eq!(
                updated.hint.as_deref(),
                Some("Use --force only after approval")
            );
            assert_eq!(
                serde_json::to_value(updated.hint_type).unwrap(),
                serde_json::to_value(typed).unwrap()
            );
            assert!(updated.verify_after.is_none());
        }
    }

    #[test]
    fn ordinary_throttling_permissions_and_unrecognized_failures_do_not_imply_blocks() {
        for (code, message) in [
            (ErrorCode::RateLimited, "Too many requests"),
            (ErrorCode::Forbidden, "Insufficient permissions"),
            (ErrorCode::ApiError, "generic backend failure"),
            (
                ErrorCode::InvalidInput,
                "a user expression contains CapacityNotActive",
            ),
        ] {
            assert!(!capacity_signal(&FabioError::new(code, message)));
        }
        for message in [
            "TooManyRequestsForCapacity",
            "RejectedSurgeProtection",
            "capacity is inactive",
        ] {
            assert!(capacity_signal(&FabioError::new(
                ErrorCode::ApiError,
                message
            )));
        }
    }

    #[test]
    fn operational_commands_reuse_diagnostic_hints_but_not_mutation_previews() {
        with_cli(|_| {
            for group in [
                "notebook",
                "data-pipeline",
                "copy-job",
                "spark-job-definition",
                "dataflow",
                "semantic-model",
                "job-scheduler",
            ] {
                let verb = match group {
                    "semantic-model" => "refresh",
                    "job-scheduler" => "run-on-demand",
                    _ => "run",
                };
                let mut args = vec!["fabio", group, verb, "--workspace", WS, "--id", ACTOR];
                if group == "job-scheduler" {
                    args.extend(["--job-type", "Pipeline"]);
                }
                let cli = Cli::try_parse_from(args).unwrap();
                let error = enrich_command_error(
                    &cli,
                    FabioError::new(ErrorCode::CapacityInactive, "CapacityNotActive").into(),
                );
                let error = error.downcast_ref::<FabioError>().unwrap();
                assert!(
                    error
                        .hint
                        .as_deref()
                        .unwrap()
                        .contains(&diagnose(WS.parse().unwrap()))
                );
                assert!(
                    !error
                        .hint
                        .as_deref()
                        .unwrap()
                        .contains("delete-blocked-actor")
                );
                assert_eq!(
                    serde_json::to_value(error.hint_type).unwrap(),
                    "semantic_correction"
                );
            }
        });
    }

    #[test]
    fn unsupported_scopes_and_protected_feature_hints_are_not_reinterpreted() {
        with_cli(|_| {
            for args in [
                vec!["fabio", "item", "list", "--workspace", WS],
                vec![
                    "fabio",
                    "data-pipeline",
                    "run",
                    "--workspace",
                    "workspace-name",
                    "--id",
                    ACTOR,
                ],
            ] {
                let cli = Cli::try_parse_from(args).unwrap();
                let error = enrich_command_error(
                    &cli,
                    FabioError::with_hint(
                        ErrorCode::CapacityInactive,
                        "CapacityNotActive",
                        "original",
                    )
                    .into(),
                );
                assert_eq!(
                    error.downcast_ref::<FabioError>().unwrap().hint.as_deref(),
                    Some("original")
                );
            }
            let cli = Cli::try_parse_from([
                "fabio",
                "data-pipeline",
                "run",
                "--workspace",
                WS,
                "--id",
                ACTOR,
            ])
            .unwrap();
            for (message, hint) in [
                ("CapacityNotActive", "Use --force only after approval"),
                (
                    "CapacityNotActive FeatureNotAvailable",
                    "tenant feature hint",
                ),
            ] {
                let error = enrich_command_error(
                    &cli,
                    FabioError::with_hint(ErrorCode::ApiError, message, hint).into(),
                );
                assert_eq!(
                    error.downcast_ref::<FabioError>().unwrap().hint.as_deref(),
                    Some(hint)
                );
            }
        });
    }
}
