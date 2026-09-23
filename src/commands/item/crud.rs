use std::fmt::Write;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::{enrich_item_create_error, enrich_item_not_found_error};

// ─── List ────────────────────────────────────────────────────────────────────

pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_type: Option<&str>,
    folder: Option<&str>,
    recursive: Option<bool>,
    include: Option<&str>,
) -> Result<()> {
    let mut path = format!("/workspaces/{workspace}/items");
    let mut params: Vec<String> = Vec::new();
    if let Some(t) = item_type {
        params.push(format!("type={t}"));
    }
    if let Some(folder_id) = folder {
        params.push(format!("rootFolderId={folder_id}"));
    }
    if let Some(r) = recursive {
        params.push(format!("recursive={r}"));
    }
    if let Some(inc) = include {
        params.push(format!("include={inc}"));
    }
    if !params.is_empty() {
        let _ = write!(path, "?{}", params.join("&"));
    }

    let resp = client
        .get_list(&path, "value", cli.all, cli.continuation_token.as_deref())
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "logicalId", "type"],
        &["NAME", "ID", "LOGICAL ID", "TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

// ─── Show ────────────────────────────────────────────────────────────────────

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/items/{id}"))
        .await
        .map_err(|e| enrich_item_not_found_error(e, workspace, id))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── List Connections ────────────────────────────────────────────────────────

pub(super) async fn list_connections(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{id}/connections"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "item list-connections", "ReadWrite"))?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "connectivityType", "displayName"],
        &["ID", "TYPE", "NAME"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

// ─── Relations (beta) ───────────────────────────────────────────────────────

fn relations_path(workspace: &str, id: &str, direction: &str) -> String {
    format!("/workspaces/{workspace}/items/{id}/relations/{direction}?beta=true")
}

pub(super) async fn list_relations(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    direction: &str,
) -> Result<()> {
    let data = client
        .get(&relations_path(workspace, id, direction))
        .await
        .map_err(|e| enrich_forbidden(e, &format!("item list-{direction}-relations"), "Viewer"))?;

    output::render_object(cli, &data, "items");
    Ok(())
}

// ─── Exists ──────────────────────────────────────────────────────────────────

pub(super) async fn exists(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let result = client
        .get(&format!("/workspaces/{workspace}/items/{id}"))
        .await;
    let item_exists = result.is_ok();
    let data = serde_json::json!({ "exists": item_exists, "id": id, "workspaceId": workspace });
    output::render_object(cli, &data, "exists");
    Ok(())
}

// ─── Url ─────────────────────────────────────────────────────────────────────

#[allow(clippy::unnecessary_wraps)]
pub(super) fn url(cli: &Cli, workspace: &str, id: &str, item_type: Option<&str>) -> Result<()> {
    // Construct a portal URL. The path segment varies by item type.
    let type_segment = item_type.map_or_else(
        || format!("/groups/{workspace}/items/{id}"),
        |t| {
            let lower = t.to_lowercase();
            match lower.as_str() {
                "lakehouse" => format!("/groups/{workspace}/lakehouses/{id}"),
                "notebook" => format!("/groups/{workspace}/notebooks/{id}"),
                "warehouse" | "datawarehouse" => format!("/groups/{workspace}/warehouses/{id}"),
                "report" => format!("/groups/{workspace}/reports/{id}"),
                "semanticmodel" | "dataset" => format!("/groups/{workspace}/datasets/{id}"),
                "datapipeline" | "pipeline" => format!("/groups/{workspace}/pipelines/{id}"),
                "eventhouse" => format!("/groups/{workspace}/eventhouses/{id}"),
                "kqldatabase" => format!("/groups/{workspace}/kqldatabases/{id}"),
                "eventstream" => format!("/groups/{workspace}/eventstreams/{id}"),
                _ => format!("/groups/{workspace}/items/{id}"),
            }
        },
    );
    let portal_url = format!("https://app.fabric.microsoft.com{type_segment}");
    let mut data = serde_json::json!({ "url": portal_url, "itemId": id, "workspaceId": workspace });
    if let Some(t) = item_type {
        data["itemType"] = Value::from(t);
    }
    output::render_object(cli, &data, "url");
    Ok(())
}

// ─── Inspect ─────────────────────────────────────────────────────────────────

pub(super) async fn inspect(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // Fetch item metadata (always succeeds for valid items)
    let metadata = client
        .get(&format!("/workspaces/{workspace}/items/{id}"))
        .await
        .map_err(|e| enrich_item_not_found_error(e, workspace, id))?;

    // Fetch definition (best-effort — some items don't support it)
    let definition = client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .ok();

    // Fetch connections (best-effort)
    let connections = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{id}/connections"),
            "value",
            true,
            None,
        )
        .await
        .ok()
        .map(|r| r.items);

    // Build aggregated response
    let mut result = serde_json::json!({
        "metadata": metadata,
    });
    if let Some(def) = definition {
        result["definition"] = def;
    }
    if let Some(conns) = connections {
        result["connections"] = Value::Array(conns);
    }

    output::render_object(cli, &result, "metadata.id");
    Ok(())
}

// ─── Create ──────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    item_type: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    definition: Option<&str>,
    options: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
        "type": item_type,
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }
    if let Some(input) = definition {
        let raw = crate::commands::query_input::resolve_query_input(
            Some(input),
            "item definition JSON",
            "--definition",
            "--definition '{\"parts\":[{\"path\":\"definition.json\",\"payload\":\"...\",\"payloadType\":\"InlineBase64\"}]}'",
        )?;
        let parsed: Value = serde_json::from_str(&raw).map_err(|error| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --definition JSON: {error}"),
                "Provide a definition object with a parts array, inline or as @file.",
            )
        })?;
        let definition_value = parsed.get("definition").cloned().unwrap_or(parsed);
        // The definition must be an object (with a parts array). Reject arrays/scalars
        // (e.g. `--definition '[]'`) locally with INVALID_INPUT instead of sending an
        // invalid `definition` to Fabric or reporting a misleading successful dry-run.
        if !definition_value.is_object() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "The --definition value must be a JSON object with a parts array, not an array or scalar."
                    .to_string(),
                "Provide a definition object, inline or as @file: --definition '{\"parts\":[{\"path\":\"...\",\"payload\":\"...\",\"payloadType\":\"InlineBase64\"}]}'"
                    .to_string(),
            )
            .into());
        }
        body["definition"] = definition_value;
    }
    if let Some(input) = options {
        body["options"] = crate::commands::item_options::parse_options_object(input, "--options")?;
    }

    // The generic --options path can enable the irreversible allowPurgeData option
    // on a create; surface the same warning + destructive signal as the typed path.
    let purge = output::body_enables_purge(&body);
    let mut preview = serde_json::json!({
        "workspace": workspace,
        "displayName": name,
        "type": item_type,
        "description": description,
        "sensitivityLabel": sensitivity_label,
        "definition": body.get("definition"),
        "options": body.get("options")
    });
    if purge {
        preview["warning"] = Value::from(output::ALLOW_PURGE_DATA_WARNING);
    }
    if output::dry_run_guard_maybe_destructive(cli, "item create", &preview, purge) {
        return Ok(());
    }

    let mut data = client
        .post(&format!("/workspaces/{workspace}/items"), &body, true)
        .await
        .map_err(|e| enrich_item_create_error(e, item_type))
        .map_err(|e| enrich_forbidden(e, "item create", "Member"))?;
    output::attach_purge_warning(&mut data, purge);
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Update ──────────────────────────────────────────────────────────────────

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio item update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "item update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/items/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "item update", "ReadWrite"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct UpdateLogicalIdRequest {
    logical_id: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct LogicalIdResponse {
    logical_id: String,
}

fn update_logical_id_path(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/items/{id}/logicalId")
}

fn build_update_logical_id_request(logical_id: &str) -> Result<UpdateLogicalIdRequest> {
    crate::client::validate_uuid(logical_id, "--logical-id")?;
    if logical_id == "00000000-0000-0000-0000-000000000000" {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--logical-id cannot be the empty GUID",
            "Provide a unique, non-empty GUID that is not already assigned in this workspace.",
        )
        .into());
    }
    Ok(UpdateLogicalIdRequest {
        logical_id: logical_id.to_owned(),
    })
}

fn enrich_update_logical_id_error(err: anyhow::Error, workspace: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };
    if fabio_err.message.contains("ActiveCiCdOperationInProgress") {
        return fabio_err
            .with_replaced_hint(
                format!(
                    "Wait for the active CI/CD operation in workspace '{workspace}' to finish, then retry. \
                     Review automations that use the current logical ID before changing it."
                ),
                None,
            )
            .into();
    }
    err
}

pub(super) async fn update_logical_id(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    logical_id: &str,
) -> Result<()> {
    crate::client::validate_uuid(workspace, "--workspace")?;
    crate::client::validate_uuid(id, "--id")?;
    let request = build_update_logical_id_request(logical_id)?;
    let body = serde_json::to_value(&request)?;

    if output::dry_run_guard(
        cli,
        "item update-logical-id",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "logicalId": logical_id,
            "warning": "Changing a logical ID can break automations that rely on its current value."
        }),
    ) {
        return Ok(());
    }

    let data = client
        .patch(&update_logical_id_path(workspace, id), &body)
        .await
        .map_err(|e| enrich_item_not_found_error(e, workspace, id))
        .map_err(|e| enrich_update_logical_id_error(e, workspace))
        .map_err(|e| enrich_forbidden(e, "item update-logical-id", "Contributor"))?;
    let response: LogicalIdResponse = serde_json::from_value(data)?;
    output::render_object(cli, &serde_json::to_value(response)?, "logicalId");
    Ok(())
}

// ─── Delete ──────────────────────────────────────────────────────────────────

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "item delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/items/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/items/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "item delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Assign Identity ─────────────────────────────────────────────────────────

pub(super) async fn assign_identity(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    assignment_type: &str,
) -> Result<()> {
    // The associate-identity request REQUIRES `assignmentType` and the endpoint
    // is gated behind the REQUIRED `?beta=true` query parameter — omitting
    // either fails (`InvalidInput` / `BetaParameterRequired`).
    let body = serde_json::json!({ "assignmentType": assignment_type });

    if output::dry_run_guard(
        cli,
        "item assign-identity",
        &serde_json::json!({ "workspace": workspace, "id": id, "assignmentType": assignment_type }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/identities/default/assign?beta=true"),
            &body,
            false,
        )
        .await?;

    let obj = serde_json::json!({ "id": id, "status": "identity_assigned" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── External Data Share Invitations ─────────────────────────────────────────

pub(super) async fn get_invitation(
    cli: &Cli,
    client: &FabricClient,
    invitation_id: &str,
    provider_tenant_id: &str,
) -> Result<()> {
    // `providerTenantId` (the tenant that issued the invitation) is a REQUIRED
    // query param — omitting it fails with InvalidInput.
    let data = client
        .get(&format!(
            "/externalDataShares/invitations/{invitation_id}?providerTenantId={provider_tenant_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn accept_invitation(
    cli: &Cli,
    client: &FabricClient,
    invitation_id: &str,
    workspace: &str,
    name: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "workspaceId": workspace,
        "displayName": name
    });

    if output::dry_run_guard(cli, "item accept-invitation", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/externalDataShares/invitations/{invitation_id}/accept"),
            &body,
            false,
        )
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    /// Helper that replicates the `url()` mapping logic for testing.
    fn portal_url_for(workspace: &str, id: &str, item_type: Option<&str>) -> String {
        let type_segment = item_type.map_or_else(
            || format!("/groups/{workspace}/items/{id}"),
            |t| {
                let lower = t.to_lowercase();
                match lower.as_str() {
                    "lakehouse" => format!("/groups/{workspace}/lakehouses/{id}"),
                    "notebook" => format!("/groups/{workspace}/notebooks/{id}"),
                    "warehouse" | "datawarehouse" => {
                        format!("/groups/{workspace}/warehouses/{id}")
                    }
                    "report" => format!("/groups/{workspace}/reports/{id}"),
                    "semanticmodel" | "dataset" => format!("/groups/{workspace}/datasets/{id}"),
                    "datapipeline" | "pipeline" => format!("/groups/{workspace}/pipelines/{id}"),
                    "eventhouse" => format!("/groups/{workspace}/eventhouses/{id}"),
                    "kqldatabase" => format!("/groups/{workspace}/kqldatabases/{id}"),
                    "eventstream" => format!("/groups/{workspace}/eventstreams/{id}"),
                    _ => format!("/groups/{workspace}/items/{id}"),
                }
            },
        );
        format!("https://app.fabric.microsoft.com{type_segment}")
    }

    #[test]
    fn url_lakehouse_maps_to_lakehouses() {
        let u = portal_url_for("ws-1", "item-1", Some("Lakehouse"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/lakehouses/item-1"
        );
    }

    #[test]
    fn url_notebook_maps_to_notebooks() {
        let u = portal_url_for("ws-1", "item-1", Some("Notebook"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/notebooks/item-1"
        );
    }

    #[test]
    fn url_warehouse_maps_to_warehouses() {
        let u = portal_url_for("ws-1", "item-1", Some("Warehouse"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/warehouses/item-1"
        );
    }

    #[test]
    fn url_datawarehouse_alias_maps_to_warehouses() {
        let u = portal_url_for("ws-1", "item-1", Some("DataWarehouse"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/warehouses/item-1"
        );
    }

    #[test]
    fn url_report_maps_to_reports() {
        let u = portal_url_for("ws-1", "item-1", Some("Report"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/reports/item-1"
        );
    }

    #[test]
    fn url_semanticmodel_maps_to_datasets() {
        let u = portal_url_for("ws-1", "item-1", Some("SemanticModel"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/datasets/item-1"
        );
    }

    #[test]
    fn url_dataset_alias_maps_to_datasets() {
        let u = portal_url_for("ws-1", "item-1", Some("Dataset"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/datasets/item-1"
        );
    }

    #[test]
    fn url_datapipeline_maps_to_pipelines() {
        let u = portal_url_for("ws-1", "item-1", Some("DataPipeline"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/pipelines/item-1"
        );
    }

    #[test]
    fn url_pipeline_alias_maps_to_pipelines() {
        let u = portal_url_for("ws-1", "item-1", Some("Pipeline"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/pipelines/item-1"
        );
    }

    #[test]
    fn url_eventhouse_maps_to_eventhouses() {
        let u = portal_url_for("ws-1", "item-1", Some("Eventhouse"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/eventhouses/item-1"
        );
    }

    #[test]
    fn url_kqldatabase_maps_to_kqldatabases() {
        let u = portal_url_for("ws-1", "item-1", Some("KQLDatabase"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/kqldatabases/item-1"
        );
    }

    #[test]
    fn url_eventstream_maps_to_eventstreams() {
        let u = portal_url_for("ws-1", "item-1", Some("Eventstream"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/eventstreams/item-1"
        );
    }

    #[test]
    fn url_unknown_type_falls_back_to_items() {
        let u = portal_url_for("ws-1", "item-1", Some("UnknownFoo"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/items/item-1"
        );
    }

    #[test]
    fn url_no_type_uses_generic_items_path() {
        let u = portal_url_for("ws-1", "item-1", None);
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/items/item-1"
        );
    }

    #[test]
    fn url_case_insensitive_type() {
        let u = portal_url_for("ws-1", "item-1", Some("lAkEhOuSe"));
        assert_eq!(
            u,
            "https://app.fabric.microsoft.com/groups/ws-1/lakehouses/item-1"
        );
    }

    #[test]
    fn relations_path_downstream_includes_beta_flag() {
        let p = super::relations_path("ws-1", "item-1", "downstream");
        assert_eq!(
            p,
            "/workspaces/ws-1/items/item-1/relations/downstream?beta=true"
        );
    }

    #[test]
    fn relations_path_upstream_includes_beta_flag() {
        let p = super::relations_path("ws-1", "item-1", "upstream");
        assert_eq!(
            p,
            "/workspaces/ws-1/items/item-1/relations/upstream?beta=true"
        );
    }

    #[test]
    fn update_logical_id_request_matches_spec_example() {
        let request =
            super::build_update_logical_id_request("cfafbeb1-8037-4d0c-896e-a46fb27ff229").unwrap();
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            serde_json::json!({"logicalId": "cfafbeb1-8037-4d0c-896e-a46fb27ff229"})
        );
        assert_eq!(
            super::update_logical_id_path(
                "cfafbeb1-8037-4d0c-896e-a46fb27ff229",
                "5b218778-e7a5-4d73-8187-f10824047715"
            ),
            "/workspaces/cfafbeb1-8037-4d0c-896e-a46fb27ff229/items/5b218778-e7a5-4d73-8187-f10824047715/logicalId"
        );
    }

    #[test]
    fn update_logical_id_response_matches_spec_example() {
        let response: super::LogicalIdResponse = serde_json::from_value(serde_json::json!({
            "logicalId": "cfafbeb1-8037-4d0c-896e-a46fb27ff229"
        }))
        .unwrap();
        assert_eq!(response.logical_id, "cfafbeb1-8037-4d0c-896e-a46fb27ff229");
    }

    #[test]
    fn update_logical_id_rejects_empty_guid() {
        let error = super::build_update_logical_id_request("00000000-0000-0000-0000-000000000000")
            .unwrap_err();
        let fabio_error = error.downcast_ref::<crate::errors::FabioError>().unwrap();
        assert_eq!(fabio_error.code, crate::errors::ErrorCode::InvalidInput);
        assert!(fabio_error.message.contains("empty GUID"));
    }

    #[test]
    fn update_logical_id_enriches_active_cicd_error() {
        let error: anyhow::Error = crate::errors::FabioError::new(
            crate::errors::ErrorCode::Conflict,
            "ActiveCiCdOperationInProgress",
        )
        .into();
        let enriched = super::enrich_update_logical_id_error(error, "workspace-id");
        let fabio_error = enriched
            .downcast_ref::<crate::errors::FabioError>()
            .unwrap();
        assert!(
            fabio_error
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("workspace-id"))
        );
    }
}
