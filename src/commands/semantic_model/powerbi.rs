use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::parse_json_content;

pub(super) async fn list_parameters(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!("/groups/{workspace}/datasets/{id}/parameters"))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model list-parameters", "Contributor"))?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &["name", "type", "currentValue", "isRequired"],
            &["NAME", "TYPE", "CURRENT VALUE", "REQUIRED"],
            "name",
            None,
        );
    } else {
        output::render_object(cli, &data, "name");
    }
    Ok(())
}

pub(super) async fn update_parameters(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    content: &str,
) -> Result<()> {
    let body = parse_json_content(content, "update-parameters")?;

    if output::dry_run_guard(cli, "semantic-model update-parameters", &body) {
        return Ok(());
    }

    client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/Default.UpdateParameters"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model update-parameters", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "parameters_updated"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn list_datasources(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!("/groups/{workspace}/datasets/{id}/datasources"))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model list-datasources", "Contributor"))?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &["datasourceId", "datasourceType", "gatewayId"],
            &["DATASOURCE ID", "TYPE", "GATEWAY ID"],
            "datasourceId",
            None,
        );
    } else {
        output::render_object(cli, &data, "datasourceId");
    }
    Ok(())
}

pub(super) async fn update_datasources(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    content: &str,
) -> Result<()> {
    let body = parse_json_content(content, "update-datasources")?;

    if output::dry_run_guard(cli, "semantic-model update-datasources", &body) {
        return Ok(());
    }

    client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/Default.UpdateDatasources"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model update-datasources", "Contributor"))?;

    let obj = serde_json::json!({
        "id": id,
        "status": "datasources_updated"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn list_users(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!("/groups/{workspace}/datasets/{id}/users"))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model list-users", "Admin"))?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &[
                "identifier",
                "principalType",
                "datasetUserAccessRight",
                "displayName",
            ],
            &["IDENTIFIER", "TYPE", "ACCESS RIGHT", "DISPLAY NAME"],
            "identifier",
            None,
        );
    } else {
        output::render_object(cli, &data, "identifier");
    }
    Ok(())
}

pub(super) async fn add_user(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    principal: &str,
    principal_type: &str,
    access_right: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "identifier": principal,
        "principalType": principal_type,
        "datasetUserAccessRight": access_right
    });

    if output::dry_run_guard(cli, "semantic-model add-user", &body) {
        return Ok(());
    }

    client
        .post_powerbi(&format!("/groups/{workspace}/datasets/{id}/users"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model add-user", "Admin"))?;

    let obj = serde_json::json!({
        "id": id,
        "principal": principal,
        "access_right": access_right,
        "status": "user_added"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn delete_user(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    user: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "datasetId": id,
        "user": user
    });

    if output::dry_run_guard(cli, "semantic-model delete-user", &body) {
        return Ok(());
    }

    client
        .delete_powerbi(&format!("/groups/{workspace}/datasets/{id}/users/{user}"))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model delete-user", "Admin"))?;

    let obj = serde_json::json!({
        "id": id,
        "user": user,
        "status": "user_removed"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn refresh_status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    top: u32,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/refreshes?$top={top}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model refresh-status", "Contributor"))?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &["requestId", "refreshType", "status", "startTime", "endTime"],
            &["REQUEST ID", "TYPE", "STATUS", "START", "END"],
            "requestId",
            None,
        );
    } else {
        output::render_object(cli, &data, "requestId");
    }
    Ok(())
}

pub(super) async fn list_upstream(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/upstreamDatasets"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model list-upstream", "Contributor"))?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &["targetDatasetId", "groupId"],
            &["DATASET ID", "WORKSPACE ID"],
            "targetDatasetId",
            None,
        );
    } else {
        output::render_object(cli, &data, "targetDatasetId");
    }
    Ok(())
}

pub(super) async fn clone_model(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    target_workspace: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "name": name });
    if let Some(tw) = target_workspace {
        body["targetWorkspaceId"] = Value::from(tw);
    }

    if output::dry_run_guard(cli, "semantic-model clone", &body) {
        return Ok(());
    }

    let data = client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/Default.Clone"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model clone", "Contributor"))?;

    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn export_pbix(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    output_path: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(
        cli,
        "semantic-model export-pbix",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "file": output_path
        }),
    ) {
        return Ok(());
    }

    let bytes = client
        .post_powerbi_bytes(
            &format!("/groups/{workspace}/datasets/{id}/Default.Export"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model export-pbix", "Contributor"))?;

    // Ensure parent directory exists
    if let Some(parent) = Path::new(output_path).parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(output_path, &bytes)?;

    let obj = serde_json::json!({
        "id": id,
        "status": "exported",
        "file": output_path,
        "size_bytes": bytes.len()
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn import_pbix(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    file_path: &str,
    name_conflict: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "semantic-model import-pbix",
        &serde_json::json!({
            "workspace": workspace,
            "name": name,
            "file": file_path,
            "nameConflict": name_conflict
        }),
    ) {
        return Ok(());
    }

    let path = Path::new(file_path);
    if !path.exists() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            format!("File not found: {file_path}"),
        )
        .into());
    }

    let file_bytes = std::fs::read(path)?;
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("application/octet-stream")
        .map_err(|e| FabioError::api_error(format!("Failed to create multipart part: {e}")))?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let encoded_name = urlencoding::encode(name);
    let api_path = format!(
        "/groups/{workspace}/imports?datasetDisplayName={encoded_name}&nameConflict={name_conflict}"
    );

    let data = client
        .post_powerbi_multipart(&api_path, form)
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model import-pbix", "Contributor"))?;

    output::render_object(cli, &data, "id");
    Ok(())
}

// ── Gateway binding (Power BI) ────────────────────────────────────────────────
// Bind an import model's data sources to an on-premises/VNet data gateway.
// (Mirrors bind-connection/unbind-connection, but for gateways.)

pub(super) async fn get_bound_gateway_datasources(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!(
            "/groups/{workspace}/datasets/{id}/Default.GetBoundGatewayDatasources"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "semantic-model get-bound-gateway-datasources",
                "Contributor",
            )
        })?;

    if let Some(items) = data.get("value").and_then(Value::as_array) {
        output::render_list_with_token(
            cli,
            items,
            &["id", "gatewayId", "datasourceType"],
            &["DATASOURCE ID", "GATEWAY ID", "TYPE"],
            "id",
            None,
        );
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

pub(super) async fn bind_to_gateway(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    gateway_id: &str,
    datasource_ids: Option<&str>,
) -> Result<()> {
    let body = build_bind_body(gateway_id, datasource_ids);

    if output::dry_run_guard(cli, "semantic-model bind-to-gateway", &body) {
        return Ok(());
    }

    client
        .post_powerbi(
            &format!("/groups/{workspace}/datasets/{id}/Default.BindToGateway"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "semantic-model bind-to-gateway", "Contributor"))?;

    let obj =
        serde_json::json!({ "id": id, "gatewayId": gateway_id, "status": "bound_to_gateway" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Build the `BindToGateway` body: `gatewayObjectId` + optional
/// `datasourceObjectIds` (a comma-separated list; omitted binds all).
fn build_bind_body(gateway_id: &str, datasource_ids: Option<&str>) -> Value {
    let mut body = serde_json::json!({ "gatewayObjectId": gateway_id });
    if let Some(ids) = datasource_ids {
        let list: Vec<String> = ids
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect();
        if !list.is_empty() {
            body["datasourceObjectIds"] = Value::from(list);
        }
    }
    body
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_bind_body_gateway_only() {
        let body = build_bind_body("gw-1", None);
        assert_eq!(body, serde_json::json!({ "gatewayObjectId": "gw-1" }));
    }

    #[test]
    fn build_bind_body_with_datasource_ids() {
        let body = build_bind_body("gw-1", Some("a, b ,c"));
        assert_eq!(body["gatewayObjectId"], "gw-1");
        assert_eq!(
            body["datasourceObjectIds"],
            serde_json::json!(["a", "b", "c"])
        );
    }

    #[test]
    fn build_bind_body_empty_datasource_ids_omitted() {
        let body = build_bind_body("gw-1", Some("  ,  "));
        assert!(body.get("datasourceObjectIds").is_none());
    }
}
