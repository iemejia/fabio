use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) async fn revalidate_cmk(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "sql-database revalidate-cmk",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/revalidateCMK"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database revalidate-cmk", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "cmk_revalidated" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn get_audit_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/sqlDatabases/{id}/settings/sqlAudit"
        ))
        .await?;
    output::render_object(cli, &data, "state");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_audit_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    state: Option<&str>,
    retention_days: Option<i64>,
    audit_actions: Option<&[String]>,
    predicate_expression: Option<&str>,
) -> Result<()> {
    if state.is_none()
        && retention_days.is_none()
        && audit_actions.is_none()
        && predicate_expression.is_none()
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one audit setting must be provided".to_string(),
            "Options: --state Enabled|Disabled, --retention-days N, --audit-actions GROUP1,GROUP2, --predicate-expression EXPR".to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(s) = state {
        body["state"] = Value::from(s);
    }
    if let Some(days) = retention_days {
        body["retentionDays"] = Value::Number(serde_json::Number::from(days));
    }
    if let Some(actions) = audit_actions {
        body["auditActionsAndGroups"] =
            Value::Array(actions.iter().map(|a| Value::from(a.as_str())).collect());
    }
    if let Some(pred) = predicate_expression {
        crate::commands::sql_audit::validate_predicate_expression(pred)?;
        body["predicateExpression"] = Value::from(pred);
    }

    if output::dry_run_guard(cli, "sql-database update-audit-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/sqlDatabases/{id}/settings/sqlAudit"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "sql-database update-audit-settings", "Contributor"))?;
    output::render_object(cli, &data, "state");
    Ok(())
}

pub(super) async fn list_deleted(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/sqlDatabases/restorableDeletedDatabases"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &[
            "displayName",
            "properties.restorableDeletedDatabaseName",
            "properties.deletionTimestamp",
        ],
        &["NAME", "RESTORABLE_NAME", "DELETED_AT"],
        "displayName",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}
