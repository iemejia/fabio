use anyhow::Result;
use serde_json::Value;

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

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "name",
                "id",
                "state",
                "type",
                "capacityId",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &[
                "NAME",
                "ID",
                "STATE",
                "TYPE",
                "CAPACITY",
                "SENSITIVITY LABEL",
                "TAGS",
            ],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "name",
                "id",
                "state",
                "type",
                "capacityId",
                "sensitivityLabel.id",
            ],
            &[
                "NAME",
                "ID",
                "STATE",
                "TYPE",
                "CAPACITY",
                "SENSITIVITY LABEL",
            ],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["name", "id", "state", "type", "capacityId", "_tagsDisplay"],
            &["NAME", "ID", "STATE", "TYPE", "CAPACITY", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["name", "id", "state", "type", "capacityId"],
            &["NAME", "ID", "STATE", "TYPE", "CAPACITY"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
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

pub(super) async fn list_network_policies(
    cli: &Cli,
    client: &FabricClient,
    filter: Option<&str>,
) -> Result<()> {
    let mut url = "/admin/workspaces/networking/communicationpolicies".to_string();
    if let Some(f) = filter {
        url.push_str("?filter=");
        url.push_str(&urlencoding::encode(f));
    }

    let resp = client
        .get_list(&url, "value", cli.all, cli.continuation_token.as_deref())
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["workspaceId", "workspaceName", "workspaceType"],
        &["WORKSPACE ID", "WORKSPACE NAME", "TYPE"],
        "workspaceId",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}
