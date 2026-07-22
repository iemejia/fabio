//! Workspace relation commands (Git branch/base relations).
//!
//! Implements the (Preview) `WorkspaceRelations` API:
//! <https://learn.microsoft.com/rest/api/fabric/core/git-integration>
//!
//! A workspace relation links a branch workspace to its base workspace
//! (`relationType: Base`/`Branch`) or expresses a general relation between
//! two workspaces (`relationType: RelatedWorkspace`, server-assigned only —
//! `RelatedWorkspace` cannot be created via this API, only `Base`/`Branch`).

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::enrich_forbidden;
use crate::output;

#[derive(Debug, Subcommand)]
pub enum RelationCommand {
    /// List workspace relations for a workspace (Preview)
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Create a workspace relation between two workspaces (Preview)
    ///
    /// The caller must have an *admin* role on the branch workspace and a
    /// *contributor* or higher role on the base workspace.
    Create {
        /// Workspace ID (the branch or base workspace, depending on relation type)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Related workspace ID
        #[arg(long)]
        related_workspace: String,

        /// Relation type to create (only `base` and `branch` are valid for creation)
        #[arg(long, value_parser = ["base", "branch"])]
        relation_type: String,
    },
    /// Delete a workspace relation (Preview)
    Delete {
        /// Workspace ID (either the base or branch workspace in the relation)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Workspace relation ID
        #[arg(long)]
        relation_id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &RelationCommand) -> Result<()> {
    match command {
        RelationCommand::List { workspace } => list(cli, client, workspace).await,
        RelationCommand::Create {
            workspace,
            related_workspace,
            relation_type,
        } => create(cli, client, workspace, related_workspace, relation_type).await,
        RelationCommand::Delete {
            workspace,
            relation_id,
        } => delete(cli, client, workspace, relation_id).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/git/workspaceRelations"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git relation list", "Viewer"))?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "relatedWorkspaceId", "relationType"],
        &["ID", "RELATED WORKSPACE", "TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    related_workspace: &str,
    relation_type: &str,
) -> Result<()> {
    let api_relation_type = map_relation_type(relation_type);

    let body = serde_json::json!({
        "relatedWorkspaceId": related_workspace,
        "relationType": api_relation_type,
    });

    if output::dry_run_guard(cli, "git relation create", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/git/workspaceRelations"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "git relation create", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    relation_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "git relation delete",
        &serde_json::json!({ "workspaceId": workspace, "workspaceRelationId": relation_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/git/workspaceRelations/{relation_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "git relation delete", "Admin"))?;

    let obj = serde_json::json!({ "id": relation_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Maps the CLI's lowercase `--relation-type` values to the API's `PascalCase`
/// enum values. Only `Base` and `Branch` are valid for creation; any other
/// value is passed through unchanged (defensive — clap's `value_parser`
/// already restricts input to "base"/"branch").
fn map_relation_type(relation_type: &str) -> &str {
    match relation_type {
        "base" => "Base",
        "branch" => "Branch",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relation_type_mapping_maps_lowercase_to_pascalcase() {
        assert_eq!(map_relation_type("base"), "Base");
        assert_eq!(map_relation_type("branch"), "Branch");
    }

    #[test]
    fn delete_response_shape() {
        let obj = serde_json::json!({ "id": "rel-1", "status": "deleted" });
        assert_eq!(obj["status"], "deleted");
        assert_eq!(obj["id"], "rel-1");
    }
}
