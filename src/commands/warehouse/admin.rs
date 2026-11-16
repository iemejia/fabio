use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

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
    validate_uuid(workspace, "--workspace")?;
    let data = client
        .get(&sql_pools_config_path(workspace))
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
    validate_uuid(workspace, "--workspace")?;
    let body: Value = match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f).map_err(|e| {
                FabioError::not_found(format!("Configuration file not found: {f}: {e}"))
            })?;
            parse_sql_pools_config(&text)?
        }
        (_, Some(c)) => parse_sql_pools_config(c)?,
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

    let preview = serde_json::json!({
        "workspaceId": workspace,
        "configuration": body,
        "warnings": ["This replaces workspace-wide pool configuration. Pools omitted from customSQLPools are removed; preserve an approved baseline and review every warehouse affected."],
    });
    if output::dry_run_guard(cli, "warehouse update-sql-pools-config", &preview) {
        return Ok(());
    }

    let data = client
        .patch(&sql_pools_config_path(workspace), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse update-sql-pools-config", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

fn sql_pools_config_path(workspace: &str) -> String {
    format!("/workspaces/{workspace}/warehouses/sqlPoolsConfiguration")
}

/// Preserve both rollout contracts without manufacturing the retiring mode flag.
fn parse_sql_pools_config(text: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| FabioError::invalid_input("SQL pools configuration must be valid JSON"))?;
    if !value.is_object()
        || value.get("data").is_some()
        || value
            .get("customSQLPools")
            .is_some_and(|pools| !pools.is_array())
        || value
            .get("customSQLPoolsEnabled")
            .is_some_and(|enabled| !enabled.is_boolean())
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Expected a SQL pools configuration object, not a fabio output envelope",
            "Save the raw configuration with warehouse get-sql-pools-config --workspace <WS> -o plain. customSQLPools must be an array when supplied; customSQLPoolsEnabled is a legacy optional boolean, not required by the unified model. Review the complete pool list before replacing it.",
        )
        .into());
    }
    Ok(value)
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
    fn sql_pools_configuration_is_ga_path() {
        let path = sql_pools_config_path("workspace");
        assert_eq!(
            path,
            "/workspaces/workspace/warehouses/sqlPoolsConfiguration"
        );
        assert!(!path.contains("beta"));
    }

    #[test]
    fn statement_type_classifier_is_preserved() {
        let body = parse_sql_pools_config(
            r#"{"customSQLPoolsEnabled":true,"customSQLPools":[{"name":"etl","isDefault":true,"classifier":{"type":"Statement Type","value":["SELECT"]}}]}"#,
        )
        .unwrap();
        assert_eq!(
            body["customSQLPools"][0]["classifier"]["type"],
            "Statement Type"
        );
    }

    #[test]
    fn unified_sql_pools_preserves_configuration_without_mode_flag() {
        let text = r#"{"customSQLPools":[{"name":"etl","isDefault":true,"maxResourcePercentage":20,"optimizeForReads":false}],"futureProperty":{"enabled":true}}"#;
        let body = parse_sql_pools_config(text).unwrap();
        assert_eq!(body, serde_json::from_str::<Value>(text).unwrap());
        assert_eq!(body.get("customSQLPoolsEnabled"), None);
    }

    #[test]
    fn sql_pools_accepts_legacy_toggle_only_without_rewriting_it() {
        let body = parse_sql_pools_config(r#"{"customSQLPoolsEnabled":false}"#).unwrap();
        assert_eq!(body, serde_json::json!({"customSQLPoolsEnabled": false}));
    }

    #[test]
    fn sql_pools_rejects_malformed_shapes_and_output_envelopes() {
        for text in [
            "not JSON",
            "[]",
            "null",
            r#"{"data":{"customSQLPools":[]}}"#,
            r#"{"customSQLPools":{}}"#,
            r#"{"customSQLPoolsEnabled":"true"}"#,
        ] {
            assert!(parse_sql_pools_config(text).is_err(), "accepted {text}");
        }
    }
}
