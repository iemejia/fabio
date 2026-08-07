use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

// ─── Apply Tags ──────────────────────────────────────────────────────────────

pub(super) async fn apply_tags(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    tag_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "tags": tag_ids });

    if output::dry_run_guard(cli, "item apply-tags", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/applyTags"),
            &body,
            false,
        )
        .await?;

    let obj = serde_json::json!({ "id": id, "status": "tags_applied" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Unapply Tags ────────────────────────────────────────────────────────────

pub(super) async fn unapply_tags(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    tag_ids: &[String],
) -> Result<()> {
    let body = serde_json::json!({ "tags": tag_ids });

    if output::dry_run_guard(cli, "item unapply-tags", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/unapplyTags"),
            &body,
            false,
        )
        .await?;

    let obj = serde_json::json!({ "id": id, "status": "tags_removed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}
