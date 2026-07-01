//! Example command outputs for AI agents (response shapes + `JMESPath` tips).

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::output;

use super::find_entry;

pub(super) fn execute(cli: &Cli, group: &str, command: Option<&str>) {
    if let Some(cmd) = command {
        // Exact lookup: group/command
        let key = format!("{group}/{cmd}");
        let normalized = key.to_lowercase().replace(['-', '_'], "");
        if let Some(content) = find_entry(OUTPUT_EXAMPLES, &normalized) {
            let val: Value =
                serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
            output::render_object(cli, &val, "command");
            return;
        }
        // Not found — fall through to show available for this group
    }

    // Group-only: list all examples matching this group prefix
    let group_normalized = group.to_lowercase().replace(['-', '_'], "");
    let matches: Vec<Value> = OUTPUT_EXAMPLES
        .iter()
        .filter(|(name, _)| {
            let prefix = name
                .split('/')
                .next()
                .unwrap_or("")
                .to_lowercase()
                .replace(['-', '_'], "");
            prefix == group_normalized
        })
        .filter_map(|(name, content)| {
            let val: Value = serde_json::from_str(content).ok()?;
            Some(json!({
                "name": name,
                "command": val.get("command").and_then(Value::as_str).unwrap_or(""),
                "description": val.get("description").and_then(Value::as_str).unwrap_or(""),
            }))
        })
        .collect();

    if matches.is_empty() {
        let available: Vec<&str> = OUTPUT_EXAMPLES.iter().map(|(name, _)| *name).collect();
        let msg = command.map_or_else(
            || format!("No output examples found for group '{group}'"),
            |cmd| format!("No output example found for '{group} {cmd}'"),
        );
        let result = json!({
            "error": msg,
            "available_examples": available,
            "hint": "Use 'fabio context list' to see all available examples"
        });
        output::render_object(cli, &result, "error");
    } else if matches.len() == 1 && command.is_none() {
        // Single example for this group — show the full content directly
        let (_, content) = OUTPUT_EXAMPLES
            .iter()
            .find(|(name, _)| {
                let prefix = name
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_lowercase()
                    .replace(['-', '_'], "");
                prefix == group_normalized
            })
            .unwrap();
        let val: Value =
            serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
        output::render_object(cli, &val, "command");
    } else {
        // Multiple examples — show summary list
        output::render_list_with_token(
            cli,
            &matches,
            &["name", "description"],
            &["EXAMPLE", "DESCRIPTION"],
            "name",
            None,
        );
    }
}

pub(super) fn list_names() -> Vec<&'static str> {
    OUTPUT_EXAMPLES.iter().map(|(name, _)| *name).collect()
}

/// Expose example entries for cross-referencing by other context subcommands.
pub(super) const fn example_entries() -> &'static [(&'static str, &'static str)] {
    OUTPUT_EXAMPLES
}

const OUTPUT_EXAMPLES: &[(&str, &str)] = &[
    (
        "lakehouse/list-tables",
        include_str!("data/examples/lakehouse_list_tables.json"),
    ),
    (
        "lakehouse/iceberg-table",
        include_str!("data/examples/lakehouse_iceberg_table.json"),
    ),
    (
        "lakehouse/iceberg-stats",
        include_str!("data/examples/lakehouse_iceberg_stats.json"),
    ),
    (
        "lakehouse/sync",
        include_str!("data/examples/lakehouse_sync.json"),
    ),
    (
        "workspace/list",
        include_str!("data/examples/workspace_list.json"),
    ),
    ("item/list", include_str!("data/examples/item_list.json")),
    (
        "deploy/plan",
        include_str!("data/examples/deploy_plan.json"),
    ),
    (
        "deploy/apply",
        include_str!("data/examples/deploy_apply.json"),
    ),
    (
        "notebook/run",
        include_str!("data/examples/notebook_run.json"),
    ),
    (
        "data-agent/query",
        include_str!("data/examples/data_agent_query.json"),
    ),
    (
        "context/tenant",
        include_str!("data/examples/context_tenant.json"),
    ),
    (
        "context/tenant-summary",
        include_str!("data/examples/context_tenant_summary.json"),
    ),
    (
        "context/tenant-resolve",
        include_str!("data/examples/context_tenant_resolve.json"),
    ),
    (
        "kql-database/list-entities",
        include_str!("data/examples/kql_database_list_entities.json"),
    ),
    (
        "kql-database/describe",
        include_str!("data/examples/kql_database_describe.json"),
    ),
    (
        "kql-database/sample",
        include_str!("data/examples/kql_database_sample.json"),
    ),
    (
        "kql-database/diagnostics",
        include_str!("data/examples/kql_database_diagnostics.json"),
    ),
    (
        "kql-database/deeplink",
        include_str!("data/examples/kql_database_deeplink.json"),
    ),
    (
        "kql-database/ingest",
        include_str!("data/examples/kql_database_ingest.json"),
    ),
    (
        "eventstream/validate",
        include_str!("data/examples/eventstream_validate.json"),
    ),
    (
        "eventstream/list-components",
        include_str!("data/examples/eventstream_list_components.json"),
    ),
    (
        "reflex/create-trigger",
        include_str!("data/examples/reflex_create_trigger.json"),
    ),
    (
        "ontology/import",
        include_str!("data/examples/ontology_import.json"),
    ),
    (
        "ontology/export",
        include_str!("data/examples/ontology_export.json"),
    ),
    (
        "warehouse/query",
        include_str!("data/examples/warehouse_query.json"),
    ),
    (
        "semantic-model/query",
        include_str!("data/examples/semantic_model_query.json"),
    ),
    (
        "sql-database/import",
        include_str!("data/examples/sql_database_import.json"),
    ),
    (
        "data-pipeline/run",
        include_str!("data/examples/data_pipeline_run.json"),
    ),
    ("git/status", include_str!("data/examples/git_status.json")),
    ("rest/call", include_str!("data/examples/rest_call.json")),
    (
        "capacity/list-skus",
        include_str!("data/examples/capacity_list_skus.json"),
    ),
    (
        "admin/list-workspaces",
        include_str!("data/examples/admin_list_workspaces.json"),
    ),
    (
        "job-scheduler/run-on-demand",
        include_str!("data/examples/job_scheduler_run.json"),
    ),
    (
        "connection/create",
        include_str!("data/examples/connection_create.json"),
    ),
    (
        "workspace/create-folder",
        include_str!("data/examples/workspace_folders.json"),
    ),
    (
        "dataflow/execute-query",
        include_str!("data/examples/dataflow_execute_query.json"),
    ),
    (
        "workspace/create",
        include_str!("data/examples/workspace_create.json"),
    ),
    (
        "workspace/show",
        include_str!("data/examples/workspace_show.json"),
    ),
    (
        "lakehouse/create",
        include_str!("data/examples/lakehouse_create.json"),
    ),
    (
        "lakehouse/upload",
        include_str!("data/examples/lakehouse_upload.json"),
    ),
    (
        "lakehouse/load-table",
        include_str!("data/examples/lakehouse_load_table.json"),
    ),
    (
        "lakehouse/list-files",
        include_str!("data/examples/lakehouse_list_files.json"),
    ),
    (
        "notebook/create",
        include_str!("data/examples/notebook_create.json"),
    ),
    (
        "notebook/get-definition",
        include_str!("data/examples/notebook_get_definition.json"),
    ),
    (
        "sql-database/query",
        include_str!("data/examples/sql_database_query.json"),
    ),
    (
        "data-agent/add-datasource",
        include_str!("data/examples/data_agent_add_datasource.json"),
    ),
    (
        "data-agent/list-datasources",
        include_str!("data/examples/data_agent_list_datasources.json"),
    ),
    (
        "connection/list",
        include_str!("data/examples/connection_list.json"),
    ),
    ("git/commit", include_str!("data/examples/git_commit.json")),
    (
        "capacity/list",
        include_str!("data/examples/capacity_list.json"),
    ),
    (
        "profile/list",
        include_str!("data/examples/profile_list.json"),
    ),
    (
        "item/exists",
        include_str!("data/examples/item_exists.json"),
    ),
    (
        "item/inspect",
        include_str!("data/examples/item_inspect.json"),
    ),
    (
        "deploy/export",
        include_str!("data/examples/deploy_export.json"),
    ),
    (
        "deploy/validate",
        include_str!("data/examples/deploy_validate.json"),
    ),
    (
        "semantic-model/create",
        include_str!("data/examples/semantic_model_create.json"),
    ),
    (
        "kql-database/query",
        include_str!("data/examples/kql_database_query.json"),
    ),
    (
        "eventstream/get-topology",
        include_str!("data/examples/eventstream_get_topology.json"),
    ),
    (
        "lakehouse/create-execution-definition",
        include_str!("data/examples/lakehouse_execution_definition.json"),
    ),
];
