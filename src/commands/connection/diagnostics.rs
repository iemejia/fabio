//! Connection visibility is not credential health, and caller roles are not the
//! workspace identity's grants. Only explicit IDs / scoped item references are read.

use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::diagnostics::{
    Check, PageCursor, ReadBudget as Budget, Status, encode_id, error_evidence, id, summary,
    uuid_target as target,
};
use crate::commands::remediation::RecoveryHint;
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

fn connection_show(connection: Uuid) -> String {
    format!("fabio --wrap-untrusted --readonly connection show --id {connection}")
}

fn references_show(workspace: Uuid, item: Uuid) -> String {
    format!(
        "fabio --wrap-untrusted --readonly item list-connections --workspace {workspace} --id {item}"
    )
}

fn review(check: &mut Check, message: &'static str) {
    check.next_steps.push(RecoveryHint::review(message));
}

fn role_check(body: &Value, connection: Uuid) -> Check {
    let role = body["myRole"].as_str();
    let (status, message) = match role {
        Some("Owner" | "User" | "UserWithReshare") => (
            Status::Ready,
            "The API reports a usable connection role for the caller. This is not a workspace identity grant or a source credential test.",
        ),
        Some("PendingOwner") => (
            Status::NeedsAction,
            "Ownership is pending, not effective. The assignee must review and update credentials before ownership becomes effective.",
        ),
        _ => (
            Status::Unknown,
            "The caller's effective role is absent or unrecognized. Metadata visibility does not establish permission to use the connection.",
        ),
    };
    let mut check = Check::new(
        "effective_role",
        status,
        message,
        json!({"source":"connection.myRole", "myRole": role.filter(|role| matches!(*role, "Owner"|"User"|"UserWithReshare"|"PendingOwner")),
            "roleReported": !body["myRole"].is_null()}),
        connection_show(connection),
    );
    if status != Status::Ready {
        review(
            &mut check,
            "Ask the intended owner/assignee to review connection access and credential ownership. No ownership transfer, credential update, role grant, or automatic credential test is authorized by this report.",
        );
    }
    check
}

fn identity_check(workspace: Option<Uuid>, metadata: Option<&Result<Value>>) -> Check {
    let next = workspace.map_or_else(
        || "fabio --wrap-untrusted context describe connection diagnostics".to_string(),
        |workspace| format!("fabio --wrap-untrusted --readonly workspace show --id {workspace}"),
    );
    let Some(workspace) = workspace else {
        return Check::new(
            "workspace_identity",
            Status::Unknown,
            "WorkspaceIdentity credentials need an explicit workspace context. No workspace was guessed.",
            json!({"reason":"workspace_context_missing", "missingInputs":["workspace"]}),
            next,
        );
    };
    let Some(Ok(body)) = metadata else {
        return metadata
            .and_then(|result| result.as_ref().err())
            .map_or_else(
                || {
                    Check::new(
                        "workspace_identity",
                        Status::Unknown,
                        "Workspace metadata was not read.",
                        json!({"reason":"not_read"}),
                        next.clone(),
                    )
                },
                |error| Check::error("workspace_identity", error, "workspace", next.clone()),
            );
    };
    if id(&body["id"]) != Some(workspace) {
        return Check::new(
            "workspace_identity",
            Status::Unknown,
            "Workspace metadata did not identify the requested workspace.",
            json!({"reason":"scope_mismatch"}),
            next,
        );
    }
    let identity = &body["workspaceIdentity"];
    let principal = id(&identity["servicePrincipalId"]);
    let application = id(&identity["applicationId"]);
    let reference = &identity["identityReference"];
    let reference_valid = reference.is_null()
        || (reference["referenceType"] == "ById"
            && id(&reference["workspaceId"]) == Some(workspace)
            && id(&reference["itemId"]).is_some());
    let (status, message) = if principal.is_some() && application.is_some() && reference_valid {
        (
            Status::Ready,
            "Workspace identity identifiers were observed. Source authorization, token acquisition, and workload use were not tested.",
        )
    } else if body.get("workspaceIdentity").is_some_and(Value::is_null) {
        (
            Status::NeedsAction,
            "Workspace metadata explicitly reports no workspace identity. Review the intended authentication design before provisioning one.",
        )
    } else {
        (
            Status::Unknown,
            "Workspace identity metadata is absent, malformed, or has an unsupported/mismatched reference. Provisioning and authorization are not inferred.",
        )
    };
    let mut check = Check::new(
        "workspace_identity",
        status,
        message,
        json!({"source":"workspace.workspaceIdentity", "applicationId": encode_id(application), "servicePrincipalId": encode_id(principal),
            "authorization":"not_checked", "identityReferenceValidated": if reference.is_null() { None } else { Some(reference_valid) }}),
        next,
    );
    review(
        &mut check,
        "The caller's connection role does not establish the workspace identity's source grants, even if the caller uses that identity. Review source permissions with an authorized administrator; no access grant, identity provisioning, credential write, or source probe was performed.",
    );
    check
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionReport {
    connection_id: Option<String>,
    credential_health: &'static str,
    checks: Vec<Check>,
}

async fn inspect(
    client: &FabricClient,
    budget: &Budget,
    connection: Uuid,
    workspace: Option<Uuid>,
    metadata: Option<&Result<Value>>,
) -> ConnectionReport {
    let next = connection_show(connection);
    let mut checks = Vec::new();
    match budget
        .get(client, &format!("/connections/{connection}"))
        .await
    {
        Err(error) => {
            let mut check = Check::error("connection_access", &error, "connection", next);
            review(
                &mut check,
                "Forbidden/not-found metadata is not proof of deletion. Review the reference and the intended caller's connection/gateway permissions; no tenant-admin fallback or role scan was attempted.",
            );
            checks.push(check);
        }
        Ok(body) if id(&body["id"]) != Some(connection) => checks.push(Check::new(
            "connection_access",
            Status::Unknown,
            "Connection metadata did not identify the requested connection.",
            json!({"reason":"scope_mismatch"}),
            next,
        )),
        Ok(body) => {
            checks.push(Check::new("connection_access", Status::Ready,
                "Metadata for the requested connection is visible. This does not establish credential health.",
                json!({"source":"connection", "connectionId":connection.to_string()}), next));
            checks.push(role_check(&body, connection));
            let credential = body["credentialDetails"]["credentialType"].as_str();
            let recognized = credential.is_some_and(|kind| {
                matches!(
                    kind,
                    "Windows"
                        | "Anonymous"
                        | "Basic"
                        | "Key"
                        | "OAuth2"
                        | "WindowsWithoutImpersonation"
                        | "SharedAccessSignature"
                        | "ServicePrincipal"
                        | "WorkspaceIdentity"
                        | "KeyPair"
                )
            });
            checks.push(Check::new("credential_configuration", if recognized { Status::Ready } else { Status::Unknown },
                "Only credential type metadata is inspected. skipTestConnection and recency are not current credential-health signals.",
                json!({"source":"connection.credentialDetails", "credentialType":credential.filter(|_| recognized),
                    "credentialHealth":"not_checked"}), connection_show(connection)));
            if credential == Some("WorkspaceIdentity") {
                checks.push(identity_check(workspace, metadata));
            }
        }
    }
    ConnectionReport {
        connection_id: Some(connection.to_string()),
        credential_health: "not_checked",
        checks,
    }
}

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
struct References {
    pages_scanned: u8,
    bindings_scanned: usize,
    complete: bool,
    reason: Option<&'static str>,
    error: Option<Value>,
    #[serde(skip)]
    rows: Vec<Value>,
    #[serde(skip)]
    cursor: PageCursor,
}

impl References {
    fn page(
        &mut self,
        body: &Value,
        limit: u8,
        seen: &mut HashMap<Uuid, String>,
    ) -> Option<String> {
        self.pages_scanned += 1;
        let Some(rows) = body["value"].as_array() else {
            self.reason = Some("malformed_page");
            return None;
        };
        for row in rows {
            if self.bindings_scanned == usize::from(limit) {
                self.reason = Some("connection_limit");
                return None;
            }
            self.bindings_scanned += 1;
            let kind = row["connectivityType"].as_str();
            if matches!(kind, Some("Automatic" | "None")) {
                self.rows.push(json!({"connectivityType":kind}));
            } else if matches!(
                kind,
                Some(
                    "ShareableCloud"
                        | "PersonalCloud"
                        | "OnPremisesGateway"
                        | "OnPremisesGatewayPersonal"
                        | "VirtualNetworkGateway"
                        | "StreamingVirtualNetworkGateway"
                )
            ) && let Some(connection) = id(&row["id"])
            {
                let kind = kind.expect("matched known connectivity type");
                if let Some(previous) = seen.get(&connection) {
                    if previous != kind {
                        self.reason = Some("conflicting_binding");
                        return None;
                    }
                } else {
                    seen.insert(connection, kind.to_string());
                    self.rows.push(json!({"id":connection.to_string()}));
                }
            } else {
                self.reason = Some("malformed_binding");
                return None;
            }
        }
        match self.cursor.next(body) {
            Ok(token) => {
                self.complete = token.is_none();
                token
            }
            Err(reason) => {
                self.reason = Some(reason);
                None
            }
        }
    }
}

async fn references(
    client: &FabricClient,
    budget: &Budget,
    workspace: Uuid,
    item: Uuid,
    limit: u8,
    pages: u8,
) -> References {
    let mut result = References::default();
    let mut token: Option<String> = None;
    let mut seen_ids = HashMap::new();
    for _ in 0..pages {
        let path = format!("/workspaces/{workspace}/items/{item}/connections");
        let path = token.as_ref().map_or_else(
            || path.clone(),
            |token| format!("{path}?continuationToken={}", urlencoding::encode(token)),
        );
        match budget.get(client, &path).await {
            Ok(body) => {
                token = result.page(&body, limit, &mut seen_ids);
                let Some(_) = &token else {
                    return result;
                };
            }
            Err(error) => {
                result.reason = Some("read_failed");
                result.error = Some(error_evidence(&error));
                return result;
            }
        }
    }
    result.reason = Some("page_limit");
    result
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    connection: Option<&str>,
    item: Option<&str>,
    workspace: Option<&str>,
    limit: u8,
    pages: u8,
    timeout: u64,
    strict: bool,
) -> Result<()> {
    let connection = connection.map(|value| target(value, "--id")).transpose()?;
    let item = item.map(|value| target(value, "--item")).transpose()?;
    let workspace = workspace
        .map(|value| target(value, "--workspace"))
        .transpose()?;
    let valid_scope = matches!(
        (connection, item, workspace),
        (Some(_), None, _) | (None, Some(_), Some(_))
    );
    if !valid_scope || cli.continuation_token.is_some() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,
            "Choose one explicit --id OR --workspace plus --item; aggregate diagnostics cannot resume a continuation token.",
            "Use connection diagnostics --id <CONNECTION_ID> or connection diagnostics --workspace <WORKSPACE_ID> --item <ITEM_ID>.").into());
    }
    let scope = json!({"area":"connection_identity_configuration", "mutates":false, "itemBindingsOnly":item.is_some(),
        "workspaceContextIsNotBindingProof":item.is_none() && workspace.is_some(),
        "connectionLimit":limit, "pageLimit":pages, "timeoutSeconds":timeout,
        "maxLogicalReads":usize::from(workspace.is_some())+if item.is_some() {usize::from(pages)+usize::from(limit)} else {1}});
    if output::dry_run_guard(
        cli,
        "connection diagnostics",
        &json!({"connectionId":encode_id(connection),"workspaceId":encode_id(workspace),"itemId":encode_id(item),"scope":scope}),
    ) {
        return Ok(());
    }
    let budget = Budget::new(timeout);
    let metadata = if let Some(workspace) = workspace {
        Some(
            budget
                .get(client, &format!("/workspaces/{workspace}"))
                .await,
        )
    } else {
        None
    };
    let mut checks = Vec::new();
    if let Some(workspace) = workspace {
        let next = format!("fabio --wrap-untrusted --readonly workspace show --id {workspace}");
        checks.push(match metadata.as_ref() {
            Some(Ok(body)) if id(&body["id"])==Some(workspace) => Check::new("workspace_context",Status::Ready,
                "Metadata for the explicit workspace context was observed; this alone proves no connection binding or source access.",json!({"workspaceId":workspace.to_string()}),next),
            Some(Err(error)) => Check::error("workspace_context",error,"workspace",next),
            _ => Check::new("workspace_context",Status::Unknown,"Requested workspace context was not established.",json!({"reason":"scope_mismatch"}),next),
        });
    }
    let mut connections = Vec::new();
    let mut scan = None;
    if let Some(connection) = connection {
        connections.push(inspect(client, &budget, connection, workspace, metadata.as_ref()).await);
    } else if let (Some(workspace), Some(item)) = (workspace, item) {
        let bindings = references(client, &budget, workspace, item, limit, pages).await;
        checks.push(Check::new("item_connections",if bindings.complete {Status::Ready} else {Status::Unknown},
            "Only the selected item's references were requested. Empty/partial references do not establish end-to-end workload readiness.",
            serde_json::to_value(&bindings)?, references_show(workspace,item)));
        for row in &bindings.rows {
            if let Some(connection) = id(&row["id"]) {
                connections.push(
                    inspect(
                        client,
                        &budget,
                        connection,
                        Some(workspace),
                        metadata.as_ref(),
                    )
                    .await,
                );
            } else {
                let unbound = row["connectivityType"] == "None";
                let check = Check::new(
                    "connection_binding",
                    if unbound {
                        Status::NeedsAction
                    } else {
                        Status::Unknown
                    },
                    if unbound {
                        "The item reports an unbound connection. Review the intended binding; no connection was created."
                    } else {
                        "Automatic/SSO binding is implicit. Caller/source authorization is not inferred, and no explicit connection resource ID was invented or probed."
                    },
                    json!({"source":"item.connections", "connectivityType":row["connectivityType"]}),
                    references_show(workspace, item),
                );
                connections.push(ConnectionReport {
                    connection_id: None,
                    credential_health: "not_checked",
                    checks: vec![check],
                });
            }
        }
        scan = Some(bindings);
    }
    let all_checks = || {
        checks
            .iter()
            .chain(connections.iter().flat_map(|connection| &connection.checks))
    };
    let status = summary(all_checks());
    let partial = all_checks().any(|check| check.status == Status::Unknown);
    let report = json!({"connectionId":encode_id(connection),"workspaceId":encode_id(workspace),"itemId":encode_id(item),"observedAt":Utc::now().to_rfc3339(),
        "scope":scope,"status":status,"partial":partial,"checks":checks,"connections":connections,"references":scan,
        "notChecked":["stored credential validity", "source authorization", "workspace identity token acquisition", "gateway health", "code-first usage eligibility", "workload execution"],
        "limitations":"Ready applies only to requested configuration observations. No credential tests, role scans, tenant inventories, admin probes, identity provisioning, ownership transfer, or source requests are performed."});
    output::render_object(cli, &report, "status");
    if strict && status != Status::Ready {
        return Err(FabioError::with_typed_hint(ErrorCode::ApiError,
            "Connection diagnostics did not establish every requested configuration check.",
            "Review the report and its scoped read-only nextSteps. Unknown credentials/source authorization are not a reason to grant access, transfer ownership, or retry a mutation.",
            HintType::SemanticCorrection).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const CONNECTION: &str = "bbbbbbbb-1111-2222-3333-444444444444";

    #[test]
    fn identifiers_reject_names_nil_and_path_scope_before_reads() {
        for value in [
            "name",
            "../connections",
            "*",
            "00000000-0000-0000-0000-000000000000",
        ] {
            assert!(target(value, "--id").is_err());
        }
        assert_eq!(
            encode_id(Some(target(CONNECTION, "--id").unwrap())).as_deref(),
            Some(CONNECTION)
        );
    }

    #[test]
    fn effective_roles_do_not_promote_pending_or_unreported_ownership() {
        for (role, status) in [
            ("Owner", Status::Ready),
            ("User", Status::Ready),
            ("UserWithReshare", Status::Ready),
            ("PendingOwner", Status::NeedsAction),
            ("FutureRole", Status::Unknown),
        ] {
            let check = role_check(&json!({"myRole":role}), CONNECTION.parse().unwrap());
            assert_eq!(check.status, status);
            assert!(
                check
                    .next_steps
                    .iter()
                    .all(|step| !step.requires_approval && step.command_template.is_none())
            );
        }
        assert_eq!(
            role_check(&json!({}), CONNECTION.parse().unwrap()).status,
            Status::Unknown
        );
    }

    #[test]
    fn identity_configuration_never_claims_source_authorization() {
        let valid = json!({"id":WS,"workspaceIdentity":{"applicationId":CONNECTION,"servicePrincipalId":CONNECTION,
            "identityReference":{"referenceType":"ById","workspaceId":WS,"itemId":CONNECTION}}});
        let metadata = Ok(valid.clone());
        let check = identity_check(Some(WS.parse().unwrap()), Some(&metadata));
        assert_eq!(check.status, Status::Ready);
        assert_eq!(check.evidence["authorization"], "not_checked");
        let missing = Ok(json!({"id":WS,"workspaceIdentity":null}));
        assert_eq!(
            identity_check(Some(WS.parse().unwrap()), Some(&missing)).status,
            Status::NeedsAction
        );
        for body in [
            json!({"id":WS}),
            json!({"id":CONNECTION,"workspaceIdentity":valid["workspaceIdentity"]}),
            json!({"id":WS,"workspaceIdentity":{"applicationId":CONNECTION,"servicePrincipalId":CONNECTION,
                "identityReference":{"referenceType":"ByVariable","variableReference":"untrusted"}}}),
        ] {
            assert_eq!(
                identity_check(Some(WS.parse().unwrap()), Some(&Ok(body))).status,
                Status::Unknown
            );
        }
        assert_eq!(identity_check(None, None).status, Status::Unknown);
    }

    #[test]
    fn reference_pages_bound_and_deduplicate_without_following_external_uris() {
        let mut scan = References::default();
        let mut seen = HashMap::new();
        let row = json!({"id":CONNECTION,"connectivityType":"ShareableCloud","connectionDetails":{"path":"secret"}});
        assert_eq!(scan.page(&json!({"value":[row,row]}), 10, &mut seen), None);
        assert!(scan.complete);
        assert_eq!(scan.bindings_scanned, 2);
        assert_eq!(scan.rows.len(), 1);
        assert!(
            !serde_json::to_string(&scan.rows)
                .unwrap()
                .contains("secret")
        );
        let mut scan = References::default();
        assert_eq!(
            scan.page(
                &json!({"value":[],"continuationUri":"https://example.org/private"}),
                10,
                &mut seen
            ),
            None
        );
        assert!(!scan.complete);
        assert_eq!(scan.reason, Some("uri_only_continuation"));
        let mut scan = References::default();
        scan.page(
            &json!({"value":[{"connectivityType":"Automatic"},{"connectivityType":"None"}]}),
            1,
            &mut seen,
        );
        assert_eq!(scan.reason, Some("connection_limit"));
        assert_eq!(scan.rows.len(), 1);
    }

    #[test]
    fn malformed_or_conflicting_references_never_report_complete() {
        for body in [
            json!({}),
            json!({"value":[{"id":CONNECTION,"connectivityType":"FutureType"}]}),
            json!({"value":[{"connectivityType":"ShareableCloud","id":"bad"}]}),
            json!({"value":[],"continuationToken":42}),
        ] {
            let mut scan = References::default();
            scan.page(&body, 10, &mut HashMap::new());
            assert!(!scan.complete);
            assert!(scan.reason.is_some());
        }
        let mut scan = References::default();
        scan.page(
            &json!({"value":[{"id":CONNECTION,"connectivityType":"ShareableCloud"},
            {"id":CONNECTION,"connectivityType":"PersonalCloud"}]}),
            10,
            &mut HashMap::new(),
        );
        assert_eq!(scan.reason, Some("conflicting_binding"));
    }

    #[test]
    fn diagnostic_followups_parse_as_readonly_commands_without_execution() {
        use clap::CommandFactory as _;
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(|| {
                let cli = Cli::command();
                let checks = [
                    role_check(
                        &json!({"myRole":"PendingOwner"}),
                        CONNECTION.parse().unwrap(),
                    ),
                    identity_check(None, None),
                    identity_check(
                        Some(WS.parse().unwrap()),
                        Some(&Ok(json!({"id":WS,"workspaceIdentity":null}))),
                    ),
                ];
                for check in checks {
                    for step in check.next_steps {
                        if let Some(command) = step.command {
                            assert!(
                                command.contains("--readonly")
                                    || command.contains("context describe")
                            );
                            cli.clone()
                                .try_get_matches_from(shlex::split(&command).unwrap())
                                .unwrap();
                        }
                        assert!(!step.requires_approval);
                        assert!(step.command_template.is_none());
                    }
                }
                cli.try_get_matches_from(
                    shlex::split(&references_show(
                        WS.parse().unwrap(),
                        CONNECTION.parse().unwrap(),
                    ))
                    .unwrap(),
                )
                .unwrap();
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
