use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating items, run: fabio context schema CopyJob\nReturns the definition template with required fields and format."
)]
pub enum CopyJobCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List copy jobs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a copy job
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,
    },
    /// Create a new copy job
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update copy job properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a copy job
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a copy job
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Reset a copy job (all entities or selected entities)
    #[command(display_order = 8)]
    Reset {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Reset all copy job entities (mutually exclusive with --entity-ids)
        #[arg(long, conflicts_with = "entity_ids")]
        all: bool,

        /// Comma-separated list of entity IDs to reset (mutually exclusive with --all)
        #[arg(long, value_delimiter = ',', conflicts_with = "all")]
        entity_ids: Vec<String>,
    },

    /// Update the definition of a copy job
    #[command(display_order = 9)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Copy job ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline)
        #[arg(long)]
        content: Option<String>,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &CopyJobCommand) -> Result<()> {
    match command {
        CopyJobCommand::List { workspace } => list(cli, client, workspace).await,
        CopyJobCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        CopyJobCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        CopyJobCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        CopyJobCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        CopyJobCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        CopyJobCommand::Reset {
            workspace,
            id,
            all,
            entity_ids,
        } => reset(cli, client, workspace, id, *all, entity_ids).await,
        CopyJobCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/copyJobs"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/copyJobs/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "copy-job create", &body) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/copyJobs"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job create", "Member"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio copy-job update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
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

    if output::dry_run_guard(cli, "copy-job update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/copyJobs/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "copy-job delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/copyJobs/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/copyJobs/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/copyJobs/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                crate::definition_spec::definition_input_hint(
                    "CopyJob",
                    "copy-job",
                    "update-definition",
                ),
            )
            .into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&script, "copyjob-content.json");

    if output::dry_run_guard(
        cli,
        "copy-job update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/copyJobs/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Reset ────────────────────────────────────────────────────────────────────

fn build_reset_body(reset_all: bool, entity_ids: &[String]) -> Result<Value> {
    if !reset_all && entity_ids.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --all or --entity-ids must be provided".to_string(),
            "Example: fabio copy-job reset --workspace <WS> --id <ID> --all".to_string(),
        )
        .into());
    }

    let body = if reset_all {
        serde_json::json!({ "resetAllCopyJobEntities": true })
    } else {
        let entities: Vec<_> = entity_ids
            .iter()
            .map(|eid| serde_json::json!({ "copyJobEntityId": eid }))
            .collect();
        serde_json::json!({
            "resetAllCopyJobEntities": false,
            "copyJobEntitiesToReset": entities
        })
    };

    Ok(body)
}

async fn reset(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    reset_all: bool,
    entity_ids: &[String],
) -> Result<()> {
    let body = build_reset_body(reset_all, entity_ids)?;

    if output::dry_run_guard(cli, "copy-job reset", &body) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/copyJobs/{id}/resetCopyJob"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "copy-job reset", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "reset" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_body_all() {
        let body = build_reset_body(true, &[]).unwrap();
        assert_eq!(body["resetAllCopyJobEntities"], true);
        assert!(body.get("copyJobEntitiesToReset").is_none());
    }

    #[test]
    fn reset_body_specific_entities() {
        let ids = vec!["id-1".to_string(), "id-2".to_string()];
        let body = build_reset_body(false, &ids).unwrap();
        assert_eq!(body["resetAllCopyJobEntities"], false);
        let entities = body["copyJobEntitiesToReset"].as_array().unwrap();
        assert_eq!(entities.len(), 2);
        assert_eq!(entities[0]["copyJobEntityId"], "id-1");
        assert_eq!(entities[1]["copyJobEntityId"], "id-2");
    }

    #[test]
    fn reset_body_no_flags_errors() {
        let result = build_reset_body(false, &[]);
        assert!(result.is_err());
    }
}
