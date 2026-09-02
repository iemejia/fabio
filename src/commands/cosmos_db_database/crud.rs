//! Control-plane (Fabric item) CRUD for Cosmos DB databases.
//!
//! These handlers manage the Cosmos DB *item* in a workspace (list/show/create/
//! update/delete + definition). They delegate to the shared [`crate::commands::crud`]
//! helpers. Data-plane operations (containers, documents, query, import) live in
//! the sibling `containers`/`documents`/`data_plane` modules.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

pub(super) async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "cosmosDbDatabases",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    crate::commands::crud::show(cli, client, "cosmosDbDatabases", workspace, id).await
}

pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    crate::commands::crud::create(
        cli,
        client,
        "cosmos-db-database",
        "cosmosDbDatabases",
        "Contributor",
        workspace,
        name,
        description,
        sensitivity_label,
    )
    .await
}

pub(super) async fn update(
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
            "Example: fabio cosmos-db-database update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "cosmos-db-database update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/cosmosDbDatabases/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "cosmos-db-database update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    crate::commands::crud::delete(
        cli,
        client,
        "cosmos-db-database",
        "cosmosDbDatabases",
        "Contributor",
        workspace,
        id,
        hard_delete,
    )
    .await
}

pub(super) async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "cosmos-db-database",
        "cosmosDbDatabases",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
}

pub(super) async fn update_definition(
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
        "cosmos-db-database",
        "cosmosDbDatabases",
        "Contributor",
        "definition.json",
        workspace,
        id,
        file,
        content,
    )
    .await
}
