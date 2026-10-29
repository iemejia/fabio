use anyhow::Result;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::enrich_forbidden;
use crate::output;

pub(super) async fn list(cli: &Cli, client: &FabricClient) -> Result<()> {
    let response = client
        .get_list(
            "/admin/connections",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "admin list-connections", "Fabric administrator"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["displayName", "id", "connectivityType", "myRole"],
        &["NAME", "ID", "CONNECTIVITY", "ROLE"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    validate_uuid(id, "--id")?;
    let data = client
        .get(&format!("/admin/connections/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "admin show-connection", "Fabric administrator"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    validate_uuid(id, "--id")?;
    if output::dry_run_guard(
        cli,
        "admin delete-connection",
        &serde_json::json!({"connectionId": id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("/admin/connections/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "admin delete-connection", "Fabric administrator"))?;
    output::render_object(
        cli,
        &serde_json::json!({"id": id, "status": "deleted"}),
        "status",
    );
    Ok(())
}

pub(super) async fn list_roles(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    validate_uuid(id, "--id")?;
    let response = client
        .get_list(
            &format!("/admin/connections/{id}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "admin list-connection-role-assignments",
                "Fabric administrator",
            )
        })?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["principal.id", "principal.type", "role", "id"],
        &["PRINCIPAL", "TYPE", "ROLE", "ID"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn add_role(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    principal_id: &str,
    principal_type: &str,
) -> Result<()> {
    validate_uuid(id, "--id")?;
    validate_uuid(principal_id, "--principal-id")?;
    let body = serde_json::json!({
        "principal": {"id": principal_id, "type": principal_type},
        "role": "Owner"
    });
    if output::dry_run_guard(cli, "admin add-connection-role-assignment", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/admin/connections/{id}/roleAssignments"),
            &body,
            false,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "admin add-connection-role-assignment",
                "Fabric administrator",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn show_role(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    validate_uuid(id, "--id")?;
    validate_uuid(assignment_id, "--assignment-id")?;
    let data = client
        .get(&format!(
            "/admin/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "admin show-connection-role-assignment",
                "Fabric administrator",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_role(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    validate_uuid(id, "--id")?;
    validate_uuid(assignment_id, "--assignment-id")?;
    let body = serde_json::json!({"role": "Owner"});
    if output::dry_run_guard(cli, "admin update-connection-role-assignment", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/admin/connections/{id}/roleAssignments/{assignment_id}"),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "admin update-connection-role-assignment",
                "Fabric administrator",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_role(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    validate_uuid(id, "--id")?;
    validate_uuid(assignment_id, "--assignment-id")?;
    if output::dry_run_guard(
        cli,
        "admin delete-connection-role-assignment",
        &serde_json::json!({"connectionId": id, "assignmentId": assignment_id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "/admin/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "admin delete-connection-role-assignment",
                "Fabric administrator",
            )
        })?;
    output::render_object(
        cli,
        &serde_json::json!({"id": assignment_id, "status": "deleted"}),
        "status",
    );
    Ok(())
}
