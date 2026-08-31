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
pub enum MlExperimentCommand {
    /// List ML experiments in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an ML experiment
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML experiment ID
        #[arg(long)]
        id: String,
    },
    /// Create a new ML experiment
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML experiment display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update ML experiment properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML experiment ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an ML experiment
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML experiment ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// List runs in an experiment (`MLflow` tracking: parameters, metrics, status)
    #[command(display_order = 6)]
    ListRuns {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// ML experiment ID
        #[arg(long)]
        id: String,

        /// `MLflow` filter expression (e.g. "metrics.accuracy > 0.9 and status = 'FINISHED'")
        #[arg(long)]
        filter: Option<String>,

        /// Order-by expression (e.g. `start_time DESC`, `metrics.accuracy DESC`)
        #[arg(long)]
        order_by: Option<String>,
    },
    /// Show details of a single run (info, parameters, metrics, tags)
    #[command(display_order = 7)]
    GetRun {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// `MLflow` run ID
        #[arg(long)]
        run_id: String,
    },
    /// Get the logged history of a single metric across a run's steps
    #[command(display_order = 8)]
    GetMetricHistory {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// `MLflow` run ID
        #[arg(long)]
        run_id: String,

        /// Metric key (e.g. "accuracy", "loss")
        #[arg(long)]
        metric: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &MlExperimentCommand,
) -> Result<()> {
    match command {
        MlExperimentCommand::List { workspace } => list(cli, client, workspace).await,
        MlExperimentCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        MlExperimentCommand::Create {
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
        MlExperimentCommand::Update {
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
        MlExperimentCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        MlExperimentCommand::ListRuns {
            workspace,
            id,
            filter,
            order_by,
        } => {
            list_runs(
                cli,
                client,
                workspace,
                id,
                filter.as_deref(),
                order_by.as_deref(),
            )
            .await
        }
        MlExperimentCommand::GetRun { workspace, run_id } => {
            get_run(cli, client, workspace, run_id).await
        }
        MlExperimentCommand::GetMetricHistory {
            workspace,
            run_id,
            metric,
        } => get_metric_history(cli, client, workspace, run_id, metric).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "mlExperiments",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "mlExperiments", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
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
        "ml-experiment create",
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
            &format!("/workspaces/{workspace}/mlExperiments"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment create", "Member"))?;
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
            "Example: fabio ml-experiment update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "ml-experiment update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/mlExperiments/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment update", "Contributor"))?;
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
        "ml-experiment delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/mlExperiments/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/mlExperiments/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── MLflow run tracking ─────────────────────────────────────────────────────
//
// Fabric hosts a per-workspace MLflow tracking server. Its REST API lives under
// `/workspaces/{ws}/mlflow/api/2.0/mlflow/...` and accepts the standard Fabric
// bearer token. The experiment's item ID doubles as the MLflow experiment_id.

/// Base path of the Fabric-hosted `MLflow` REST API for a workspace.
fn mlflow_base(workspace: &str) -> String {
    format!("/workspaces/{workspace}/mlflow/api/2.0/mlflow")
}

/// Build the `runs/search` request body. Pure — unit-tested.
fn build_search_body(
    experiment_id: &str,
    max_results: Option<usize>,
    filter: Option<&str>,
    order_by: Option<&str>,
) -> Value {
    let mut body = serde_json::json!({ "experiment_ids": [experiment_id] });
    if let Some(n) = max_results {
        body["max_results"] = Value::from(n);
    }
    if let Some(f) = filter {
        body["filter"] = Value::from(f);
    }
    if let Some(o) = order_by {
        body["order_by"] = serde_json::json!([o]);
    }
    body
}

async fn list_runs(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    filter: Option<&str>,
    order_by: Option<&str>,
) -> Result<()> {
    let search_body = build_search_body(id, cli.limit, filter, order_by);
    let data = client
        .post(
            &format!("{}/runs/search", mlflow_base(workspace)),
            &search_body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment list-runs", "Viewer"))?;

    let runs = data
        .get("runs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let token = data
        .get("next_page_token")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty());
    output::render_list_with_token(
        cli,
        &runs,
        &[
            "info.run_id",
            "info.run_name",
            "info.status",
            "info.start_time",
        ],
        &["RUN ID", "NAME", "STATUS", "START TIME"],
        "info.run_id",
        token,
    );
    Ok(())
}

async fn get_run(cli: &Cli, client: &FabricClient, workspace: &str, run_id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "{}/runs/get?run_id={run_id}",
            mlflow_base(workspace)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment get-run", "Viewer"))?;
    // Unwrap the `{ "run": {...} }` envelope for a cleaner object.
    let run = data.get("run").cloned().unwrap_or(data);
    output::render_object(cli, &run, "info.run_id");
    Ok(())
}

async fn get_metric_history(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    run_id: &str,
    metric: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "{}/metrics/get-history?run_id={run_id}&metric_key={metric}",
            mlflow_base(workspace)
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "ml-experiment get-metric-history", "Viewer"))?;
    let metrics = data
        .get("metrics")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    output::render_list_with_token(
        cli,
        &metrics,
        &["key", "value", "step", "timestamp"],
        &["METRIC", "VALUE", "STEP", "TIMESTAMP"],
        "value",
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlflow_base_path() {
        assert_eq!(
            mlflow_base("ws-1"),
            "/workspaces/ws-1/mlflow/api/2.0/mlflow"
        );
    }

    #[test]
    fn build_search_body_minimal() {
        let body = build_search_body("exp-1", None, None, None);
        assert_eq!(body, serde_json::json!({ "experiment_ids": ["exp-1"] }));
    }

    #[test]
    fn build_search_body_full() {
        let body = build_search_body(
            "exp-1",
            Some(5),
            Some("metrics.acc > 0.9"),
            Some("start_time DESC"),
        );
        assert_eq!(body["experiment_ids"][0], "exp-1");
        assert_eq!(body["max_results"], 5);
        assert_eq!(body["filter"], "metrics.acc > 0.9");
        assert_eq!(body["order_by"][0], "start_time DESC");
    }
}
