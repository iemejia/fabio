//! Role-assignment handlers for `fabio connection` (list/add/show/update/delete)
//! plus the `test-connection` action.

use anyhow::Result;
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

pub(super) async fn list_role_assignments(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/connections/{id}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "role", "principal.id", "principal.type"],
        &["ID", "ROLE", "PRINCIPAL ID", "PRINCIPAL TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn add_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    principal_id: &str,
    principal_type: &str,
    role: &str,
) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would add role assignment '{role}' for principal '{principal_id}' on connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({
        "principal": {
            "id": principal_id,
            "type": principal_type,
        },
        "role": role,
    });

    let data = client
        .post(&format!("/connections/{id}/roleAssignments"), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "connection add-role-assignment", "Owner"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn show_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
    role: &str,
) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would update role assignment '{assignment_id}' to role '{role}' on connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({ "role": role });

    let data = client
        .patch(
            &format!("/connections/{id}/roleAssignments/{assignment_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "connection update-role-assignment", "Owner"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "connection delete-role-assignment",
        &json!({ "id": id, "assignmentId": assignment_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "connection delete-role-assignment", "Owner"))?;

    let result = json!({
        "status": "deleted",
        "id": assignment_id,
        "connectionId": id,
    });
    output::render_object(cli, &result, "id");
    Ok(())
}

pub(super) async fn test_connection(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would test connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({});
    let data = client
        .post(&format!("/connections/{id}/testConnection"), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "connection test-connection", "User"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}
