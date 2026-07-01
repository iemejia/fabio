use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

use super::read_json_body;

// ─── Materialized Lake View Execution Definitions ───────────────────────────

fn definitions_path(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/lakehouses/{id}/mlvexecutiondefinitions")
}

fn definition_path(workspace: &str, id: &str, execution_definition_id: &str) -> String {
    format!(
        "/workspaces/{workspace}/lakehouses/{id}/mlvexecutiondefinitions/{execution_definition_id}"
    )
}

pub(super) async fn list_execution_definitions(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &definitions_path(workspace, id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse list-execution-definitions", "Viewer"))?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show_execution_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    execution_definition_id: &str,
) -> Result<()> {
    let data = client
        .get(&definition_path(workspace, id, execution_definition_id))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse show-execution-definition", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn create_execution_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "create-execution-definition")?;

    if output::dry_run_guard(cli, "lakehouse create-execution-definition", &body) {
        return Ok(());
    }

    let data = client
        .post(&definitions_path(workspace, id), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse create-execution-definition", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_execution_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    execution_definition_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-execution-definition")?;

    if output::dry_run_guard(cli, "lakehouse update-execution-definition", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &definition_path(workspace, id, execution_definition_id),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse update-execution-definition", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_execution_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    execution_definition_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "lakehouse delete-execution-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "executionDefinitionId": execution_definition_id
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&definition_path(workspace, id, execution_definition_id))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse delete-execution-definition", "Contributor"))?;

    let obj = serde_json::json!({ "id": execution_definition_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{definition_path, definitions_path};

    #[test]
    fn definitions_path_builds_collection_url() {
        assert_eq!(
            definitions_path("ws-1", "lh-1"),
            "/workspaces/ws-1/lakehouses/lh-1/mlvexecutiondefinitions"
        );
    }

    #[test]
    fn definition_path_builds_item_url() {
        assert_eq!(
            definition_path("ws-1", "lh-1", "def-1"),
            "/workspaces/ws-1/lakehouses/lh-1/mlvexecutiondefinitions/def-1"
        );
    }
}
