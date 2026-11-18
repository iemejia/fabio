use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

const COLLECTION: &str = "playbooks";
const PART: &str = "PlaybookV1.json";

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum SectionRole {
    User,
    Curator,
}

impl SectionRole {
    const fn as_str(self) -> &'static str {
        match self {
            Self::User => "User",
            Self::Curator => "Curator",
        }
    }
}

#[derive(Debug, clap::Args)]
pub struct ResourceArgs {
    #[arg(short, long, env = "FABIO_WORKSPACE")]
    workspace: String,
    #[arg(long)]
    id: String,
    /// Complete logical resource path rooted at the Playbook display name
    #[arg(long)]
    path: String,
}

#[derive(Debug, clap::Args)]
pub struct ResourcePutArgs {
    #[command(flatten)]
    resource: ResourceArgs,
    /// Description; omission resets it to an empty string
    #[arg(long, conflicts_with = "rename_source")]
    description: Option<String>,
    /// Existing logical path when renaming
    #[arg(long)]
    rename_source: Option<String>,
}

#[derive(Debug, clap::Args)]
pub struct ResourceDeleteArgs {
    #[command(flatten)]
    resource: ResourceArgs,
    /// Delete all descendants
    #[arg(long)]
    recursive: bool,
}

#[derive(Debug, clap::Args)]
pub struct RoleAssignmentArgs {
    #[command(flatten)]
    resource: ResourceArgs,
    #[arg(long)]
    principal_id: String,
    #[arg(long, value_parser = ["User", "Group", "ServicePrincipal"])]
    principal_type: String,
    #[arg(long, value_enum)]
    role: SectionRole,
}

#[derive(Debug, Subcommand)]
pub enum PlaybookCommand {
    /// List Playbooks in a workspace
    List {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show a Playbook
    Show {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
    },
    /// Create a Playbook
    Create {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        folder_id: Option<String>,
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// `PlaybookV1` definition content (inline or @file)
        #[arg(long)]
        definition: Option<String>,
    },
    /// Update Playbook metadata
    Update {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Playbook
    Delete {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
    },
    /// Get a Playbook definition
    GetDefinition {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        decode: bool,
    },
    /// Replace a Playbook definition
    UpdateDefinition {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
    },
    /// Create, update, or rename a section
    PutSection(ResourcePutArgs),
    /// Get a section
    GetSection(ResourceArgs),
    /// Delete a section
    DeleteSection(ResourceDeleteArgs),
    /// List groups in a section context root
    ListGroups(ResourceArgs),
    /// Create, update, or rename a group
    PutGroup(ResourcePutArgs),
    /// Get a group
    GetGroup(ResourceArgs),
    /// Delete a group
    DeleteGroup(ResourceDeleteArgs),
    /// List direct section role assignments
    ListSectionRoleAssignments(ResourceArgs),
    /// Add a direct section role assignment
    AddSectionRoleAssignment(RoleAssignmentArgs),
    /// Update a direct section role assignment
    UpdateSectionRoleAssignment {
        #[command(flatten)]
        resource: ResourceArgs,
        #[arg(long)]
        principal_id: String,
        #[arg(long, value_enum)]
        role: SectionRole,
    },
    /// Delete a direct section role assignment
    DeleteSectionRoleAssignment {
        #[command(flatten)]
        resource: ResourceArgs,
        #[arg(long)]
        principal_id: String,
    },
    /// Create or replace a context
    PutContext {
        #[command(flatten)]
        resource: ResourceArgs,
        /// Non-empty summary, maximum 2,048 UTF-8 bytes
        #[arg(long)]
        summary: String,
        #[arg(long, value_parser = ["application/zip", "text/markdown", "text/plain"], requires = "data")]
        content_type: Option<String>,
        /// Base64-encoded content
        #[arg(long, requires = "content_type", conflicts_with = "reference")]
        data: Option<String>,
        /// Polymorphic `ContextReference` JSON (inline or @file)
        #[arg(long, conflicts_with_all = ["content_type", "data"])]
        reference: Option<String>,
    },
    /// Get a context
    GetContext {
        #[command(flatten)]
        resource: ResourceArgs,
        #[arg(long)]
        include_content: bool,
    },
    /// Update only a context summary
    UpdateContext {
        #[command(flatten)]
        resource: ResourceArgs,
        #[arg(long)]
        summary: String,
    },
    /// Delete a context
    DeleteContext(ResourceArgs),
    /// Download a context's stored ZIP package
    GetContextContent {
        #[command(flatten)]
        resource: ResourceArgs,
        #[arg(long)]
        output_file: String,
    },
    /// Get the calling identity's Playbook preference
    GetUserPreference {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
    },
    /// Update the calling identity's Playbook preference
    UpdateUserPreference {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long, action = clap::ArgAction::Set)]
        active: bool,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &PlaybookCommand) -> Result<()> {
    match command {
        PlaybookCommand::List { workspace } => {
            crate::commands::crud::list(
                cli,
                client,
                COLLECTION,
                workspace,
                &["displayName", "id", "description"],
                &["NAME", "ID", "DESCRIPTION"],
            )
            .await
        }
        PlaybookCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        PlaybookCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            sensitivity_label,
            definition,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                folder_id.as_deref(),
                sensitivity_label.as_deref(),
                definition.as_deref(),
            )
            .await
        }
        PlaybookCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crate::commands::crud::update(
                cli,
                client,
                "playbook",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        PlaybookCommand::Delete { workspace, id } => {
            crate::commands::crud::delete(
                cli,
                client,
                "playbook",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                false,
            )
            .await
        }
        PlaybookCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => {
            crate::commands::crud::get_definition(
                cli, client, "playbook", COLLECTION, "Viewer", workspace, id, *decode,
            )
            .await
        }
        PlaybookCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            crate::commands::crud::update_definition_with_options(
                cli,
                client,
                "playbook",
                COLLECTION,
                "Contributor",
                PART,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                Some("PlaybookV1"),
                false,
            )
            .await
        }
        PlaybookCommand::PutSection(args) => put_resource(cli, client, args, "section").await,
        PlaybookCommand::GetSection(args) => get_resource(cli, client, args, "section").await,
        PlaybookCommand::DeleteSection(args) => {
            delete_resource(cli, client, args, "section", "playbook delete-section").await
        }
        PlaybookCommand::ListGroups(args) => list_groups(cli, client, args).await,
        PlaybookCommand::PutGroup(args) => put_resource(cli, client, args, "group").await,
        PlaybookCommand::GetGroup(args) => get_resource(cli, client, args, "group").await,
        PlaybookCommand::DeleteGroup(args) => {
            delete_resource(cli, client, args, "group", "playbook delete-group").await
        }
        PlaybookCommand::ListSectionRoleAssignments(args) => {
            list_role_assignments(cli, client, args).await
        }
        PlaybookCommand::AddSectionRoleAssignment(args) => {
            add_role_assignment(cli, client, args).await
        }
        PlaybookCommand::UpdateSectionRoleAssignment {
            resource,
            principal_id,
            role,
        } => update_role_assignment(cli, client, resource, principal_id, *role).await,
        PlaybookCommand::DeleteSectionRoleAssignment {
            resource,
            principal_id,
        } => delete_role_assignment(cli, client, resource, principal_id).await,
        PlaybookCommand::PutContext {
            resource,
            summary,
            content_type,
            data,
            reference,
        } => {
            put_context(
                cli,
                client,
                resource,
                summary,
                content_type.as_deref(),
                data.as_deref(),
                reference.as_deref(),
            )
            .await
        }
        PlaybookCommand::GetContext {
            resource,
            include_content,
        } => get_context(cli, client, resource, *include_content).await,
        PlaybookCommand::UpdateContext { resource, summary } => {
            mutate_context(cli, client, resource, false, json!({ "summary": summary })).await
        }
        PlaybookCommand::DeleteContext(resource) => delete_context(cli, client, resource).await,
        PlaybookCommand::GetContextContent {
            resource,
            output_file,
        } => get_context_content(cli, client, resource, output_file).await,
        PlaybookCommand::GetUserPreference { workspace, id } => {
            let data = client
                .get(&format!(
                    "/workspaces/{workspace}/{COLLECTION}/{id}/userPreference"
                ))
                .await?;
            output::render_object(cli, &data, "active");
            Ok(())
        }
        PlaybookCommand::UpdateUserPreference {
            workspace,
            id,
            active,
        } => {
            let body = json!({ "active": active });
            if output::dry_run_guard(
                cli,
                "playbook update-user-preference",
                &json!({ "workspace": workspace, "id": id, "request": body }),
            ) {
                return Ok(());
            }
            let data = client
                .patch(
                    &format!("/workspaces/{workspace}/{COLLECTION}/{id}/userPreference"),
                    &body,
                )
                .await?;
            output::render_object(cli, &data, "active");
            Ok(())
        }
    }
}

fn encode_logical_path(path: &str) -> String {
    path.split('/')
        .map(urlencoding::encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn resource_path(args: &ResourceArgs, kind: &str) -> String {
    format!(
        "/workspaces/{}/{COLLECTION}/{}/sections/{}?resource={kind}",
        args.workspace,
        args.id,
        encode_logical_path(&args.path)
    )
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    sensitivity_label: Option<&str>,
    definition: Option<&str>,
) -> Result<()> {
    let mut body = json!({ "displayName": name });
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = folder_id {
        validate_uuid(value, "--folder-id")?;
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = sensitivity_label {
        body["sensitivityLabelSettings"] = json!({ "labelId": value });
    }
    if let Some(value) = definition {
        let raw = crate::commands::query_input::resolve_query_input(
            Some(value),
            "Playbook definition",
            "--definition",
            "{}",
        )?;
        body["definition"] =
            crate::definition_spec::build_update_definition_body(&raw, PART)["definition"].clone();
        body["definition"]["format"] = Value::from("PlaybookV1");
    }
    if output::dry_run_guard(
        cli,
        "playbook create",
        &json!({ "workspace": workspace, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{COLLECTION}"),
            &body,
            true,
        )
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn put_resource(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourcePutArgs,
    kind: &str,
) -> Result<()> {
    let body = json!({ "description": args.description.as_deref().unwrap_or_default() });
    let operation = format!("playbook put-{kind}");
    if output::dry_run_guard(
        cli,
        &operation,
        &json!({ "workspace": args.resource.workspace, "id": args.resource.id, "path": args.resource.path, "renameSource": args.rename_source, "request": body }),
    ) {
        return Ok(());
    }
    let path = resource_path(&args.resource, kind);
    let data = if let Some(source) = &args.rename_source {
        client
            .put_with_header(
                &path,
                None,
                "x-ms-rename-source",
                &encode_logical_path(source),
            )
            .await?
    } else {
        client.put(&path, &body).await?
    };
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn get_resource(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    kind: &str,
) -> Result<()> {
    let data = client.get(&resource_path(args, kind)).await?;
    output::render_object(cli, &data, "description");
    Ok(())
}

async fn list_groups(cli: &Cli, client: &FabricClient, args: &ResourceArgs) -> Result<()> {
    let response = client
        .get_list(
            &resource_path(args, "contextRoot"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["name", "path", "description"],
        &["NAME", "PATH", "DESCRIPTION"],
        "path",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn delete_resource(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceDeleteArgs,
    kind: &str,
    operation: &str,
) -> Result<()> {
    if args.resource.path.trim_matches('/').is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "A root or empty Playbook path cannot be deleted.",
            "Provide the complete path of one section or group.",
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        operation,
        &json!({ "workspace": args.resource.workspace, "id": args.resource.id, "path": args.resource.path, "recursive": args.recursive }),
    ) {
        return Ok(());
    }
    let mut path = resource_path(&args.resource, kind);
    if args.recursive {
        path.push_str("&recursive=true");
    }
    client.delete(&path).await?;
    output::render_object(
        cli,
        &json!({ "status": "deleted", "path": args.resource.path }),
        "status",
    );
    Ok(())
}

fn role_path(args: &ResourceArgs) -> String {
    format!(
        "/workspaces/{}/{COLLECTION}/{}/sections/roleAssignments/{}",
        args.workspace,
        args.id,
        encode_logical_path(&args.path)
    )
}

async fn list_role_assignments(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
) -> Result<()> {
    let response = client
        .get_list(
            &role_path(args),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["principal.id", "principal.type", "role"],
        &["PRINCIPAL", "TYPE", "ROLE"],
        "principal.id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn add_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    args: &RoleAssignmentArgs,
) -> Result<()> {
    validate_uuid(&args.principal_id, "--principal-id")?;
    let body = json!({
        "principal": { "id": args.principal_id, "type": args.principal_type },
        "role": args.role.as_str()
    });
    if output::dry_run_guard(
        cli,
        "playbook add-section-role-assignment",
        &json!({ "workspace": args.resource.workspace, "id": args.resource.id, "path": args.resource.path, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(&role_path(&args.resource), &body, false)
        .await?;
    output::render_object(cli, &data, "role");
    Ok(())
}

async fn update_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    principal_id: &str,
    role: SectionRole,
) -> Result<()> {
    validate_uuid(principal_id, "--principal-id")?;
    let body = json!({ "role": role.as_str() });
    if output::dry_run_guard(
        cli,
        "playbook update-section-role-assignment",
        &json!({ "workspace": args.workspace, "id": args.id, "path": args.path, "principalId": principal_id, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!(
                "{}?principalId={}",
                role_path(args),
                urlencoding::encode(principal_id)
            ),
            &body,
        )
        .await?;
    output::render_object(cli, &data, "role");
    Ok(())
}

async fn delete_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    principal_id: &str,
) -> Result<()> {
    validate_uuid(principal_id, "--principal-id")?;
    if output::dry_run_guard(
        cli,
        "playbook delete-section-role-assignment",
        &json!({ "workspace": args.workspace, "id": args.id, "path": args.path, "principalId": principal_id }),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "{}?principalId={}",
            role_path(args),
            urlencoding::encode(principal_id)
        ))
        .await?;
    output::render_object(
        cli,
        &json!({ "status": "deleted", "principalId": principal_id }),
        "status",
    );
    Ok(())
}

async fn put_context(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    summary: &str,
    content_type: Option<&str>,
    data: Option<&str>,
    reference: Option<&str>,
) -> Result<()> {
    let mut body = json!({ "summary": summary });
    if let (Some(kind), Some(payload)) = (content_type, data) {
        body["content"] = json!({ "type": kind, "data": payload });
    }
    if let Some(value) = reference {
        let resolved = crate::commands::query_input::resolve_query_input(
            Some(value),
            "context reference",
            "--reference",
            r#"{"sourceType":"FabricIQ","itemType":"Report","identifiers":{"workspaceId":"...","itemId":"..."}}"#,
        )?;
        body["reference"] = serde_json::from_str(&resolved)?;
    }
    mutate_context(cli, client, args, true, body).await
}

async fn mutate_context(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    replace: bool,
    body: Value,
) -> Result<()> {
    let operation = if replace {
        "playbook put-context"
    } else {
        "playbook update-context"
    };
    if output::dry_run_guard(
        cli,
        operation,
        &json!({ "workspace": args.workspace, "id": args.id, "path": args.path, "request": body }),
    ) {
        return Ok(());
    }
    let path = resource_path(args, "context");
    let data = if replace {
        client.put(&path, &body).await?
    } else {
        client.patch(&path, &body).await?
    };
    output::render_object(cli, &data, "summary");
    Ok(())
}

async fn get_context(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    include_content: bool,
) -> Result<()> {
    let mut path = resource_path(args, "context");
    if include_content {
        path.push_str("&include=content");
    }
    let data = client.get(&path).await?;
    output::render_object(cli, &data, "summary");
    Ok(())
}

async fn delete_context(cli: &Cli, client: &FabricClient, args: &ResourceArgs) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "playbook delete-context",
        &json!({ "workspace": args.workspace, "id": args.id, "path": args.path }),
    ) {
        return Ok(());
    }
    client.delete(&resource_path(args, "context")).await?;
    output::render_object(
        cli,
        &json!({ "status": "deleted", "path": args.path }),
        "status",
    );
    Ok(())
}

async fn get_context_content(
    cli: &Cli,
    client: &FabricClient,
    args: &ResourceArgs,
    output_file: &str,
) -> Result<()> {
    let bytes = client.get_bytes(&resource_path(args, "content")).await?;
    std::fs::write(output_file, &bytes)?;
    output::render_object(
        cli,
        &json!({ "path": output_file, "bytes": bytes.len() }),
        "path",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_path_preserves_logical_separators() {
        let args = ResourceArgs {
            workspace: "ws".to_string(),
            id: "pb".to_string(),
            path: "Playbook/Section A".to_string(),
        };
        assert_eq!(
            resource_path(&args, "section"),
            "/workspaces/ws/playbooks/pb/sections/Playbook/Section%20A?resource=section"
        );
    }
}
