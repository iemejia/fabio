use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const SQL_POOLS_CONFIGURATION_PATH: &str = "warehouses/sqlPoolsConfiguration";

fn sql_pools_configuration_path(workspace: &str) -> String {
    format!("/workspaces/{workspace}/{SQL_POOLS_CONFIGURATION_PATH}")
}

fn parse_sql_pools_configuration(content: &str) -> Result<Value> {
    Ok(serde_json::from_str(content)?)
}

pub(super) async fn connection_string(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    guest_tenant_id: Option<&str>,
    private_link_type: Option<&str>,
) -> Result<()> {
    let mut url = format!("/workspaces/{workspace}/warehouses/{id}/connectionString");
    let mut params = Vec::new();
    if let Some(tenant) = guest_tenant_id {
        params.push(format!("guestTenantId={tenant}"));
    }
    if let Some(link_type) = private_link_type {
        params.push(format!("privateLinkType={link_type}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let data = client
        .get(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse connection-string", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn get_sql_pools_config(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&sql_pools_configuration_path(workspace))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse get-sql-pools-config", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_sql_pools_config(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body: Value = match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f).map_err(|e| {
                FabioError::not_found(format!("Configuration file not found: {f}: {e}"))
            })?;
            parse_sql_pools_configuration(&text)?
        }
        (_, Some(c)) => parse_sql_pools_configuration(c)?,
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio warehouse update-sql-pools-config --workspace <WS> --content '{...}'"
                    .to_string(),
            )
            .into());
        }
    };

    if output::dry_run_guard(cli, "warehouse update-sql-pools-config", &body) {
        return Ok(());
    }

    let data = client
        .patch(&sql_pools_configuration_path(workspace), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse update-sql-pools-config", "Admin"))?;
    output::render_object(cli, &data, "id");
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
            "/workspaces/{workspace}/warehouses/{id}/settings/sqlAudit"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse get-audit-settings", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_audit_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    state: Option<&str>,
    retention_days: Option<u32>,
    audit_actions: Option<&str>,
    predicate_expression: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({});
    if let Some(s) = state {
        body["state"] = Value::from(s);
    }
    if let Some(days) = retention_days {
        body["retentionDays"] = Value::from(days);
    }
    if let Some(actions) = audit_actions {
        let list: Vec<&str> = actions.split(',').map(str::trim).collect();
        body["auditActionsAndGroups"] = serde_json::json!(list);
    }
    if let Some(pred) = predicate_expression {
        crate::commands::sql_audit::validate_predicate_expression(pred)?;
        body["predicateExpression"] = Value::from(pred);
    }

    if output::dry_run_guard(cli, "warehouse update-audit-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/warehouses/{id}/settings/sqlAudit"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse update-audit-settings", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn set_audit_actions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    actions: &[String],
) -> Result<()> {
    let body = serde_json::json!({
        "auditActionsAndGroups": actions,
    });

    if output::dry_run_guard(cli, "warehouse set-audit-actions", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/warehouses/{id}/settings/sqlAudit/setAuditActionsAndGroups"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse set-audit-actions", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_pools_path_no_longer_uses_beta_query_parameter() {
        assert_eq!(
            sql_pools_configuration_path("workspace-id"),
            "/workspaces/workspace-id/warehouses/sqlPoolsConfiguration"
        );
    }

    #[test]
    fn sql_pools_configuration_accepts_statement_type_classifier() {
        let body = parse_sql_pools_configuration(
            r#"{
                "customSQLPoolsEnabled": true,
                "customSQLPools": [{
                    "name": "Read workloads",
                    "classifier": {
                        "type": "Statement Type",
                        "value": ["SELECT"]
                    }
                }]
            }"#,
        )
        .unwrap();

        assert_eq!(
            body["customSQLPools"][0]["classifier"]["type"],
            "Statement Type"
        );
        assert_eq!(
            body["customSQLPools"][0]["classifier"]["value"][0],
            "SELECT"
        );
    }
}
