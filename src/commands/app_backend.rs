use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
pub enum AppBackendCommand {
    /// List app backends in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,
    },
    /// Show details of an app backend
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// App backend ID
        #[arg(long)]
        id: String,
    },
    /// Create a new app backend
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// App backend display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,
    },
    /// Update app backend properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// App backend ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an app backend
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long)]
        workspace: String,

        /// App backend ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &AppBackendCommand) -> Result<()> {
    match command {
        AppBackendCommand::List { workspace } => list(cli, client, workspace).await,
        AppBackendCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        AppBackendCommand::Create {
            workspace,
            name,
            description,
        } => create(cli, client, workspace, name, description.as_deref()).await,
        AppBackendCommand::Update {
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
        AppBackendCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/appBackends"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

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

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/appBackends/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "app-backend show", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
    if let Some(desc) = description {
        body["description"] = Value::String(desc.to_string());
    }

    if output::dry_run_guard(
        cli,
        "app-backend create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/appBackends"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "app-backend create", "Contributor"))?;
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
            "Example: fabio app-backend update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::String(n.to_string());
    }
    if let Some(d) = description {
        body["description"] = Value::String(d.to_string());
    }

    if output::dry_run_guard(cli, "app-backend update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/appBackends/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "app-backend update", "Contributor"))?;
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
        "app-backend delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/appBackends/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/appBackends/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "app-backend delete", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_url_construction() {
        let ws = "cfafbeb1-8037-4d0c-896e-a46fb27ff229";
        let url = format!("/workspaces/{ws}/appBackends");
        assert!(url.contains(ws));
        assert!(url.ends_with("/appBackends"));
    }

    #[test]
    fn show_url_construction() {
        let ws = "ws-123";
        let id = "ab-456";
        let url = format!("/workspaces/{ws}/appBackends/{id}");
        assert_eq!(url, "/workspaces/ws-123/appBackends/ab-456");
    }

    #[test]
    fn delete_url_with_hard_delete() {
        let ws = "ws-123";
        let id = "ab-456";
        let url_soft = format!("/workspaces/{ws}/appBackends/{id}");
        let url_hard = format!("/workspaces/{ws}/appBackends/{id}?hardDelete=true");
        assert!(!url_soft.contains("hardDelete"));
        assert!(url_hard.contains("hardDelete=true"));
    }

    #[test]
    fn create_body_with_description() {
        let mut body = serde_json::json!({"displayName": "MyBackend"});
        body["description"] = Value::String("A backend".to_string());
        assert_eq!(body["displayName"], "MyBackend");
        assert_eq!(body["description"], "A backend");
    }

    #[test]
    fn create_body_without_description() {
        let body = serde_json::json!({"displayName": "MyBackend"});
        assert_eq!(body["displayName"], "MyBackend");
        assert!(body.get("description").is_none());
    }

    #[test]
    fn update_body_name_only() {
        let mut body = serde_json::json!({});
        body["displayName"] = Value::String("New Name".to_string());
        assert_eq!(body["displayName"], "New Name");
        assert!(body.get("description").is_none());
    }

    #[test]
    fn update_body_description_only() {
        let mut body = serde_json::json!({});
        body["description"] = Value::String("New Desc".to_string());
        assert!(body.get("displayName").is_none());
        assert_eq!(body["description"], "New Desc");
    }

    #[test]
    fn update_body_both_fields() {
        let mut body = serde_json::json!({});
        body["displayName"] = Value::String("Name".to_string());
        body["description"] = Value::String("Desc".to_string());
        assert_eq!(body["displayName"], "Name");
        assert_eq!(body["description"], "Desc");
    }
}
