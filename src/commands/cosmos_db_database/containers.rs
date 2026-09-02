//! Cosmos DB data-plane: container operations (list / create / delete).

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::data_plane::CosmosClient;

pub(super) async fn list_containers(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    endpoint: Option<&str>,
) -> Result<()> {
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;
    let resp = cosmos.list_containers().await?;
    let containers = resp
        .body
        .get("DocumentCollections")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    output::render_list(
        cli,
        &containers,
        &["id", "partitionKey.paths", "_rid"],
        &["ID", "PARTITION KEY", "RID"],
        "id",
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create_container(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    partition_key: &str,
    autoscale_max: u32,
    ttl: Option<i64>,
    endpoint: Option<&str>,
) -> Result<()> {
    let pk = normalize_partition_key(partition_key);
    if output::dry_run_guard(
        cli,
        "cosmos-db-database create-container",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "container": container,
            "partitionKey": pk,
            "autoscaleMaxThroughput": autoscale_max,
            "defaultTtl": ttl,
        }),
    ) {
        return Ok(());
    }
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;
    let resp = cosmos
        .create_container(container, &pk, autoscale_max, ttl)
        .await?;
    output::render_object(cli, &resp.body, "id");
    Ok(())
}

pub(super) async fn delete_container(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    container: &str,
    endpoint: Option<&str>,
) -> Result<()> {
    validate_container_name(container)?;
    if output::dry_run_guard(
        cli,
        "cosmos-db-database delete-container",
        &serde_json::json!({ "workspace": workspace, "id": id, "container": container }),
    ) {
        return Ok(());
    }
    let cosmos = CosmosClient::connect(client, workspace, id, endpoint).await?;
    cosmos.delete_container(container).await?;
    let obj = serde_json::json!({ "container": container, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Normalize a partition-key path to the Cosmos form (leading `/`).
/// `categoryId` → `/categoryId`; `/categoryId` is unchanged.
pub(super) fn normalize_partition_key(path: &str) -> String {
    let trimmed = path.trim();
    if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    }
}

/// Reject an empty/whitespace/wildcard container name before a destructive call.
/// Deleting a container drops every document it holds, so a malformed target
/// must fail fast rather than risk destroying the wrong data.
pub(super) fn validate_container_name(container: &str) -> Result<()> {
    let name = container.trim();
    if name.is_empty() || name.contains('*') || name.contains('/') {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid container name for deletion: {container:?}"),
            "Provide the exact container id (no wildcards or slashes). \
             Example: fabio cosmos-db-database delete-container --workspace <WS> --id <ID> --container products",
        )
        .into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_partition_key_adds_leading_slash() {
        assert_eq!(normalize_partition_key("categoryId"), "/categoryId");
        assert_eq!(normalize_partition_key("/categoryId"), "/categoryId");
        assert_eq!(normalize_partition_key("  pk  "), "/pk");
    }

    #[test]
    fn validate_container_name_rejects_blast_radius() {
        assert!(validate_container_name("").is_err());
        assert!(validate_container_name("   ").is_err());
        assert!(validate_container_name("*").is_err());
        assert!(validate_container_name("a/b").is_err());
        assert!(validate_container_name("products").is_ok());
    }
}
