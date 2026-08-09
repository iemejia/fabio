use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_admin};
use crate::output;

use super::read_body;

/// Build the bulk-create-tags request body from a list of display names. The
/// Fabric API field is `createTagsRequest` (an array of `{displayName}`) — a
/// non-obvious name (NOT `createTagRequests`), so this convenience builder
/// spares callers from getting the raw shape wrong.
fn build_create_tags_body(names: &[String]) -> Value {
    serde_json::json!({
        "createTagsRequest": names
            .iter()
            .map(|n| serde_json::json!({ "displayName": n }))
            .collect::<Vec<_>>(),
    })
}

pub(super) async fn list_tags(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/admin/tags",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "displayName", "description"],
        &["ID", "NAME", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn create_tags(
    cli: &Cli,
    client: &FabricClient,
    names: &[String],
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    // Convenience --name flags take precedence and build the correct body; fall
    // back to the raw --file/--content for full control.
    let body = if names.is_empty() {
        read_body(file, content, "create-tags")?
    } else {
        if file.is_some() || content.is_some() {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--name cannot be combined with --file/--content".to_string(),
                "Use repeatable --name for the simple case, or --file/--content for a raw body."
                    .to_string(),
            )
            .into());
        }
        build_create_tags_body(names)
    };

    if output::dry_run_guard(cli, "admin create-tags", &body) {
        return Ok(());
    }

    let data = client
        .post("/admin/tags/bulkCreateTags", &body, false)
        .await
        .map_err(|e| enrich_admin(e, "admin create-tags"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update_tag(
    cli: &Cli,
    client: &FabricClient,
    tag_id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio admin update-tag --tag-id <ID> --name \"New Name\"".to_string(),
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

    if output::dry_run_guard(cli, "admin update-tag", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/admin/tags/{tag_id}"), &body)
        .await
        .map_err(|e| enrich_admin(e, "admin update-tag"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_tag(cli: &Cli, client: &FabricClient, tag_id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "admin delete-tag",
        &serde_json::json!({ "tagId": tag_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/admin/tags/{tag_id}"))
        .await
        .map_err(|e| enrich_admin(e, "admin delete-tag"))?;

    let obj = serde_json::json!({ "tagId": tag_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_create_tags_body;

    #[test]
    fn build_create_tags_body_uses_the_correct_field() {
        let body = build_create_tags_body(&["Finance 2024".to_string(), "HR 2024".to_string()]);
        // The API field is `createTagsRequest` (NOT `createTagRequests`).
        let arr = body["createTagsRequest"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["displayName"], "Finance 2024");
        assert_eq!(arr[1]["displayName"], "HR 2024");
        assert!(body.get("createTagRequests").is_none());
    }
}
