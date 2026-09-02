use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils::{
    capture_query_plan, execute_and_render_sql, parse_connection_string, resolve_sql_input,
};
use crate::output;

use super::get_connection_string;

#[allow(clippy::too_many_lines)]
pub async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = resolve_sql_input(sql)?;

    // Get connection string from warehouse or lakehouse
    let (connection_string, item_name) = get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };

    execute_and_render_sql(cli, client, &server, &database, &sql_text).await
}

pub async fn plan(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = resolve_sql_input(sql)?;

    // Get connection string from warehouse or lakehouse
    let (connection_string, item_name) = get_connection_string(client, workspace, id).await?;
    let (server, parsed_db) = parse_connection_string(&connection_string);
    let database = if item_name.is_empty() {
        parsed_db
    } else {
        item_name
    };

    let plans = capture_query_plan(client, &server, &database, &sql_text).await?;

    // Output as structured JSON: array of plan XML strings (one per statement)
    if plans.len() == 1 {
        let obj = serde_json::json!({
            "statementCount": 1,
            "plans": [{"statementIndex": 0, "planXml": plans[0]}]
        });
        output::render_object(cli, &obj, "statementCount");
    } else {
        let plan_objects: Vec<Value> = plans
            .iter()
            .enumerate()
            .map(|(i, xml)| {
                serde_json::json!({
                    "statementIndex": i,
                    "planXml": xml
                })
            })
            .collect();
        let obj = serde_json::json!({
            "statementCount": plans.len(),
            "plans": plan_objects
        });
        output::render_object(cli, &obj, "statementCount");
    }

    Ok(())
}
