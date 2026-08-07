use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

pub(super) async fn list_restore_points(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/warehouses/{id}/restorePoints"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse list-restore-points", "Viewer"))?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "creationMode", "description"],
        &["DISPLAY NAME", "ID", "CREATION MODE", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn create_restore_point(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    // The API's CreateRestorePointRequest is {displayName, description} — NOT
    // {restorePointLabel} (that field is silently ignored, leaving every point
    // named "Restore point").
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "warehouse create-restore-point", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/warehouses/{id}/restorePoints"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse create-restore-point", "Contributor"))?;

    // A 201 returns the RestorePoint; a 202 (async) may leave an empty LRO body.
    // If we didn't get a restore point back, re-fetch the list and return the
    // most-recent restore point (ids are creation timestamps, so the max id is
    // the one just created) — so agents always get an id to reference.
    if data.get("id").and_then(Value::as_str).is_some() {
        output::render_object(cli, &data, "id");
        return Ok(());
    }
    let listed = client
        .get_list(
            &format!("/workspaces/{workspace}/warehouses/{id}/restorePoints"),
            "value",
            true,
            None,
        )
        .await;
    if let Ok(resp) = listed
        && let Some(newest) = resp.items.iter().max_by_key(|rp| {
            rp.get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned()
        })
    {
        output::render_object(cli, newest, "id");
    } else {
        output::render_object(cli, &serde_json::json!({ "status": "created" }), "status");
    }
    Ok(())
}

pub(super) async fn show_restore_point(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    restore_point_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/warehouses/{id}/restorePoints/{restore_point_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse show-restore-point", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_restore_point(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    restore_point_id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    // UpdateRestorePointRequest is {displayName, description} — NOT
    // {restorePointLabel} (silently ignored).
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    if output::dry_run_guard(cli, "warehouse update-restore-point", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/warehouses/{id}/restorePoints/{restore_point_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse update-restore-point", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_restore_point(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    restore_point_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "warehouse delete-restore-point",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "restorePointId": restore_point_id
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/warehouses/{id}/restorePoints/{restore_point_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse delete-restore-point", "Contributor"))?;

    let obj = serde_json::json!({ "id": restore_point_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn restore_to_point(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    restore_point_id: &str,
) -> Result<()> {
    // Restore-in-place: the API takes NO request body (only the restore point id
    // in the path). fabio previously required a bogus `--name` and sent a
    // `{restoreToWarehouseName}` body the server ignores.
    let body = serde_json::json!({});

    if output::dry_run_guard(
        cli,
        "warehouse restore-to-point",
        &serde_json::json!({ "id": id, "restorePointId": restore_point_id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/warehouses/{id}/restorePoints/{restore_point_id}/restore"
            ),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "warehouse restore-to-point", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        output::render_object(
            cli,
            &serde_json::json!({ "id": id, "restorePointId": restore_point_id, "status": "restored" }),
            "status",
        );
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}
