//! `fabio scorecard` — Power BI Goals (Metrics) scorecards.
//!
//! Scorecards are a **Power BI** artifact (the "Goals"/Metrics feature), NOT a
//! Fabric Items API type — the Fabric Items API rejects `Scorecard` as an
//! invalid item type. They are served by the Power BI REST API under
//! `/groups/{workspaceId}/scorecards` (`OData`). This group therefore uses the
//! `*_powerbi` client helpers, and scorecards do not appear in the Fabric
//! item-capability matrix. A scorecard contains `goals` (internally "metrics").

use anyhow::Result;
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Scorecards are a Power BI Goals artifact (not a Fabric item). For complete flag reference, run: fabio context agent"
)]
pub enum ScorecardCommand {
    /// List scorecards in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show a scorecard (add --goals to expand its goals)
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard ID
        #[arg(long)]
        id: String,
        /// Expand the scorecard's goals in the response
        #[arg(long)]
        goals: bool,
    },
    /// Create a scorecard
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard name
        #[arg(long)]
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,
        /// Optional contact (user email or group)
        #[arg(long)]
        contact: Option<String>,
    },
    /// Delete a scorecard (permanent — Power BI has no soft delete)
    #[command(display_order = 4)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard ID
        #[arg(long)]
        id: String,
    },
    /// List a scorecard's goals
    #[command(display_order = 5)]
    ListGoals {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard ID
        #[arg(long)]
        id: String,
    },
    /// Create a goal in a scorecard
    #[command(display_order = 6)]
    CreateGoal {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard ID
        #[arg(long)]
        id: String,
        /// Goal name
        #[arg(long)]
        name: String,
        /// Optional display rank (ordering)
        #[arg(long)]
        rank: Option<i64>,
    },
    /// Delete a goal from a scorecard (permanent)
    #[command(display_order = 7)]
    DeleteGoal {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Scorecard ID
        #[arg(long)]
        id: String,
        /// Goal ID
        #[arg(long)]
        goal_id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &ScorecardCommand) -> Result<()> {
    match command {
        ScorecardCommand::List { workspace } => list(cli, client, workspace).await,
        ScorecardCommand::Show {
            workspace,
            id,
            goals,
        } => show(cli, client, workspace, id, *goals).await,
        ScorecardCommand::Create {
            workspace,
            name,
            description,
            contact,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                contact.as_deref(),
            )
            .await
        }
        ScorecardCommand::Delete { workspace, id } => delete(cli, client, workspace, id).await,
        ScorecardCommand::ListGoals { workspace, id } => {
            list_goals(cli, client, workspace, id).await
        }
        ScorecardCommand::CreateGoal {
            workspace,
            id,
            name,
            rank,
        } => create_goal(cli, client, workspace, id, name, *rank).await,
        ScorecardCommand::DeleteGoal {
            workspace,
            id,
            goal_id,
        } => delete_goal(cli, client, workspace, id, goal_id).await,
    }
}

/// Extract the `OData` `value` array from a Power BI list response.
fn odata_items(resp: &Value) -> Vec<Value> {
    resp.get("value")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_powerbi(&format!("/groups/{workspace}/scorecards"))
        .await?;
    let items = odata_items(&resp);
    output::render_list(
        cli,
        &items,
        &["name", "id", "description", "contact"],
        &["NAME", "ID", "DESCRIPTION", "CONTACT"],
        "id",
    );
    Ok(())
}

async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    goals: bool,
) -> Result<()> {
    let path = if goals {
        format!("/groups/{workspace}/scorecards({id})?$expand=goals")
    } else {
        format!("/groups/{workspace}/scorecards({id})")
    };
    let data = client.get_powerbi(&path).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    contact: Option<&str>,
) -> Result<()> {
    let mut body = json!({ "name": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(c) = contact {
        body["contact"] = Value::from(c);
    }
    if output::dry_run_guard(
        cli,
        "scorecard create",
        &json!({ "workspace": workspace, "name": name, "description": description, "contact": contact }),
    ) {
        return Ok(());
    }
    let data = client
        .post_powerbi(&format!("/groups/{workspace}/scorecards"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "scorecard create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "scorecard delete",
        &json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }
    client
        .delete_powerbi(&format!("/groups/{workspace}/scorecards({id})"))
        .await
        .map_err(|e| enrich_forbidden(e, "scorecard delete", "Contributor"))?;
    output::render_object(cli, &json!({ "id": id, "status": "deleted" }), "status");
    Ok(())
}

async fn list_goals(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let resp = client
        .get_powerbi(&format!("/groups/{workspace}/scorecards({id})/goals"))
        .await?;
    let items = odata_items(&resp);
    output::render_list(
        cli,
        &items,
        &["name", "id", "scorecardId"],
        &["NAME", "ID", "SCORECARD ID"],
        "id",
    );
    Ok(())
}

async fn create_goal(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    rank: Option<i64>,
) -> Result<()> {
    let mut body = json!({ "name": name });
    if let Some(r) = rank {
        body["rank"] = Value::from(r);
    }
    if output::dry_run_guard(
        cli,
        "scorecard create-goal",
        &json!({ "workspace": workspace, "scorecardId": id, "name": name, "rank": rank }),
    ) {
        return Ok(());
    }
    let data = client
        .post_powerbi(
            &format!("/groups/{workspace}/scorecards({id})/goals"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "scorecard create-goal", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_goal(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    goal_id: &str,
) -> Result<()> {
    if goal_id.trim().is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--goal-id must not be empty".to_string(),
            "List goals first: fabio scorecard list-goals --workspace <WS> --id <SCORECARD_ID>"
                .to_string(),
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        "scorecard delete-goal",
        &json!({ "workspace": workspace, "scorecardId": id, "goalId": goal_id }),
    ) {
        return Ok(());
    }
    client
        .delete_powerbi(&format!(
            "/groups/{workspace}/scorecards({id})/goals({goal_id})"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "scorecard delete-goal", "Contributor"))?;
    output::render_object(
        cli,
        &json!({ "id": goal_id, "status": "deleted" }),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odata_items_extracts_value_array() {
        let resp = json!({"value": [{"id": "1"}, {"id": "2"}]});
        assert_eq!(odata_items(&resp).len(), 2);
    }

    #[test]
    fn odata_items_empty_when_no_value() {
        assert!(odata_items(&json!({})).is_empty());
        assert!(odata_items(&json!({"value": null})).is_empty());
    }
}
