use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum OrgAppAudienceCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List org app audiences in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an org app audience
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Org app audience ID
        #[arg(long)]
        id: String,
    },
    /// Create a new org app audience
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Parent org app ID. REQUIRED — an audience belongs to an org app; the
        /// API rejects a create without it (`InvalidParentAppId`).
        #[arg(long, visible_alias = "parent-app-id")]
        org_app_id: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update org app audience properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Org app audience ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an org app audience
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Org app audience ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of an org app audience
    #[command(display_order = 6, name = "get-definition")]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Org app audience ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an org app audience
    #[command(display_order = 7, name = "update-definition")]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Org app audience ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &OrgAppAudienceCommand,
) -> Result<()> {
    match command {
        OrgAppAudienceCommand::List { workspace } => list(cli, client, workspace).await,
        OrgAppAudienceCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        OrgAppAudienceCommand::Create {
            workspace,
            name,
            org_app_id,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                org_app_id,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        OrgAppAudienceCommand::Update {
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
        OrgAppAudienceCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        OrgAppAudienceCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        OrgAppAudienceCommand::UpdateDefinition {
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

fn build_list_url(workspace: &str) -> String {
    format!("/workspaces/{workspace}/orgAppAudiences")
}

fn build_item_url(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/orgAppAudiences/{id}")
}

fn build_delete_url(workspace: &str, id: &str, hard_delete: bool) -> String {
    if hard_delete {
        format!("/workspaces/{workspace}/orgAppAudiences/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/orgAppAudiences/{id}")
    }
}

/// Build the create body. An org-app audience REQUIRES its `parentAppId`, which
/// the API only accepts inside a `definition.json` part (a top-level
/// `parentAppId` is rejected with `InvalidParentAppId`). The definition shape is
/// `{$schema, settings: null, parentAppId}` (verified live).
fn build_create_body(name: &str, org_app_id: &str, description: Option<&str>) -> Value {
    use base64::Engine;
    let definition = serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/orgappaudience/definition/orgAppAudienceDefinition/1.0.0/schema.json",
        "settings": Value::Null,
        "parentAppId": org_app_id,
    });
    let payload = base64::engine::general_purpose::STANDARD
        .encode(serde_json::to_string(&definition).unwrap_or_default());
    let mut body = serde_json::json!({
        "displayName": name,
        "definition": {
            "parts": [
                { "path": "definition.json", "payload": payload, "payloadType": "InlineBase64" }
            ]
        }
    });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    body
}

fn build_update_body(name: Option<&str>, description: Option<&str>) -> Result<Value> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio org-app-audience update --workspace <WS> --id <ID> --name \"New Name\""
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
    Ok(body)
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &build_list_url(workspace),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client.get(&build_item_url(workspace, id)).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    org_app_id: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = build_create_body(name, org_app_id, description);
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(cli, "org-app-audience create", &body) {
        return Ok(());
    }

    let data = client
        .post(&build_list_url(workspace), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "org-app-audience create", "Contributor"))?;
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
    let body = build_update_body(name, description)?;

    if output::dry_run_guard(cli, "org-app-audience update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&build_item_url(workspace, id), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "org-app-audience update", "Contributor"))?;
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
        "org-app-audience delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = build_delete_url(workspace, id, hard_delete);

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "org-app-audience delete", "Contributor"))?;

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
            &format!("/workspaces/{workspace}/orgAppAudiences/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "org-app-audience get-definition", "Contributor"))?;

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
    crate::commands::crud::update_definition(
        cli,
        client,
        "org-app-audience",
        "orgAppAudiences",
        "Contributor",
        "definition.json",
        workspace,
        id,
        file,
        content,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_url_construction() {
        assert_eq!(build_list_url("ws-1"), "/workspaces/ws-1/orgAppAudiences");
    }

    #[test]
    fn item_url_construction() {
        assert_eq!(
            build_item_url("ws-1", "id-2"),
            "/workspaces/ws-1/orgAppAudiences/id-2"
        );
    }

    #[test]
    fn delete_url_without_hard_delete() {
        let url = build_delete_url("ws-1", "id-2", false);
        assert!(!url.contains("hardDelete"));
    }

    #[test]
    fn delete_url_with_hard_delete() {
        let url = build_delete_url("ws-1", "id-2", true);
        assert!(url.contains("hardDelete=true"));
    }

    #[test]
    fn create_body_with_description() {
        let body = build_create_body("Aud", "app-1", Some("desc"));
        assert_eq!(body["displayName"], "Aud");
        assert_eq!(body["description"], "desc");
        // parentAppId must be inside the definition.json part (not top-level).
        assert!(body.get("parentAppId").is_none());
        let part = &body["definition"]["parts"][0];
        assert_eq!(part["path"], "definition.json");
        let decoded = {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(part["payload"].as_str().unwrap())
                .unwrap();
            serde_json::from_slice::<Value>(&bytes).unwrap()
        };
        assert_eq!(decoded["parentAppId"], "app-1");
        assert!(decoded["settings"].is_null());
    }

    #[test]
    fn create_body_without_description() {
        let body = build_create_body("Aud", "app-1", None);
        assert_eq!(body["displayName"], "Aud");
        assert!(body.get("description").is_none());
        assert_eq!(body["definition"]["parts"][0]["path"], "definition.json");
    }

    #[test]
    fn update_body_validates_at_least_one_field() {
        assert!(build_update_body(None, None).is_err());
        assert!(build_update_body(Some("x"), None).is_ok());
        assert!(build_update_body(None, Some("y")).is_ok());
    }
}
