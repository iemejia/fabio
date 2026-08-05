pub mod admin;
pub mod anomaly_detector;
pub mod apache_airflow_job;
pub mod app_backend;
pub mod auth;
pub mod azure_databricks_storage;
pub mod capacity;
pub mod catalog;
pub mod connection;
pub mod context;
pub mod copy_job;
pub mod cosmos_db_database;
pub mod dashboard;
pub mod data_build_tool_job;
pub mod data_pipeline;
pub mod dataagent;
pub mod dataflow;
pub mod datamart;
pub mod deploy;
pub mod deployment_pipeline;
pub mod digital_twin_builder;
pub mod digital_twin_builder_flow;
pub mod domain;
pub mod environment;
pub mod event_schema_set;
pub mod eventhouse;
pub mod eventstream;
pub mod feedback;
pub mod gateway;
pub mod git;
pub mod graph_model;
pub mod graph_query_set;
pub mod graphql_api;
pub mod item;
pub mod job_scheduler;
pub mod jobs;
pub mod kql_dashboard;
pub mod kql_database;
pub mod kql_queryset;
pub mod kql_utils;
pub mod label;
pub mod lakehouse;
pub mod lro;
pub mod managed_private_endpoint;
pub mod map;
pub mod mcp;
pub mod mirrored_catalog;
pub mod mirrored_database;
pub mod mirrored_databricks_catalog;
pub mod mirrored_warehouse;
pub mod ml_experiment;
pub mod ml_model;
pub mod mounted_data_factory;
pub mod notebook;
pub mod onelake_security;
pub mod ontology;
pub mod operations_agent;
pub mod org_app;
pub mod org_app_audience;
pub mod paginated_report;
pub mod plan;
pub mod powerbi_export;
pub mod profile;
pub mod query_input;
pub mod reflex;
pub mod report;
pub mod rest;
pub mod rti;
pub mod semantic_model;
pub mod snowflake_database;
pub mod spark;
pub mod spark_job_definition;
pub mod sql_database;
pub mod sql_endpoint;
pub mod tds_utils;
pub mod user_data_function;
pub mod variable_library;
pub mod warehouse;
pub mod warehouse_snapshot;
pub mod workspace;

pub mod upgrade;

mod completions;

use anyhow::Result;

use crate::cli::{Cli, Command};
use crate::client::FabricClient;

/// Execute the CLI command.
#[allow(
    clippy::too_many_lines,
    clippy::large_stack_frames,
    clippy::large_futures
)]
pub async fn execute(cli: Cli) -> Result<()> {
    crate::metrics::init();
    let mut client = FabricClient::new();

    // Enable verbose tracing if --verbose flag is set (and not --quiet)
    if cli.verbose && !cli.quiet {
        crate::verbose::enable();
        client = client.with_verbose(true);
    }

    // Apply LRO timeout from --lro-timeout flag if specified
    if let Some(timeout_secs) = cli.lro_timeout {
        client = client.with_lro_timeout(std::time::Duration::from_secs(timeout_secs));
    }

    // Apply readonly mode from --readonly flag or FABIO_READONLY env var
    if cli.readonly {
        client = client.with_readonly(true);
    }

    // Apply private link routing from profile if configured
    if let Some(ws_id) = resolve_private_link_workspace(&cli) {
        client = client.with_private_link(ws_id);
    }

    // Enforce --enable-commands / --disable-commands before dispatch.
    check_command_policy(&cli)?;

    match &cli.command {
        // Admin
        Command::Admin { command } => admin::execute(&cli, &client, command).await,
        // Core
        Command::Workspace { command } => workspace::execute(&cli, &client, command).await,
        Command::Item { command } => item::execute(&cli, &client, command).await,
        Command::Lakehouse { command } => lakehouse::execute(&cli, &client, command).await,
        Command::Capacity { command } => capacity::execute(&cli, &client, command).await,
        Command::Catalog { command } => catalog::execute(&cli, &client, command).await,
        Command::Context { command } => context::execute(&cli, &client, command).await,
        // Data & Compute
        Command::Notebook { command } => notebook::execute(&cli, &client, command).await,
        Command::Warehouse { command } => warehouse::execute(&cli, &client, command).await,
        Command::SqlDatabase { command } => sql_database::execute(&cli, &client, command).await,
        Command::SqlEndpoint { command } => sql_endpoint::execute(&cli, &client, command).await,
        Command::DataAgent { command } => dataagent::execute(&cli, &client, command).await,
        Command::Ontology { command } => ontology::execute(&cli, &client, command).await,
        Command::Environment { command } => environment::execute(&cli, &client, command).await,
        Command::DataPipeline { command } => data_pipeline::execute(&cli, &client, command).await,
        Command::CopyJob { command } => copy_job::execute(&cli, &client, command).await,
        Command::Dataflow { command } => dataflow::execute(&cli, &client, command).await,
        Command::Report { command } => report::execute(&cli, &client, command).await,
        Command::SemanticModel { command } => semantic_model::execute(&cli, &client, command).await,
        Command::Eventhouse { command } => eventhouse::execute(&cli, &client, command).await,
        Command::Eventstream { command } => eventstream::execute(&cli, &client, command).await,
        Command::KqlDatabase { command } => kql_database::execute(&cli, &client, command).await,
        Command::KqlQueryset { command } => kql_queryset::execute(&cli, &client, command).await,
        Command::KqlDashboard { command } => kql_dashboard::execute(&cli, &client, command).await,
        Command::MirroredDatabase { command } => {
            mirrored_database::execute(&cli, &client, command).await
        }
        Command::Reflex { command } => reflex::execute(&cli, &client, command).await,
        Command::MlModel { command } => ml_model::execute(&cli, &client, command).await,
        Command::MlExperiment { command } => ml_experiment::execute(&cli, &client, command).await,
        Command::Spark { command } => spark::execute(&cli, &client, command).await,
        Command::SparkJobDefinition { command } => {
            spark_job_definition::execute(&cli, &client, command).await
        }
        Command::GraphqlApi { command } => graphql_api::execute(&cli, &client, command).await,
        Command::CosmosDbDatabase { command } => {
            cosmos_db_database::execute(&cli, &client, command).await
        }
        Command::SnowflakeDatabase { command } => {
            snowflake_database::execute(&cli, &client, command).await
        }
        Command::DigitalTwinBuilder { command } => {
            digital_twin_builder::execute(&cli, &client, command).await
        }
        Command::DigitalTwinBuilderFlow { command } => {
            digital_twin_builder_flow::execute(&cli, &client, command).await
        }
        Command::EventSchemaSet { command } => {
            event_schema_set::execute(&cli, &client, command).await
        }
        Command::OperationsAgent { command } => {
            operations_agent::execute(&cli, &client, command).await
        }
        Command::MountedDataFactory { command } => {
            mounted_data_factory::execute(&cli, &client, command).await
        }
        Command::UserDataFunction { command } => {
            user_data_function::execute(&cli, &client, command).await
        }
        Command::VariableLibrary { command } => {
            variable_library::execute(&cli, &client, command).await
        }
        Command::Map { command } => map::execute(&cli, &client, command).await,
        Command::Plan { command } => plan::execute(&cli, &client, command).await,
        Command::GraphQuerySet { command } => {
            graph_query_set::execute(&cli, &client, command).await
        }
        Command::GraphModel { command } => graph_model::execute(&cli, &client, command).await,
        Command::MirroredCatalog { command } => {
            mirrored_catalog::execute(&cli, &client, command).await
        }
        Command::MirroredDatabricksCatalog { command } => {
            mirrored_databricks_catalog::execute(&cli, &client, command).await
        }
        Command::WarehouseSnapshot { command } => {
            warehouse_snapshot::execute(&cli, &client, command).await
        }
        Command::PaginatedReport { command } => {
            paginated_report::execute(&cli, &client, command).await
        }
        Command::Dashboard { command } => dashboard::execute(&cli, &client, command).await,
        Command::Datamart { command } => datamart::execute(&cli, &client, command).await,
        Command::MirroredWarehouse { command } => {
            mirrored_warehouse::execute(&cli, &client, command).await
        }
        Command::ApacheAirflowJob { command } => {
            apache_airflow_job::execute(&cli, &client, command).await
        }
        Command::AnomalyDetector { command } => {
            anomaly_detector::execute(&cli, &client, command).await
        }
        Command::AppBackend { command } => app_backend::execute(&cli, &client, command).await,
        Command::AzureDatabricksStorage { command } => {
            azure_databricks_storage::execute(&cli, &client, command).await
        }
        Command::DataBuildToolJob { command } => {
            data_build_tool_job::execute(&cli, &client, command).await
        }
        Command::OrgApp { command } => org_app::execute(&cli, &client, command).await,
        Command::OrgAppAudience { command } => {
            org_app_audience::execute(&cli, &client, command).await
        }
        // Integration
        Command::Gateway { command } => gateway::execute(&cli, &client, command).await,
        Command::Git { command } => git::execute(&cli, &client, command).await,
        Command::Connection { command } => connection::execute(&cli, &client, command).await,
        Command::DeploymentPipeline { command } => {
            deployment_pipeline::execute(&cli, &client, command).await
        }
        Command::Domain { command } => domain::execute(&cli, &client, command).await,
        Command::Deploy { command } => deploy::execute(&cli, &client, command).await,
        Command::JobScheduler { command } => job_scheduler::execute(&cli, &client, command).await,
        // Security & Governance
        Command::Label { command } => label::execute(&cli, &client, command).await,
        Command::OnelakeSecurity { command } => {
            onelake_security::execute(&cli, &client, command).await
        }
        Command::ManagedPrivateEndpoint { command } => {
            managed_private_endpoint::execute(&cli, &client, command).await
        }
        // Configuration
        Command::Auth { command } => auth::execute(&cli, command).await,
        Command::Profile { command } => profile::execute(&cli, command),
        Command::Jobs { command } => jobs::execute(&cli, command),
        Command::Feedback { command } => feedback::execute(&cli, command),
        Command::Lro { command } => lro::execute(&cli, &client, command).await,
        Command::Rest { command } => rest::execute(&cli, &client, command).await,
        Command::Rti { command } => rti::execute(&cli, &client, command).await,
        Command::Upgrade {
            check,
            target_version,
        } => upgrade::execute(&cli, *check, target_version.as_deref(), cli.force).await,
        Command::Completions { shell } => completions::execute(*shell),
        Command::Mcp { command } => mcp::execute(&cli, command).await,
    }
}

/// Resolve private link workspace ID from the active profile.
fn resolve_private_link_workspace(cli: &Cli) -> Option<String> {
    let store = profile::ProfileStore::load();
    let profile_name = cli.profile.as_deref().or(store.active.as_deref())?;
    let p = store.profiles.get(profile_name)?;
    p.private_link_workspace.clone()
}

/// Enforce `--enable-commands` and `--disable-commands` policies.
///
/// Command paths use dot notation: `workspace.create`, `lakehouse.list-tables`.
/// Parent paths allow/deny all children: `workspace` matches `workspace.create`, etc.
/// Glob patterns supported: `*.delete` matches any group's delete subcommand,
/// `kql-*` matches all groups starting with `kql-`.
/// Deny rules always override allow rules.
fn check_command_policy(cli: &Cli) -> Result<()> {
    let enable = cli.enable_commands.as_ref();
    let disable = cli.disable_commands.as_ref();

    // No policy configured — everything is allowed.
    if enable.is_none() && disable.is_none() {
        return Ok(());
    }

    // Extract the command path from the CLI (e.g., "workspace.create").
    let cmd_path = extract_command_path(cli);

    // Check deny list first (deny always wins).
    if let Some(deny_list) = disable {
        for rule in deny_list {
            if command_pattern_matches(&cmd_path, rule) {
                return Err(crate::errors::FabioError::with_hint(
                    crate::errors::ErrorCode::Forbidden,
                    format!("Command '{cmd_path}' is blocked by --disable-commands policy"),
                    format!(
                        "This command is in the deny list. Allowed commands can be \
                         inspected via: fabio context agent. Blocked rule: {rule}"
                    ),
                )
                .into());
            }
        }
    }

    // Check allow list (if present, only listed commands are permitted).
    if let Some(allow_list) = enable {
        let allowed = allow_list
            .iter()
            .any(|rule| command_pattern_matches(&cmd_path, rule));
        if !allowed {
            return Err(crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::Forbidden,
                format!("Command '{cmd_path}' is not in the --enable-commands allowlist"),
                format!("Only these commands are allowed: {}", allow_list.join(", ")),
            )
            .into());
        }
    }

    Ok(())
}

/// Match a command path against a pattern (case-insensitive, dot separator).
///
/// Patterns:
/// - `workspace` — matches `workspace` and `workspace.*` (group-level)
/// - `workspace.create` — matches exactly `workspace.create`
/// - `*.delete` — matches `<any-group>.delete`
/// - `kql-*` — matches any group starting with `kql-` (all subcommands)
/// - `kql-*.query` — matches `kql-<anything>.query`
/// - `*` — matches everything
pub fn command_pattern_matches(cmd_path: &str, pattern: &str) -> bool {
    command_pattern_matches_sep(cmd_path, pattern, '.')
}

/// Match a command path against a pattern using a custom separator.
/// Used by both CLI (dot separator) and MCP (underscore separator) matching.
pub fn command_pattern_matches_sep(cmd_path: &str, pattern: &str, sep: char) -> bool {
    let cmd_lower = cmd_path.to_lowercase();
    let pat_lower = pattern.to_lowercase();

    // Wildcard-all.
    if pat_lower == "*" {
        return true;
    }

    // Split both into group + subcommand parts.
    let (cmd_group, cmd_sub) = split_first(sep, &cmd_lower);
    let (pat_group, pat_sub) = split_first(sep, &pat_lower);

    // Match the group segment (supports * as wildcard).
    if !segment_matches(cmd_group, pat_group) {
        return false;
    }

    // If pattern has no subcommand part, it's a group-level match (all subcommands).
    if pat_sub.is_empty() {
        return true;
    }

    // Match the subcommand segment (which may itself contain separators).
    segment_matches(cmd_sub, pat_sub)
}

/// Split a string at the first occurrence of `sep`. Returns `(before, after)`.
/// If `sep` is not found, returns the full string as `before` with empty `after`.
fn split_first(sep: char, s: &str) -> (&str, &str) {
    s.split_once(sep).unwrap_or((s, ""))
}

/// Match a single segment against a pattern with `*` as wildcard.
/// `*` matches any sequence of characters; only one `*` is supported per segment.
pub(in crate::commands) fn segment_matches(value: &str, pattern: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if let Some((prefix, suffix)) = pattern.split_once('*') {
        value.starts_with(prefix)
            && value.ends_with(suffix)
            && value.len() >= prefix.len() + suffix.len()
    } else {
        value == pattern
    }
}

/// Extract the dotted command path from the Cli struct (e.g., "workspace.create").
///
/// Parses raw CLI arguments to find the group and subcommand names, skipping
/// global flags (which start with `-`). Returns `"group"` if no subcommand is
/// found, or `"group.subcommand"` for full paths.
#[allow(clippy::too_many_lines)]
fn extract_command_path(cli: &Cli) -> String {
    // Fast path: for commands that don't have subcommands, just return the group.
    let group = match &cli.command {
        Command::Admin { .. } => "admin",
        Command::Workspace { .. } => "workspace",
        Command::Item { .. } => "item",
        Command::Lakehouse { .. } => "lakehouse",
        Command::Capacity { .. } => "capacity",
        Command::Catalog { .. } => "catalog",
        Command::Context { .. } => "context",
        Command::Notebook { .. } => "notebook",
        Command::Warehouse { .. } => "warehouse",
        Command::SqlDatabase { .. } => "sql-database",
        Command::SqlEndpoint { .. } => "sql-endpoint",
        Command::DataAgent { .. } => "data-agent",
        Command::Ontology { .. } => "ontology",
        Command::Environment { .. } => "environment",
        Command::DataPipeline { .. } => "data-pipeline",
        Command::CopyJob { .. } => "copy-job",
        Command::Dataflow { .. } => "dataflow",
        Command::Report { .. } => "report",
        Command::SemanticModel { .. } => "semantic-model",
        Command::Eventhouse { .. } => "eventhouse",
        Command::Eventstream { .. } => "eventstream",
        Command::KqlDatabase { .. } => "kql-database",
        Command::KqlQueryset { .. } => "kql-queryset",
        Command::KqlDashboard { .. } => "kql-dashboard",
        Command::MirroredDatabase { .. } => "mirrored-database",
        Command::Reflex { .. } => "reflex",
        Command::MlModel { .. } => "ml-model",
        Command::MlExperiment { .. } => "ml-experiment",
        Command::Spark { .. } => "spark",
        Command::SparkJobDefinition { .. } => "spark-job-definition",
        Command::GraphqlApi { .. } => "graphql-api",
        Command::GraphModel { .. } => "graph-model",
        Command::GraphQuerySet { .. } => "graph-query-set",
        Command::DigitalTwinBuilder { .. } => "digital-twin-builder",
        Command::DigitalTwinBuilderFlow { .. } => "digital-twin-builder-flow",
        Command::CosmosDbDatabase { .. } => "cosmos-db-database",
        Command::SnowflakeDatabase { .. } => "snowflake-database",
        Command::Map { .. } => "map",
        Command::Plan { .. } => "plan",
        Command::Connection { .. } => "connection",
        Command::DeploymentPipeline { .. } => "deployment-pipeline",
        Command::Domain { .. } => "domain",
        Command::Deploy { .. } => "deploy",
        Command::Gateway { .. } => "gateway",
        Command::Git { .. } => "git",
        Command::JobScheduler { .. } => "job-scheduler",
        Command::OnelakeSecurity { .. } => "onelake-security",
        Command::Label { .. } => "label",
        Command::ManagedPrivateEndpoint { .. } => "managed-private-endpoint",
        Command::Auth { .. } => "auth",
        Command::Profile { .. } => "profile",
        Command::Jobs { .. } => "jobs",
        Command::Feedback { .. } => "feedback",
        Command::Lro { .. } => "operation",
        Command::Rest { .. } => "rest",
        Command::Rti { .. } => "rti",
        Command::Upgrade { .. } => return "upgrade".to_owned(),
        Command::Completions { .. } => return "completions".to_owned(),
        Command::Mcp { .. } => "mcp",
        Command::VariableLibrary { .. } => "variable-library",
        Command::EventSchemaSet { .. } => "event-schema-set",
        Command::UserDataFunction { .. } => "user-data-function",
        Command::OperationsAgent { .. } => "operations-agent",
        Command::MountedDataFactory { .. } => "mounted-data-factory",
        Command::PaginatedReport { .. } => "paginated-report",
        Command::Dashboard { .. } => "dashboard",
        Command::Datamart { .. } => "datamart",
        Command::WarehouseSnapshot { .. } => "warehouse-snapshot",
        Command::MirroredCatalog { .. } => "mirrored-catalog",
        Command::MirroredDatabricksCatalog { .. } => "mirrored-databricks-catalog",
        Command::MirroredWarehouse { .. } => "mirrored-warehouse",
        Command::ApacheAirflowJob { .. } => "apache-airflow-job",
        Command::AnomalyDetector { .. } => "anomaly-detector",
        Command::AppBackend { .. } => "app-backend",
        Command::AzureDatabricksStorage { .. } => "azure-databricks-storage",
        Command::DataBuildToolJob { .. } => "data-build-tool-job",
        Command::OrgApp { .. } => "org-app",
        Command::OrgAppAudience { .. } => "org-app-audience",
    };

    // Extract subcommand from raw args by finding the group name (which we
    // already know from the enum) followed by the subcommand.
    // We skip flags and their values to avoid matching flag values like
    // "--disable-commands workspace" as the group positional.
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1; // skip argv[0]
    let mut found_group = false;
    while i < args.len() {
        let arg = &args[i];
        if arg.starts_with("--") {
            // Long flag: skip it and its value (next non-flag arg)
            i += 1;
            if i < args.len() && !args[i].starts_with('-') {
                i += 1;
            }
            continue;
        }
        if arg.starts_with('-') {
            i += 1;
            continue;
        }
        // Positional arg
        if found_group {
            return format!("{group}.{}", arg.to_lowercase());
        }
        if arg == group {
            found_group = true;
        }
        i += 1;
    }

    group.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── command_pattern_matches (dot separator) ─────────────────────────────

    #[test]
    fn pattern_wildcard_matches_everything() {
        assert!(command_pattern_matches("workspace.list", "*"));
        assert!(command_pattern_matches("kql-database.query", "*"));
    }

    #[test]
    fn pattern_group_only_matches_all_subcommands() {
        assert!(command_pattern_matches("workspace.list", "workspace"));
        assert!(command_pattern_matches("workspace.create", "workspace"));
        assert!(command_pattern_matches("workspace.delete", "workspace"));
    }

    #[test]
    fn pattern_group_only_does_not_match_other_groups() {
        assert!(!command_pattern_matches("lakehouse.list", "workspace"));
    }

    #[test]
    fn pattern_exact_subcommand_matches() {
        assert!(command_pattern_matches(
            "workspace.delete",
            "workspace.delete"
        ));
    }

    #[test]
    fn pattern_exact_subcommand_does_not_match_other() {
        assert!(!command_pattern_matches(
            "workspace.list",
            "workspace.delete"
        ));
    }

    #[test]
    fn pattern_star_dot_subcommand_matches_any_group() {
        assert!(command_pattern_matches("workspace.delete", "*.delete"));
        assert!(command_pattern_matches("lakehouse.delete", "*.delete"));
        assert!(command_pattern_matches("kql-database.delete", "*.delete"));
    }

    #[test]
    fn pattern_star_dot_subcommand_does_not_match_other_sub() {
        assert!(!command_pattern_matches("workspace.list", "*.delete"));
    }

    #[test]
    fn pattern_group_prefix_glob_matches() {
        assert!(command_pattern_matches("kql-database.query", "kql-*"));
        assert!(command_pattern_matches("kql-queryset.list", "kql-*"));
        assert!(command_pattern_matches("kql-dashboard.show", "kql-*"));
    }

    #[test]
    fn pattern_group_prefix_glob_does_not_match_unrelated() {
        assert!(!command_pattern_matches("workspace.list", "kql-*"));
    }

    #[test]
    fn pattern_group_glob_with_subcommand() {
        assert!(command_pattern_matches("kql-database.query", "kql-*.query"));
        assert!(!command_pattern_matches("kql-database.list", "kql-*.query"));
    }

    #[test]
    fn pattern_case_insensitive() {
        assert!(command_pattern_matches("Workspace.List", "workspace.list"));
        assert!(command_pattern_matches("workspace.list", "WORKSPACE.LIST"));
        assert!(command_pattern_matches("KQL-Database.Query", "*.query"));
    }

    #[test]
    fn pattern_multi_word_subcommand() {
        assert!(command_pattern_matches(
            "lakehouse.list-tables",
            "lakehouse.list-tables"
        ));
        assert!(command_pattern_matches("lakehouse.list-tables", "*.list-*"));
    }

    #[test]
    fn pattern_subcommand_suffix_glob() {
        // Match all get-definition commands
        assert!(command_pattern_matches(
            "notebook.get-definition",
            "*.get-definition"
        ));
        assert!(command_pattern_matches(
            "lakehouse.get-definition",
            "*.get-definition"
        ));
    }

    // ─── segment_matches ─────────────────────────────────────────────────────

    #[test]
    fn segment_star_matches_anything() {
        assert!(segment_matches("hello", "*"));
        assert!(segment_matches("", "*"));
    }

    #[test]
    fn segment_exact_match() {
        assert!(segment_matches("workspace", "workspace"));
        assert!(!segment_matches("workspace", "lakehouse"));
    }

    #[test]
    fn segment_prefix_star() {
        assert!(segment_matches("kql-database", "kql-*"));
        assert!(segment_matches("kql-queryset", "kql-*"));
        assert!(!segment_matches("workspace", "kql-*"));
    }

    #[test]
    fn segment_suffix_star() {
        assert!(segment_matches("get-definition", "*-definition"));
        assert!(!segment_matches("get-definition", "*-tables"));
    }

    #[test]
    fn segment_infix_star() {
        assert!(segment_matches("list-tables", "list-*s"));
        assert!(segment_matches("list-files", "list-*s"));
        assert!(!segment_matches("list-file", "list-*s"));
    }

    // ─── command_pattern_matches_sep (underscore separator for MCP) ──────────

    #[test]
    fn sep_underscore_group_only() {
        assert!(command_pattern_matches_sep(
            "workspace_list",
            "workspace",
            '_'
        ));
        assert!(command_pattern_matches_sep(
            "workspace_create",
            "workspace",
            '_'
        ));
    }

    #[test]
    fn sep_underscore_wildcard() {
        assert!(command_pattern_matches_sep("anything_here", "*", '_'));
    }
}
