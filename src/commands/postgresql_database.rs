use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const COLLECTION: &str = "postgreSQLDatabases";
const PART: &str = "definition.pgschema";
const REBUILD_WARNING: &str = "--allow-table-rebuild is active: schema changes may rebuild objects and permanently lose affected data.";

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
            validate_metadata(name.as_deref(), description.as_deref())?;
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
    let body = create_body(
        name,
        description,
        folder_id,
        originating_product,
        null_creation_payload,
        definition,
    )?;
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

fn validate_metadata(name: Option<&str>, description: Option<&str>) -> Result<()> {
    if name.is_some_and(|value| value.trim().is_empty() || value.encode_utf16().count() > 63) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "PostgreSQL database name must be non-empty and at most 63 UTF-16 code units",
            "Shorten --name; supplementary Unicode characters count as two code units.",
        )
        .into());
    }
    if description.is_some_and(|value| value.encode_utf16().count() > 256) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "PostgreSQL database description exceeds 256 UTF-16 code units",
            "Shorten --description to 256 code units or fewer.",
        )
        .into());
    }
    Ok(())
}

fn create_body(
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    originating_product: Option<&str>,
    null_creation_payload: bool,
    definition: Option<&str>,
) -> Result<Value> {
    validate_metadata(Some(name), description)?;
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
    Ok(body)
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
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
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
    let mut preview = json!({ "workspace": workspace, "id": id, "request": body });
    if allow_table_rebuild {
        preview["warning"] = Value::from(REBUILD_WARNING);
    }
    if output::dry_run_guard(cli, "postgresql-database update-definition", &preview) {
        return Ok(());
    }
    let mut data = client
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
    if allow_table_rebuild {
        if !data.is_object() {
            data = json!({ "result": data });
        }
        data["destructive"] = Value::Bool(true);
        data["warning"] = Value::from(REBUILD_WARNING);
    }
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creation_payload_serializes_originating_product() {
        let body = create_body("database", None, None, Some("MigrationTool"), false, None).unwrap();
        assert_eq!(
            body["creationPayload"]["originatingProduct"],
            "MigrationTool"
        );
    }

    #[test]
    fn creation_distinguishes_omitted_and_explicit_null_payload() {
        let omitted = create_body("database", None, None, None, false, None).unwrap();
        assert!(omitted.get("creationPayload").is_none());
        let explicit = create_body("database", None, None, None, true, None).unwrap();
        assert_eq!(explicit.get("creationPayload"), Some(&Value::Null));
    }

    #[test]
    fn creation_wraps_schema_in_the_published_definition_part() {
        use base64::Engine;
        let sql = "CREATE TABLE example (id bigint);";
        let body = create_body(
            "database",
            Some("Synthetic schema"),
            Some("folder"),
            None,
            false,
            Some(sql),
        )
        .unwrap();
        assert_eq!(body["definition"]["format"], "pgschema");
        assert_eq!(body["definition"]["parts"][0]["path"], PART);
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(body["definition"]["parts"][0]["payload"].as_str().unwrap())
                .unwrap(),
            sql.as_bytes()
        );
        assert!(body.get("creationPayload").is_none());
        assert_eq!(body["folderId"], "folder");
    }

    #[test]
    fn metadata_limits_handle_supplementary_unicode() {
        assert!(validate_metadata(Some(&"a".repeat(63)), Some(&"a".repeat(256))).is_ok());
        assert!(validate_metadata(Some(&"\u{1f600}".repeat(31)), None).is_ok());
        assert!(validate_metadata(Some(&"\u{1f600}".repeat(32)), None).is_err());
        assert!(validate_metadata(Some(&"a".repeat(64)), None).is_err());
        assert!(validate_metadata(None, Some(&"a".repeat(257))).is_err());
        assert!(validate_metadata(Some(" \t"), None).is_err());
    }
}
