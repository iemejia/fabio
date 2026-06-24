use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_admin;
use crate::output;

pub(super) async fn list_workspaces(
    cli: &Cli,
    client: &FabricClient,
    include: Option<&str>,
    encryption_status: Option<&str>,
) -> Result<()> {
    let mut url = "/admin/workspaces".to_string();
    let mut params: Vec<String> = Vec::new();
    if let Some(inc) = include {
        params.push(format!("include={inc}"));
    }
    if let Some(status) = encryption_status {
        params.push(format!("encryptionStatus={status}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let resp = client
        .get_list(
            &url,
            "workspaces",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "id", "state", "type", "capacityId"],
        &["NAME", "ID", "STATE", "TYPE", "CAPACITY"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show_workspace(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/admin/workspaces/{workspace}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list_workspace_users(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/admin/workspaces/{workspace}/users"),
            "accessDetails",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["principal", "workspaceAccessDetails"],
        &["PRINCIPAL", "ACCESS"],
        "principal",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn list_git_connections(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/admin/workspaces/discoverGitConnections",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["workspaceId", "gitProviderType"],
        &["WORKSPACE", "PROVIDER"],
        "workspaceId",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn grant_admin_access(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(cli, "admin grant-admin-access", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/workspaces/{workspace}/grantAdminTemporaryAccess"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin grant-admin-access"))?;

    let obj = serde_json::json!({ "workspaceId": workspace, "status": "granted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn remove_admin_access(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let body = serde_json::json!({});

    if output::dry_run_guard(cli, "admin remove-admin-access", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/admin/workspaces/{workspace}/removeAdminTemporaryAccess"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin remove-admin-access"))?;

    let obj = serde_json::json!({ "workspaceId": workspace, "status": "removed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn restore_workspace(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    capacity_id: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "restoredWorkspaceName": name,
        "capacityId": capacity_id
    });

    if output::dry_run_guard(cli, "admin restore-workspace", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/admin/workspaces/{workspace}/restore"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_admin(e, "admin restore-workspace"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn list_network_policies(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/admin/workspaces/networking/communicationpolicies",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "workspaceId", "policyType"],
        &["ID", "WORKSPACE", "TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}
