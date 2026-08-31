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
pub enum MirroredGoogleLakehouseCatalogCommand {
    /// List mirrored Google Lakehouse runtime catalogs in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a mirrored Google Lakehouse runtime catalog
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,
    },
    /// Create a new mirrored Google Lakehouse runtime catalog
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update mirrored Google Lakehouse runtime catalog properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a mirrored Google Lakehouse runtime catalog
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a mirrored Google Lakehouse runtime catalog
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a mirrored Google Lakehouse runtime catalog
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Refresh catalog metadata
    #[command(display_order = 10)]
    RefreshMetadata {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,
    },
    /// List catalog mirroring scopes (workspace-level)
    #[command(display_order = 11)]
    ListScopes {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Connection ID of the catalog mirroring source (required).
        #[arg(long)]
        connection_id: String,

        /// Parent scope to list under (optional).
        #[arg(long)]
        parent: Option<String>,

        /// Recurse into nested scopes (optional).
        #[arg(long)]
        recursive: bool,
    },
    /// List catalog mirroring tables (workspace-level)
    #[command(display_order = 12)]
    ListTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Connection ID of the catalog mirroring source (required).
        #[arg(long)]
        connection_id: String,

        /// Scope to list tables under (optional).
        #[arg(long)]
        scope: Option<String>,
    },
    /// Get mirroring status
    #[command(display_order = 13)]
    MirroringStatus {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,
    },
    /// Get tables mirroring status
    #[command(display_order = 14)]
    TablesMirroringStatus {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Mirrored Google Lakehouse runtime catalog ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &MirroredGoogleLakehouseCatalogCommand,
) -> Result<()> {
    match command {
        MirroredGoogleLakehouseCatalogCommand::List { workspace } => {
            list(cli, client, workspace).await
        }
        MirroredGoogleLakehouseCatalogCommand::Show { workspace, id } => {
            show(cli, client, workspace, id).await
        }
        MirroredGoogleLakehouseCatalogCommand::Create {
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
        MirroredGoogleLakehouseCatalogCommand::Update {
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
        MirroredGoogleLakehouseCatalogCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        MirroredGoogleLakehouseCatalogCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        MirroredGoogleLakehouseCatalogCommand::UpdateDefinition {
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
        MirroredGoogleLakehouseCatalogCommand::RefreshMetadata { workspace, id } => {
            refresh_metadata(cli, client, workspace, id).await
        }
        MirroredGoogleLakehouseCatalogCommand::ListScopes {
            workspace,
            connection_id,
            parent,
            recursive,
        } => {
            list_scopes(
                cli,
                client,
                workspace,
                connection_id,
                parent.as_deref(),
                *recursive,
            )
            .await
        }
        MirroredGoogleLakehouseCatalogCommand::ListTables {
            workspace,
            connection_id,
            scope,
        } => list_tables(cli, client, workspace, connection_id, scope.as_deref()).await,
        MirroredGoogleLakehouseCatalogCommand::MirroringStatus { workspace, id } => {
            mirroring_status(cli, client, workspace, id).await
        }
        MirroredGoogleLakehouseCatalogCommand::TablesMirroringStatus { workspace, id } => {
            tables_mirroring_status(cli, client, workspace, id).await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "mirroredGoogleLakehouseRuntimeCatalogs",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(
        cli,
        client,
        "mirroredGoogleLakehouseRuntimeCatalogs",
        workspace,
        id,
    )
    .await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "mirrored-google-lakehouse-catalog create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "mirrored-google-lakehouse-catalog create", "Member"))?;
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
            "Example: fabio mirrored-google-lakehouse-catalog update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "mirrored-google-lakehouse-catalog update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}"),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "mirrored-google-lakehouse-catalog update", "Contributor")
        })?;
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
    crate::commands::crud::delete(
        cli,
        client,
        "mirrored-google-lakehouse-catalog",
        "mirroredGoogleLakehouseRuntimeCatalogs",
        "Member",
        workspace,
        id,
        hard_delete,
    )
    .await
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
            &format!(
                "/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}/getDefinition"
            ),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-google-lakehouse-catalog get-definition",
                "Contributor",
            )
        })?;
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
    let definition_json = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio mirrored-google-lakehouse-catalog update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body =
        crate::definition_spec::build_update_definition_body(&definition_json, "mirroring.json");

    if output::dry_run_guard(
        cli,
        "mirrored-google-lakehouse-catalog update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": definition_json.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}/updateDefinition"
            ),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-google-lakehouse-catalog update-definition",
                "Contributor",
            )
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Extra operations ────────────────────────────────────────────────────────

async fn refresh_metadata(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "mirrored-google-lakehouse-catalog refresh-metadata",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}/refreshCatalogMetadata?beta=true"
            ),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "mirrored-google-lakehouse-catalog refresh-metadata",
                "Contributor",
            )
        })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "refresh_triggered" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

async fn list_scopes(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    connection_id: &str,
    parent: Option<&str>,
    recursive: bool,
) -> Result<()> {
    let data = client
        .get(&build_scopes_url(
            workspace,
            connection_id,
            parent,
            recursive,
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

/// Build the catalog-mirroring scopes URL. `connectionId` is a REQUIRED query
/// param (the catalog mirroring source); `parent`/`recursive` are optional.
fn build_scopes_url(
    workspace: &str,
    connection_id: &str,
    parent: Option<&str>,
    recursive: bool,
) -> String {
    use std::fmt::Write as _;
    let mut url = format!(
        "/workspaces/{workspace}/catalogmirroring/scopes?beta=true&connectionId={connection_id}"
    );
    if let Some(p) = parent {
        let _ = write!(url, "&parent={p}");
    }
    if recursive {
        url.push_str("&recursive=true");
    }
    url
}

async fn list_tables(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    connection_id: &str,
    scope: Option<&str>,
) -> Result<()> {
    let data = client
        .get(&build_tables_url(workspace, connection_id, scope))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

/// Build the catalog-mirroring tables URL. `connectionId` is a REQUIRED query
/// param; `scope` is optional.
fn build_tables_url(workspace: &str, connection_id: &str, scope: Option<&str>) -> String {
    use std::fmt::Write as _;
    let mut url = format!(
        "/workspaces/{workspace}/catalogmirroring/tables?beta=true&connectionId={connection_id}"
    );
    if let Some(s) = scope {
        let _ = write!(url, "&scope={s}");
    }
    url
}

async fn mirroring_status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}/mirroringStatus?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "status");
    Ok(())
}

async fn tables_mirroring_status(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/mirroredGoogleLakehouseRuntimeCatalogs/{id}/tablesMirroringStatus?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "data");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{build_scopes_url, build_tables_url};

    #[test]
    fn scopes_url_includes_required_connection_id() {
        let url = build_scopes_url("ws1", "conn1", None, false);
        assert!(url.contains("catalogmirroring/scopes"));
        assert!(url.contains("beta=true"));
        assert!(url.contains("connectionId=conn1"));
        assert!(!url.contains("parent="));
        assert!(!url.contains("recursive="));
    }

    #[test]
    fn scopes_url_adds_optional_params() {
        let url = build_scopes_url("ws1", "conn1", Some("cat.schema"), true);
        assert!(url.contains("connectionId=conn1"));
        assert!(url.contains("parent=cat.schema"));
        assert!(url.contains("recursive=true"));
    }

    #[test]
    fn tables_url_includes_required_connection_id() {
        let url = build_tables_url("ws1", "conn1", None);
        assert!(url.contains("catalogmirroring/tables"));
        assert!(url.contains("connectionId=conn1"));
        assert!(!url.contains("scope="));
    }

    #[test]
    fn tables_url_adds_optional_scope() {
        let url = build_tables_url("ws1", "conn1", Some("mycat"));
        assert!(url.contains("connectionId=conn1"));
        assert!(url.contains("scope=mycat"));
    }
}
