use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const COLLECTION: &str = "postgreSQLDatabases";
const PART: &str = "definition.pgschema";

#[derive(Debug, Subcommand)]
pub enum PostgreSqlDatabaseCommand {
    /// List PostgreSQL databases in a workspace
    List {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        no_recursive: bool,
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a PostgreSQL database
    Show {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
    },
    /// Create a PostgreSQL database
    Create {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name (maximum 63 characters)
        #[arg(long)]
        name: String,
        #[arg(long)]
        description: Option<String>,
        #[arg(long)]
        folder_id: Option<String>,
        /// Caller-declared product that initiated provisioning
        #[arg(long, conflicts_with = "definition")]
        originating_product: Option<String>,
        /// Explicitly send a null creation payload
        #[arg(long, conflicts_with_all = ["originating_product", "definition"])]
        null_creation_payload: bool,
        /// PostgreSQL schema definition content (inline or @file)
        #[arg(long)]
        definition: Option<String>,
    },
    /// Update PostgreSQL database metadata
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
    /// Permanently delete a PostgreSQL database
    Delete {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
    },
    /// Get a PostgreSQL database definition
    GetDefinition {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long)]
        decode: bool,
    },
    /// Replace a PostgreSQL database definition
    UpdateDefinition {
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        #[arg(long)]
        id: String,
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        #[arg(long)]
        content: Option<String>,
        /// Consent to schema changes that may rebuild objects and lose data
        #[arg(long)]
        allow_table_rebuild: bool,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &PostgreSqlDatabaseCommand,
) -> Result<()> {
    match command {
        PostgreSqlDatabaseCommand::List {
            workspace,
            no_recursive,
            root_folder_id,
        } => {
            crate::commands::crud::list_with_options(
                cli,
                client,
                COLLECTION,
                workspace,
                *no_recursive,
                root_folder_id.as_deref(),
                &[
                    "displayName",
                    "id",
                    "properties.databaseName",
                    "properties.serverFqdn",
                ],
                &["NAME", "ID", "DATABASE", "SERVER"],
            )
            .await
        }
        PostgreSqlDatabaseCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        PostgreSqlDatabaseCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            originating_product,
            null_creation_payload,
            definition,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                folder_id.as_deref(),
                originating_product.as_deref(),
                *null_creation_payload,
                definition.as_deref(),
            )
            .await
        }
        PostgreSqlDatabaseCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crate::commands::crud::update(
                cli,
                client,
                "postgresql-database",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        PostgreSqlDatabaseCommand::Delete { workspace, id } => {
            crate::commands::crud::delete(
                cli,
                client,
                "postgresql-database",
                COLLECTION,
                "Contributor",
                workspace,
                id,
                false,
            )
            .await
        }
        PostgreSqlDatabaseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => {
            crate::commands::crud::get_definition(
                cli,
                client,
                "postgresql-database",
                COLLECTION,
                "Viewer",
                workspace,
                id,
                *decode,
            )
            .await
        }
        PostgreSqlDatabaseCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            allow_table_rebuild,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                *allow_table_rebuild,
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    originating_product: Option<&str>,
    null_creation_payload: bool,
    definition: Option<&str>,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    if let Some(folder) = folder_id {
        validate_uuid(folder, "--folder-id")?;
    }
    let mut body = json!({ "displayName": name });
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = definition {
        let resolved = crate::commands::query_input::resolve_query_input(
            Some(value),
            "PostgreSQL definition",
            "--definition",
            "{}",
        )?;
        body["definition"] = crate::definition_spec::build_update_definition_body(&resolved, PART)
            ["definition"]
            .clone();
        body["definition"]["format"] = Value::from("pgschema");
    } else if null_creation_payload {
        body["creationPayload"] = Value::Null;
    } else if let Some(value) = originating_product {
        body["creationPayload"] = json!({ "originatingProduct": value });
    }
    if output::dry_run_guard(
        cli,
        "postgresql-database create",
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
        .await
        .map_err(|error| enrich_forbidden(error, "postgresql-database create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    allow_table_rebuild: bool,
) -> Result<()> {
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)?,
        (_, Some(value)) => value.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "A PostgreSQL definition is required.",
                "Provide --file or --content.",
            )
            .into());
        }
    };
    let mut body = crate::definition_spec::build_update_definition_body(&raw, PART);
    body["definition"]["format"] = Value::from("pgschema");
    if allow_table_rebuild {
        body["options"] = json!({ "allowTableRebuild": true });
    }
    if output::dry_run_guard(
        cli,
        "postgresql-database update-definition",
        &json!({ "workspace": workspace, "id": id, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "postgresql-database update-definition",
                "Contributor",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn creation_payload_serializes_originating_product() {
        let body = json!({ "creationPayload": { "originatingProduct": "MigrationTool" } });
        assert_eq!(
            body["creationPayload"]["originatingProduct"],
            "MigrationTool"
        );
    }
}
