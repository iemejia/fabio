pub mod admin;
pub mod anomaly_detector;
pub mod apache_airflow_job;
pub mod app_backend;
pub mod auth;
pub mod capacity;
pub mod catalog;
pub mod connection;
pub mod copy_job;
pub mod cosmos_db_database;
pub mod dashboard;
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
pub mod lakehouse;
pub mod lro;
pub mod managed_private_endpoint;
pub mod map;
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
pub mod paginated_report;
pub mod profile;
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

mod agent_context;
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
    let mut client = FabricClient::new();

    // Apply LRO timeout from --lro-timeout flag if specified
    if let Some(timeout_secs) = cli.lro_timeout {
        client = client.with_lro_timeout(std::time::Duration::from_secs(timeout_secs));
    }

    // Apply private link routing from profile if configured
    if let Some(ws_id) = resolve_private_link_workspace(&cli) {
        client = client.with_private_link(ws_id);
    }

    match &cli.command {
        // Admin
        Command::Admin { command } => admin::execute(&cli, &client, command).await,
        // Core
        Command::Workspace { command } => workspace::execute(&cli, &client, command).await,
        Command::Item { command } => item::execute(&cli, &client, command).await,
        Command::Lakehouse { command } => lakehouse::execute(&cli, &client, command).await,
        Command::Capacity { command } => capacity::execute(&cli, &client, command).await,
        Command::Catalog { command } => catalog::execute(&cli, &client, command).await,
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
        Command::AgentContext => agent_context::execute(&cli),
        Command::Completions { shell } => completions::execute(*shell),
    }
}

/// Resolve private link workspace ID from the active profile.
fn resolve_private_link_workspace(cli: &Cli) -> Option<String> {
    let store = profile::ProfileStore::load();
    let profile_name = cli.profile.as_deref().or(store.active.as_deref())?;
    let p = store.profiles.get(profile_name)?;
    p.private_link_workspace.clone()
}
