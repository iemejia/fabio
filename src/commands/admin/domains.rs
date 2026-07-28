use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_admin};
use crate::output;

use super::read_body;

pub(super) async fn list_domains(
    cli: &Cli,
    client: &FabricClient,
    non_empty_only: bool,
    with_assigned_workspaces_only: bool,
) -> Result<()> {
    let mut url = "/admin/domains".to_string();
    let mut params: Vec<&str> = Vec::new();
    if non_empty_only {
        params.push("nonEmptyOnly=true");
    }
    if with_assigned_workspaces_only {
        params.push("withAssignedWorkspacesOnly=true");
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let resp = client
        .get_list(&url, "domains", cli.all, cli.continuation_token.as_deref())
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn create_domain(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    description: Option<&str>,
    parent_id: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(parent) = parent_id {
        body["parentDomainId"] = Value::from(parent);
    }

    if output::dry_run_guard(cli, "admin create-domain", &body) {
        return Ok(());
    }

    let data = client
        .post("/admin/domains", &body, false)
        .await
        .map_err(|e| enrich_admin(e, "admin create-domain"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn show_domain(cli: &Cli, client: &FabricClient, domain_id: &str) -> Result<()> {
    let data = client.get(&format!("/admin/domains/{domain_id}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_domain(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio admin update-domain --domain-id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "admin update-domain", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/admin/domains/{domain_id}"), &body)
        .await
        .map_err(|e| enrich_admin(e, "admin update-domain"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_domain(cli: &Cli, client: &FabricClient, domain_id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "admin delete-domain",
        &serde_json::json!({ "domainId": domain_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/admin/domains/{domain_id}"))
        .await
        .map_err(|e| enrich_admin(e, "admin delete-domain"))?;

    let obj = serde_json::json!({ "domainId": domain_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn list_domain_workspaces(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/admin/domains/{domain_id}/workspaces"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id"],
        &["NAME", "ID"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn assign_domain_workspaces(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    workspace_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "workspacesIds": workspace_ids });

    if output::dry_run_guard(cli, "admin assign-domain-workspaces", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/assignWorkspaces"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin assign-domain-workspaces"))?;

    let obj = serde_json::json!({
        "domainId": domain_id,
        "workspacesAssigned": workspace_ids.len(),
        "status": "assigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn unassign_domain_workspaces(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    workspace_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "workspacesIds": workspace_ids });

    if output::dry_run_guard(cli, "admin unassign-domain-workspaces", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/unassignWorkspaces"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin unassign-domain-workspaces"))?;

    let obj = serde_json::json!({
        "domainId": domain_id,
        "workspacesUnassigned": workspace_ids.len(),
        "status": "unassigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn unassign_all_domain_workspaces(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(cli, "admin unassign-all-domain-workspaces", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/unassignAllWorkspaces"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin unassign-all-domain-workspaces"))?;

    let obj = serde_json::json!({ "domainId": domain_id, "status": "all_unassigned" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn list_domain_role_assignments(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/admin/domains/{domain_id}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "principal", "role"],
        &["ID", "PRINCIPAL", "ROLE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn bulk_assign_domain_roles(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_body(file, content, "bulk-assign-domain-roles")?;

    if output::dry_run_guard(cli, "admin bulk-assign-domain-roles", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/admin/domains/{domain_id}/roleAssignments/bulkAssign"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin bulk-assign-domain-roles"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

pub(super) async fn bulk_unassign_domain_roles(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_body(file, content, "bulk-unassign-domain-roles")?;

    if output::dry_run_guard(cli, "admin bulk-unassign-domain-roles", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/admin/domains/{domain_id}/roleAssignments/bulkUnassign"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin bulk-unassign-domain-roles"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

pub(super) async fn sync_domain_roles_to_subdomains(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    role: &str,
) -> Result<()> {
    let body = serde_json::json!({"role": role});

    if output::dry_run_guard(cli, "admin sync-domain-roles-to-subdomains", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/roleAssignments/syncToSubdomains"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin sync-domain-roles-to-subdomains"))?;

    let obj = serde_json::json!({ "domainId": domain_id, "status": "synced" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn assign_domain_workspaces_by_capacities(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    capacity_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "capacitiesIds": capacity_ids });

    if output::dry_run_guard(cli, "admin assign-domain-workspaces-by-capacities", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/assignWorkspacesByCapacities"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin assign-domain-workspaces-by-capacities"))?;

    let obj = serde_json::json!({
        "domainId": domain_id,
        "capacitiesUsed": capacity_ids.len(),
        "status": "assigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn assign_domain_workspaces_by_principals(
    cli: &Cli,
    client: &FabricClient,
    domain_id: &str,
    principal_ids: &[String],
    principal_type: &str,
) -> Result<()> {
    let principals: Vec<Value> = principal_ids
        .iter()
        .map(|id| serde_json::json!({ "id": id, "type": principal_type }))
        .collect();
    let body = serde_json::json!({ "principals": principals });

    if output::dry_run_guard(cli, "admin assign-domain-workspaces-by-principals", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/domains/{domain_id}/assignWorkspacesByPrincipals"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin assign-domain-workspaces-by-principals"))?;

    let obj = serde_json::json!({
        "domainId": domain_id,
        "principalsUsed": principal_ids.len(),
        "status": "assigned"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}
