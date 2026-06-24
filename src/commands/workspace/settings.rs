use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

use super::read_json_body;

pub(super) async fn apply_tags(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    tag_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "tagIds": tag_ids });
    if output::dry_run_guard(cli, "workspace apply-tags", &body) {
        return Ok(());
    }
    client
        .post(&format!("/workspaces/{workspace}/applyTags"), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "workspace apply-tags", "Admin"))?;
    let obj =
        serde_json::json!({ "workspaceId": workspace, "tagIds": tag_ids, "status": "applied" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn unapply_tags(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    tag_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "tagIds": tag_ids });
    if output::dry_run_guard(cli, "workspace unapply-tags", &body) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/unapplyTags"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace unapply-tags", "Admin"))?;
    let obj =
        serde_json::json!({ "workspaceId": workspace, "tagIds": tag_ids, "status": "unapplied" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn assign_to_domain(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    domain_id: &str,
) -> Result<()> {
    let body = serde_json::json!({ "domainId": domain_id });
    if output::dry_run_guard(cli, "workspace assign-to-domain", &body) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/assignToDomain"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace assign-to-domain", "Admin"))?;
    let obj = serde_json::json!({ "workspaceId": workspace, "domainId": domain_id, "status": "assigned" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn unassign_from_domain(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "workspace unassign-from-domain",
        &serde_json::json!({ "workspaceId": workspace }),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/unassignFromDomain"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace unassign-from-domain", "Admin"))?;
    let obj = serde_json::json!({ "workspaceId": workspace, "status": "unassigned" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn get_onelake_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/onelake/settings"))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-onelake-settings", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn modify_default_tier(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    tier: &str,
) -> Result<()> {
    let body = serde_json::json!({ "defaultTier": tier });
    if output::dry_run_guard(cli, "workspace modify-default-tier", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/onelake/settings/modifyDefaultTier?defaultTier={tier}"
            ),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace modify-default-tier", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn modify_diagnostics(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace modify-diagnostics")?;
    if output::dry_run_guard(cli, "workspace modify-diagnostics", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/onelake/settings/modifyDiagnostics"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace modify-diagnostics", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn modify_immutability_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace modify-immutability-policy")?;
    if output::dry_run_guard(cli, "workspace modify-immutability-policy", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/onelake/settings/modifyImmutabilityPolicy"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace modify-immutability-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn export_lifecycle_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/onelake/lifecycle/exportPolicy"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace export-lifecycle-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn import_lifecycle_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace import-lifecycle-policy")?;
    if output::dry_run_guard(cli, "workspace import-lifecycle-policy", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/onelake/lifecycle/importPolicy"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace import-lifecycle-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn reset_shortcut_cache(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "workspace reset-shortcut-cache",
        &serde_json::json!({ "workspaceId": workspace }),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/onelake/resetShortcutCache"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace reset-shortcut-cache", "Admin"))?;
    let obj = serde_json::json!({ "workspaceId": workspace, "status": "cache_reset" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn set_dataset_storage_format(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    format: &str,
) -> Result<()> {
    let body = serde_json::json!({ "defaultDatasetStorageFormat": format });
    if output::dry_run_guard(cli, "workspace set-dataset-storage-format", &body) {
        return Ok(());
    }
    let data = client
        .patch_powerbi(&format!("/groups/{workspace}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "workspace set-dataset-storage-format", "Admin"))?;
    if data.is_null() {
        let obj = serde_json::json!({ "workspaceId": workspace, "defaultDatasetStorageFormat": format, "status": "updated" });
        output::render_object(cli, &obj, "workspaceId");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

pub(super) async fn get_dataset_storage_format(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get_powerbi(&format!("/groups/{workspace}"))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-dataset-storage-format", "Viewer"))?;
    let format_value = data
        .get("defaultDatasetStorageFormat")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let obj = serde_json::json!({ "workspaceId": workspace, "defaultDatasetStorageFormat": format_value });
    output::render_object(cli, &obj, "workspaceId");
    Ok(())
}

pub(super) async fn get_settings(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}"))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-settings", "Viewer"))?;
    if let Some(props) = data.get("properties") {
        output::render_object(cli, props, "automaticMetadataSync");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

pub(super) async fn update_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace update-settings")?;
    if output::dry_run_guard(cli, "workspace update-settings", &body) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{workspace}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "workspace update-settings", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── CMK Encryption ──────────────────────────────────────────────────────────

pub(super) async fn get_encryption(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/encryption"))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-encryption", "Admin"))?;
    output::render_object(cli, &data, "encryptionDetail");
    Ok(())
}

pub(super) async fn assign_encryption(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    key_identifier: &str,
) -> Result<()> {
    let body = serde_json::json!({ "keyIdentifier": key_identifier });
    if output::dry_run_guard(
        cli,
        "workspace assign-encryption",
        &serde_json::json!({ "workspace": workspace, "keyIdentifier": key_identifier }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/encryption/assign"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace assign-encryption", "Admin"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let result = serde_json::json!({ "workspace": workspace, "status": "assign_in_progress", "keyIdentifier": key_identifier });
        output::render_object(cli, &result, "status");
    } else {
        output::render_object(cli, &data, "encryptionDetail");
    }
    Ok(())
}

pub(super) async fn reset_encryption(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let body = serde_json::json!({});
    if output::dry_run_guard(
        cli,
        "workspace reset-encryption",
        &serde_json::json!({ "workspace": workspace }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/encryption/reset"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace reset-encryption", "Admin"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let result = serde_json::json!({ "workspace": workspace, "status": "reset_complete" });
        output::render_object(cli, &result, "status");
    } else {
        output::render_object(cli, &data, "encryptionDetail");
    }
    Ok(())
}
