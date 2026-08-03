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
pub enum SparkCommand {
    /// Get workspace-level Spark settings (custom pools, starter pools, etc.)
    #[command(display_order = 1)]
    GetSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Update workspace-level Spark settings
    #[command(display_order = 2)]
    UpdateSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Settings as JSON (merges with existing settings; conflicts with --runtime-version).
        /// Example: '{"automaticLog":{"enabled":true},"highConcurrency":{"notebookInteractiveRunEnabled":true}}'
        #[arg(long)]
        settings: Option<String>,

        /// Set the workspace default Spark runtime version (e.g. "1.3" for Spark 3.5, "2.0" for the Spark 4.1 / Delta 4.2 preview). Merged into existing settings.
        #[arg(long, conflicts_with = "settings")]
        runtime_version: Option<String>,
    },

    // ── Custom Pools ─────────────────────────────────────────────────────
    /// List custom Spark pools in a workspace
    #[command(display_order = 10)]
    ListPools {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a custom Spark pool
    #[command(display_order = 11)]
    GetPool {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },
    /// Create a custom Spark pool
    #[command(display_order = 12)]
    CreatePool {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool name
        #[arg(long)]
        name: String,

        /// Node family (e.g., `MemoryOptimized`)
        #[arg(long, default_value = "MemoryOptimized")]
        node_family: String,

        /// Node size (e.g., Small, Medium, Large, `XLarge`, `XXLarge`)
        #[arg(long, default_value = "Small", value_parser = ["Small", "Medium", "Large", "XLarge", "XXLarge"])]
        node_size: String,

        /// Enable auto-scale
        #[arg(long, default_value_t = true)]
        auto_scale_enabled: bool,

        /// Minimum node count (when auto-scale is enabled)
        #[arg(long, default_value_t = 1)]
        min_node_count: u32,

        /// Maximum node count
        #[arg(long, default_value_t = 3)]
        max_node_count: u32,

        /// Enable dynamic executor allocation
        #[arg(long, default_value_t = true)]
        dynamic_executor_enabled: bool,

        /// Minimum executors for dynamic allocation
        #[arg(long, default_value_t = 1)]
        min_executors: u32,

        /// Maximum executors for dynamic allocation
        #[arg(long, default_value_t = 2)]
        max_executors: u32,
    },
    /// Update a custom Spark pool
    #[command(display_order = 13)]
    UpdatePool {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,

        /// Pool configuration as JSON (partial update)
        #[arg(long)]
        config: String,
    },
    /// Delete a custom Spark pool
    #[command(display_order = 14)]
    DeletePool {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },

    // ── Capacity Spark Settings ──────────────────────────────────────────
    /// Get capacity-level Spark settings
    #[command(display_order = 20)]
    GetCapacitySettings {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,
    },
    /// Update capacity-level Spark settings
    #[command(display_order = 21)]
    UpdateCapacitySettings {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// JSON file path with settings
        #[arg(long)]
        file: Option<String>,

        /// JSON content with settings (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Capacity Custom Pools ────────────────────────────────────────────
    /// List custom Spark pools in a capacity
    #[command(display_order = 30)]
    ListCapacityPools {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,
    },
    /// Create a custom Spark pool in a capacity
    #[command(display_order = 31)]
    CreateCapacityPool {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// Pool name
        #[arg(long)]
        name: String,

        /// JSON file path with pool configuration
        #[arg(long)]
        file: Option<String>,

        /// JSON content with pool configuration (inline)
        #[arg(long)]
        content: Option<String>,
    },
    /// Get details of a capacity Spark pool
    #[command(display_order = 32)]
    GetCapacityPool {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },
    /// Update a capacity Spark pool
    #[command(display_order = 33)]
    UpdateCapacityPool {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,

        /// JSON file path with pool configuration
        #[arg(long)]
        file: Option<String>,

        /// JSON content with pool configuration (inline)
        #[arg(long)]
        content: Option<String>,
    },
    /// Delete a capacity Spark pool
    #[command(display_order = 34)]
    DeleteCapacityPool {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,

        /// Pool ID
        #[arg(long)]
        pool_id: String,
    },

    // ── Livy Sessions ────────────────────────────────────────────────────
    /// List Livy sessions in a workspace
    #[command(display_order = 40)]
    ListLivySessions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Get details of a Livy session
    #[command(display_order = 41)]
    GetLivySession {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Livy session ID
        #[arg(long)]
        livy_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &SparkCommand) -> Result<()> {
    match command {
        SparkCommand::GetSettings { workspace } => get_settings(cli, client, workspace).await,
        SparkCommand::UpdateSettings {
            workspace,
            settings,
            runtime_version,
        } => {
            update_settings(
                cli,
                client,
                workspace,
                settings.as_deref(),
                runtime_version.as_deref(),
            )
            .await
        }
        SparkCommand::ListPools { workspace } => list_pools(cli, client, workspace).await,
        SparkCommand::GetPool { workspace, pool_id } => {
            get_pool(cli, client, workspace, pool_id).await
        }
        SparkCommand::CreatePool {
            workspace,
            name,
            node_family,
            node_size,
            auto_scale_enabled,
            min_node_count,
            max_node_count,
            dynamic_executor_enabled,
            min_executors,
            max_executors,
        } => {
            create_pool(
                cli,
                client,
                workspace,
                name,
                node_family,
                node_size,
                *auto_scale_enabled,
                *min_node_count,
                *max_node_count,
                *dynamic_executor_enabled,
                *min_executors,
                *max_executors,
            )
            .await
        }
        SparkCommand::UpdatePool {
            workspace,
            pool_id,
            config,
        } => update_pool(cli, client, workspace, pool_id, config).await,
        SparkCommand::DeletePool { workspace, pool_id } => {
            delete_pool(cli, client, workspace, pool_id).await
        }
        SparkCommand::GetCapacitySettings { capacity_id } => {
            get_capacity_settings(cli, client, capacity_id).await
        }
        SparkCommand::UpdateCapacitySettings {
            capacity_id,
            file,
            content,
        } => {
            update_capacity_settings(
                cli,
                client,
                capacity_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        SparkCommand::ListCapacityPools { capacity_id } => {
            list_capacity_pools(cli, client, capacity_id).await
        }
        SparkCommand::CreateCapacityPool {
            capacity_id,
            name,
            file,
            content,
        } => {
            create_capacity_pool(
                cli,
                client,
                capacity_id,
                name,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        SparkCommand::GetCapacityPool {
            capacity_id,
            pool_id,
        } => get_capacity_pool(cli, client, capacity_id, pool_id).await,
        SparkCommand::UpdateCapacityPool {
            capacity_id,
            pool_id,
            file,
            content,
        } => {
            update_capacity_pool(
                cli,
                client,
                capacity_id,
                pool_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        SparkCommand::DeleteCapacityPool {
            capacity_id,
            pool_id,
        } => delete_capacity_pool(cli, client, capacity_id, pool_id).await,
        SparkCommand::ListLivySessions { workspace } => {
            list_livy_sessions(cli, client, workspace).await
        }
        SparkCommand::GetLivySession { workspace, livy_id } => {
            get_livy_session(cli, client, workspace, livy_id).await
        }
    }
}

// ─── Settings ────────────────────────────────────────────────────────────────

async fn get_settings(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/spark/settings"))
        .await?;
    output::render_object(cli, &data, "workspace");
    Ok(())
}

async fn update_settings(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    settings: Option<&str>,
    runtime_version: Option<&str>,
) -> Result<()> {
    let path = format!("/workspaces/{workspace}/spark/settings");

    // Two mutually-exclusive modes:
    //   (a) raw JSON via --settings (merges with existing settings server-side)
    //   (b) typed --runtime-version (read-merge-write, preserving all other settings
    //       and any sibling keys under `environment`)
    let body = if let Some(settings) = settings {
        serde_json::from_str::<Value>(settings).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --settings JSON: {e}"),
                "Provide valid JSON, e.g.: --settings '{\"automaticLog\":{\"enabled\":true}}'",
            )
        })?
    } else if let Some(rv) = runtime_version {
        if output::dry_run_guard(
            cli,
            "spark update-settings",
            &serde_json::json!({ "workspace": workspace, "runtimeVersion": rv }),
        ) {
            return Ok(());
        }
        let current = client
            .get(&path)
            .await
            .map_err(|e| enrich_forbidden(e, "spark update-settings", "Admin"))?;
        let updated = apply_workspace_runtime_version(current, rv);
        let data = client
            .patch(&path, &updated)
            .await
            .map_err(|e| enrich_forbidden(e, "spark update-settings", "Admin"))?;
        output::render_object(cli, &data, "workspace");
        return Ok(());
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide either --settings or --runtime-version".to_string(),
            "Example: fabio spark update-settings --workspace <WS> --runtime-version 2.0",
        )
        .into());
    };

    if output::dry_run_guard(cli, "spark update-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(&path, &body)
        .await
        .map_err(|e| enrich_forbidden(e, "spark update-settings", "Admin"))?;
    output::render_object(cli, &data, "workspace");
    Ok(())
}

/// Set `environment.runtimeVersion` on a workspace Spark-settings object, preserving
/// all other settings and any sibling keys under `environment`. Pure for testing.
fn apply_workspace_runtime_version(mut current: Value, runtime_version: &str) -> Value {
    if !current.is_object() {
        current = Value::Object(serde_json::Map::new());
    }
    let obj = current.as_object_mut().expect("ensured object above");
    let env = obj
        .entry("environment".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    if !env.is_object() {
        *env = Value::Object(serde_json::Map::new());
    }
    env.as_object_mut()
        .expect("ensured object above")
        .insert("runtimeVersion".to_string(), Value::from(runtime_version));
    current
}

// ─── Custom Pools ────────────────────────────────────────────────────────────

async fn list_pools(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/spark/pools"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "id", "nodeFamily", "nodeSize"],
        &["NAME", "ID", "NODE FAMILY", "NODE SIZE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_pool(cli: &Cli, client: &FabricClient, workspace: &str, pool_id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/spark/pools/{pool_id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create_pool(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    node_family: &str,
    node_size: &str,
    auto_scale_enabled: bool,
    min_node_count: u32,
    max_node_count: u32,
    dynamic_executor_enabled: bool,
    min_executors: u32,
    max_executors: u32,
) -> Result<()> {
    let body = serde_json::json!({
        "name": name,
        "nodeFamily": node_family,
        "nodeSize": node_size,
        "autoScale": {
            "enabled": auto_scale_enabled,
            "minNodeCount": min_node_count,
            "maxNodeCount": max_node_count
        },
        "dynamicExecutorAllocation": {
            "enabled": dynamic_executor_enabled,
            "minExecutors": min_executors,
            "maxExecutors": max_executors
        }
    });

    if output::dry_run_guard(cli, "spark create-pool", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/spark/pools"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark create-pool", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_pool(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    pool_id: &str,
    config: &str,
) -> Result<()> {
    let body: Value = serde_json::from_str(config).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --config JSON: {e}"),
            "Provide valid JSON pool configuration via --config.",
        )
    })?;

    if output::dry_run_guard(cli, "spark update-pool", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/spark/pools/{pool_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark update-pool", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_pool(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    pool_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "spark delete-pool",
        &serde_json::json!({ "workspace": workspace, "poolId": pool_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/workspaces/{workspace}/spark/pools/{pool_id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "spark delete-pool", "Admin"))?;

    let obj = serde_json::json!({ "poolId": pool_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Capacity Spark Settings ─────────────────────────────────────────────────

async fn get_capacity_settings(cli: &Cli, client: &FabricClient, capacity_id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/capacities/{capacity_id}/spark/settings?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "capacity");
    Ok(())
}

async fn update_capacity_settings(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-capacity-settings")?;

    if output::dry_run_guard(cli, "spark update-capacity-settings", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/capacities/{capacity_id}/spark/settings?beta=true"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark update-capacity-settings", "Admin"))?;
    output::render_object(cli, &data, "capacity");
    Ok(())
}

// ─── Capacity Custom Pools ───────────────────────────────────────────────────

async fn list_capacity_pools(cli: &Cli, client: &FabricClient, capacity_id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/capacities/{capacity_id}/spark/pools?beta=true"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "id", "nodeFamily", "nodeSize"],
        &["NAME", "ID", "NODE FAMILY", "NODE SIZE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn create_capacity_pool(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    name: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let mut body = match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{f}': {e}"))?;
            serde_json::from_str::<Value>(&text)?
        }
        (_, Some(c)) => serde_json::from_str::<Value>(c)?,
        (None, None) => serde_json::json!({}),
    };
    body["name"] = Value::from(name);

    if output::dry_run_guard(cli, "spark create-capacity-pool", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/capacities/{capacity_id}/spark/pools?beta=true"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark create-capacity-pool", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_capacity_pool(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    pool_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/capacities/{capacity_id}/spark/pools/{pool_id}?beta=true"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_capacity_pool(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    pool_id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "update-capacity-pool")?;

    if output::dry_run_guard(cli, "spark update-capacity-pool", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/capacities/{capacity_id}/spark/pools/{pool_id}?beta=true"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "spark update-capacity-pool", "Admin"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_capacity_pool(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
    pool_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "spark delete-capacity-pool",
        &serde_json::json!({ "capacityId": capacity_id, "poolId": pool_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/capacities/{capacity_id}/spark/pools/{pool_id}?beta=true"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "spark delete-capacity-pool", "Admin"))?;

    let obj = serde_json::json!({ "poolId": pool_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Livy Sessions ───────────────────────────────────────────────────────────

async fn list_livy_sessions(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/spark/livySessions"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "name", "state", "kind"],
        &["ID", "NAME", "STATE", "KIND"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn get_livy_session(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    livy_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/spark/livySessions/{livy_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn read_json_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{f}': {e}"))?;
            Ok(serde_json::from_str(&text)?)
        }
        (_, Some(c)) => Ok(serde_json::from_str(c)?),
        _ => Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!("Example: fabio spark {command} --capacity-id <ID> --file settings.json"),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn apply_runtime_version_sets_nested_field() {
        let current = json!({
            "automaticLog": { "enabled": true },
            "environment": { "runtimeVersion": "1.3" }
        });
        let out = apply_workspace_runtime_version(current, "2.0");
        assert_eq!(out["environment"]["runtimeVersion"], json!("2.0"));
        // Sibling top-level settings preserved.
        assert_eq!(out["automaticLog"]["enabled"], json!(true));
    }

    #[test]
    fn apply_runtime_version_preserves_environment_siblings() {
        let current = json!({ "environment": { "runtimeVersion": "1.3", "otherKey": "keep" } });
        let out = apply_workspace_runtime_version(current, "2.0");
        assert_eq!(out["environment"]["runtimeVersion"], json!("2.0"));
        assert_eq!(out["environment"]["otherKey"], json!("keep"));
    }

    #[test]
    fn apply_runtime_version_creates_environment_when_absent() {
        let current = json!({ "automaticLog": { "enabled": false } });
        let out = apply_workspace_runtime_version(current, "2.0");
        assert_eq!(out["environment"]["runtimeVersion"], json!("2.0"));
    }

    #[test]
    fn apply_runtime_version_recovers_from_non_object() {
        let out = apply_workspace_runtime_version(json!("nope"), "2.0");
        assert_eq!(out["environment"]["runtimeVersion"], json!("2.0"));
    }
}
