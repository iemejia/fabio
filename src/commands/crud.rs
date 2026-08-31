//! Shared CRUD helpers for Fabric item-type command groups.
//!
//! Nearly every item-type module (`map`, `plan`, `reflex`, `notebook`, …)
//! exposes the same `list`/`show`/`delete`/`get-definition`/`update-definition`
//! shape whose handler bodies differ only by the workspace **collection**
//! segment (`maps`, `plans`, …), the op-name prefix, the required role, and the
//! definition part filename. These helpers centralize that logic so each module
//! delegates instead of duplicating it. The typed clap enums and per-group
//! dispatch stay in each module (for `--help` and the agent schema).

use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

/// List items in a workspace and render them.
///
/// Fetches `/workspaces/{workspace}/{collection}` (paginated, honoring `--all`
/// and `--continuation-token`) and renders via
/// [`output::render_item_list`], which auto-appends the SENSITIVITY LABEL and
/// TAGS columns when present. `base_columns`/`base_headers` are the
/// type-specific leading columns (usually name/id/description).
pub async fn list(
    cli: &Cli,
    client: &FabricClient,
    collection: &str,
    workspace: &str,
    base_columns: &[&str],
    base_headers: &[&str],
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/{collection}"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_item_list(
        cli,
        &resp.items,
        base_columns,
        base_headers,
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

/// GET a single item by id and render it.
pub async fn show(
    cli: &Cli,
    client: &FabricClient,
    collection: &str,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/{collection}/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Delete an item (soft by default, or permanently with `hard_delete`).
///
/// `group` is the op-name prefix (e.g. `"map"` → `"map delete"`); `role` is the
/// minimum role named in the permission-error hint. Dry-run guarded.
#[allow(clippy::too_many_arguments)]
pub async fn delete(
    cli: &Cli,
    client: &FabricClient,
    group: &str,
    collection: &str,
    role: &str,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    let op = format!("{group} delete");
    if output::dry_run_guard(
        cli,
        &op,
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }
    let url = if hard_delete {
        format!("/workspaces/{workspace}/{collection}/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/{collection}/{id}")
    };
    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, &op, role))?;
    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Fetch an item's definition via `POST .../{id}/getDefinition`.
///
/// With `decode`, base64 definition parts are decoded for readability.
#[allow(clippy::too_many_arguments)]
pub async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    group: &str,
    collection: &str,
    role: &str,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let op = format!("{group} get-definition");
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{collection}/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, &op, role))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}
