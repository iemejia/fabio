use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_trusted_url};
use crate::errors::enrich_forbidden;
use crate::output;

const COLLECTION: &str = "shareSubscriptions";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum ShareSubscriptionCommand {
    /// List Share Subscriptions in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list subscriptions directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a Share Subscription
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share Subscription ID
        #[arg(long)]
        id: String,
    },
    /// Create a Share Subscription from an invitation
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// `IQShare` invitation URL
        #[arg(long)]
        invitation_url: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
        /// Folder ID (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,
        /// Sensitivity label ID
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Sensitivity label application strategy
        #[arg(
            long,
            requires = "sensitivity_label",
            value_parser = ["ApplyOrFail", "Ignore"]
        )]
        sensitivity_label_apply_strategy: Option<String>,
    },
    /// Update Share Subscription properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share Subscription ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Share Subscription
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share Subscription ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &ShareSubscriptionCommand,
) -> Result<()> {
    match command {
        ShareSubscriptionCommand::List {
            workspace,
            no_recursive,
            root_folder_id,
        } => {
            list(
                cli,
                client,
                workspace,
                *no_recursive,
                root_folder_id.as_deref(),
            )
            .await
        }
        ShareSubscriptionCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        ShareSubscriptionCommand::Create {
            workspace,
            name,
            invitation_url,
            description,
            folder_id,
            sensitivity_label,
            sensitivity_label_apply_strategy,
        } => {
            let args = CreateArgs {
                workspace,
                name,
                invitation_url,
                description: description.as_deref(),
                folder_id: folder_id.as_deref(),
                sensitivity_label: sensitivity_label.as_deref(),
                sensitivity_label_apply_strategy: sensitivity_label_apply_strategy.as_deref(),
            };
            create(cli, client, &args).await
        }
        ShareSubscriptionCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crate::commands::crud::update(
                cli,
                client,
                "share-subscription",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        ShareSubscriptionCommand::Delete { workspace, id } => {
            delete(cli, client, workspace, id).await
        }
    }
}

fn list_path(workspace: &str, no_recursive: bool, root_folder_id: Option<&str>) -> String {
    let mut path = format!("/workspaces/{workspace}/{COLLECTION}");
    let mut params = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder) = root_folder_id {
        params.push(format!("rootFolderId={folder}"));
    }
    if !params.is_empty() {
        path.push('?');
        path.push_str(&params.join("&"));
    }
    path
}

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    let response = client
        .get_list(
            &list_path(workspace, no_recursive, root_folder_id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_item_list(
        cli,
        &response.items,
        &["displayName", "id", "description", "properties.status"],
        &["NAME", "ID", "DESCRIPTION", "STATUS"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

struct CreateArgs<'a> {
    workspace: &'a str,
    name: &'a str,
    invitation_url: &'a str,
    description: Option<&'a str>,
    folder_id: Option<&'a str>,
    sensitivity_label: Option<&'a str>,
    sensitivity_label_apply_strategy: Option<&'a str>,
}

fn build_create_body(args: &CreateArgs<'_>) -> Value {
    let mut body = json!({
        "displayName": args.name,
        "creationPayload": {
            "invitationUrl": args.invitation_url
        }
    });
    if let Some(value) = args.description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = args.folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = args.sensitivity_label {
        let mut settings = json!({ "labelId": value });
        if let Some(strategy) = args.sensitivity_label_apply_strategy {
            settings["sensitivityLabelApplyStrategy"] = Value::from(strategy);
        }
        body["sensitivityLabelSettings"] = settings;
    }
    body
}

async fn create(cli: &Cli, client: &FabricClient, args: &CreateArgs<'_>) -> Result<()> {
    validate_trusted_url(args.invitation_url, "--invitation-url")?;
    let body = build_create_body(args);
    if output::dry_run_guard(
        cli,
        "share-subscription create",
        &json!({ "workspace": args.workspace, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{}/{COLLECTION}", args.workspace),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "share-subscription create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "share-subscription delete",
        &json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!("/workspaces/{workspace}/{COLLECTION}/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "share-subscription delete", "Contributor"))?;
    output::render_object(cli, &json!({ "id": id, "status": "deleted" }), "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_body_serializes_required_creation_payload_and_optional_fields() {
        let args = CreateArgs {
            workspace: "workspace",
            name: "Subscription",
            invitation_url: "https://app.fabric.microsoft.com/share/invite/example",
            description: Some("Description"),
            folder_id: Some("folder"),
            sensitivity_label: Some("label"),
            sensitivity_label_apply_strategy: Some("Ignore"),
        };
        let body = build_create_body(&args);
        assert_eq!(
            body["creationPayload"]["invitationUrl"],
            args.invitation_url
        );
        assert_eq!(body["folderId"], "folder");
        assert_eq!(body["sensitivityLabelSettings"]["labelId"], "label");
        assert_eq!(
            body["sensitivityLabelSettings"]["sensitivityLabelApplyStrategy"],
            "Ignore"
        );
    }

    #[test]
    fn list_path_serializes_folder_query_parameters() {
        assert_eq!(
            list_path("ws", true, Some("folder")),
            "/workspaces/ws/shareSubscriptions?recursive=false&rootFolderId=folder"
        );
    }
}
