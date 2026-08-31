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

/// Replace an item's definition via `POST .../{id}/updateDefinition`.
///
/// Reads the payload from `file` or inline `content`, wraps it in a single
/// definition part named `part_filename` (e.g. `"map.json"`,
/// `"notebook-content.py"`), and posts it. Dry-run guarded.
#[allow(clippy::too_many_arguments)]
pub async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    group: &str,
    collection: &str,
    role: &str,
    part_filename: &str,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let op = format!("{group} update-definition");
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                format!(
                    "Example: fabio {group} update-definition --workspace <WS> --id <ID> --file definition.json"
                ),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, part_filename);
    if output::dry_run_guard(
        cli,
        &op,
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{collection}/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, &op, role))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// Create an item with the common `{displayName, description?,
/// sensitivityLabelSettings?}` body shared by most item types.
///
/// `group` is the op-name prefix (e.g. `"map"` → `"map create"`). The dry-run
/// preview mirrors the historical per-module shape
/// (`{workspace, displayName, description, sensitivityLabel}`).
#[allow(clippy::too_many_arguments)]
pub async fn create(
    cli: &Cli,
    client: &FabricClient,
    group: &str,
    collection: &str,
    role: &str,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = serde_json::Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({ "sensitivityLabelId": label_id });
    }

    let op = format!("{group} create");
    if output::dry_run_guard(
        cli,
        &op,
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label,
        }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{collection}"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, &op, role))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Update an item's `displayName`/`description` (the common metadata patch).
///
/// Requires at least one of `name`/`description`; the "neither provided" error
/// hint is built from `group` to match the historical per-module message.
#[allow(clippy::too_many_arguments)]
pub async fn update(
    cli: &Cli,
    client: &FabricClient,
    group: &str,
    collection: &str,
    role: &str,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            format!("Example: fabio {group} update --workspace <WS> --id <ID> --name \"New Name\""),
        )
        .into());
    }
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = serde_json::Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = serde_json::Value::from(d);
    }
    let op = format!("{group} update");
    if output::dry_run_guard(cli, &op, &body) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{workspace}/{collection}/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, &op, role))?;
    output::render_object(cli, &data, "id");
    Ok(())
}
