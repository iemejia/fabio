use clap::{Parser, Subcommand, ValueEnum};

use crate::commands::{
    admin, anomaly_detector, apache_airflow_job, app_backend, auth, capacity, catalog, connection,
    copy_job, cosmos_db_database, dashboard, data_build_tool_job, data_pipeline, dataagent,
    dataflow, datamart, deploy, deployment_pipeline, digital_twin_builder,
    digital_twin_builder_flow, domain, environment, event_schema_set, eventhouse, eventstream,
    feedback, gateway, git, graph_model, graph_query_set, graphql_api, item, job_scheduler, jobs,
    kql_dashboard, kql_database, kql_queryset, lakehouse, lro, managed_private_endpoint, map,
    mirrored_catalog, mirrored_database, mirrored_databricks_catalog, mirrored_warehouse,
    ml_experiment, ml_model, mounted_data_factory, notebook, onelake_security, ontology,
    operations_agent, org_app, org_app_audience, paginated_report, profile, reflex, report, rest,
    rti, semantic_model, snowflake_database, spark, spark_job_definition, sql_database,
    sql_endpoint, user_data_function, variable_library, warehouse, warehouse_snapshot, workspace,
};

/// Agent-first CLI for managing Microsoft Fabric artifacts and data.
///
/// Structured JSON output by default. Designed for composability via stdin/stdout.
#[derive(Parser, Debug)]
#[command(name = "fabio", version, about, long_about = None)]
#[command(propagate_version = true)]
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Output format
    #[arg(
        short,
        long,
        global = true,
        default_value = "json",
        env = "FABIO_OUTPUT"
    )]
    pub output: OutputFormat,

    /// Shorthand for --output json (agent-native convention)
    #[arg(long, global = true)]
    pub json: bool,

    /// `JMESPath` query expression (see <https://jmespath.org/>)
    #[arg(short, long, global = true)]
    pub query: Option<String>,

    /// Suppress all output
    #[arg(long, global = true)]
    pub quiet: bool,

    /// Skip confirmation prompts (for destructive operations)
    #[arg(long, global = true)]
    pub force: bool,

    /// Preview what would happen without making changes
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Maximum number of items to return in list commands
    #[arg(long, global = true)]
    pub limit: Option<usize>,

    /// Fetch all pages (auto-paginate). Without this, only the first page is returned.
    #[arg(long, global = true)]
    pub all: bool,

    /// Resume pagination from a specific continuation token (returned by a previous list call)
    #[arg(long, global = true)]
    pub continuation_token: Option<String>,

    /// Use a named profile for default settings
    #[arg(long, global = true)]
    pub profile: Option<String>,

    /// Enable verbose HTTP diagnostics on stderr (request/response tracing)
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// Maximum seconds to wait for long-running operations (default: 120)
    #[arg(long, global = true)]
    pub lro_timeout: Option<u64>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// Returns the effective output format, considering --json shorthand.
    pub const fn effective_output(&self) -> &OutputFormat {
        if self.json {
            &OutputFormat::Json
        } else {
            &self.output
        }
    }
}

#[derive(Debug, Clone, ValueEnum)]
pub enum OutputFormat {
    Json,
    Table,
    Plain,
    Csv,
    Tsv,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    // ── Admin ────────────────────────────────────────────────────────────
    /// Fabric tenant administration (settings, tags, workloads, users)
    #[command(display_order = 55)]
    Admin {
        #[command(subcommand)]
        command: admin::AdminCommand,
    },

    // ── Core ─────────────────────────────────────────────────────────────
    /// Manage workspaces
    #[command(display_order = 1)]
    Workspace {
        #[command(subcommand)]
        command: workspace::WorkspaceCommand,
    },
    /// Manage items (datasets, reports, notebooks, etc.)
    #[command(display_order = 2)]
    Item {
        #[command(subcommand)]
        command: item::ItemCommand,
    },
    /// Manage lakehouses (tables, files, shortcuts)
    #[command(display_order = 3)]
    Lakehouse {
        #[command(subcommand)]
        command: lakehouse::LakehouseCommand,
    },
    /// List and inspect Fabric capacities
    #[command(display_order = 4)]
    Capacity {
        #[command(subcommand)]
        command: capacity::CapacityCommand,
    },
    /// Search the Fabric catalog
    #[command(display_order = 5)]
    Catalog {
        #[command(subcommand)]
        command: catalog::CatalogCommand,
    },

    // ── Data & Compute ───────────────────────────────────────────────────
    /// Manage notebooks
    #[command(display_order = 10)]
    Notebook {
        #[command(subcommand)]
        command: notebook::NotebookCommand,
    },
    /// Manage warehouses and run SQL queries
    #[command(display_order = 11)]
    Warehouse {
        #[command(subcommand)]
        command: warehouse::WarehouseCommand,
    },
    /// Manage SQL databases (Fabric-native transactional databases)
    #[command(display_order = 12)]
    SqlDatabase {
        #[command(subcommand)]
        command: sql_database::SqlDatabaseCommand,
    },
    /// Manage SQL endpoints (analytics endpoints for lakehouses)
    #[command(display_order = 13)]
    SqlEndpoint {
        #[command(subcommand)]
        command: sql_endpoint::SqlEndpointCommand,
    },
    /// Manage data agents (create, query, and interact with AI agents)
    #[command(display_order = 12)]
    DataAgent {
        #[command(subcommand)]
        command: dataagent::DataAgentCommand,
    },
    /// Manage ontologies (entity types, data bindings)
    #[command(display_order = 13)]
    Ontology {
        #[command(subcommand)]
        command: ontology::OntologyCommand,
    },
    /// Manage environments (Spark compute, libraries, publish)
    #[command(display_order = 14)]
    Environment {
        #[command(subcommand)]
        command: environment::EnvironmentCommand,
    },
    /// Manage data pipelines (orchestration, scheduling)
    #[command(display_order = 15)]
    DataPipeline {
        #[command(subcommand)]
        command: data_pipeline::DataPipelineCommand,
    },
    /// Manage copy jobs (data movement)
    #[command(display_order = 16)]
    CopyJob {
        #[command(subcommand)]
        command: copy_job::CopyJobCommand,
    },
    /// Manage dataflows (Power BI data transformation)
    #[command(display_order = 17)]
    Dataflow {
        #[command(subcommand)]
        command: dataflow::DataflowCommand,
    },
    /// Manage reports (Power BI)
    #[command(display_order = 18)]
    Report {
        #[command(subcommand)]
        command: report::ReportCommand,
    },
    /// Manage semantic models (Power BI datasets)
    #[command(display_order = 19)]
    SemanticModel {
        #[command(subcommand)]
        command: semantic_model::SemanticModelCommand,
    },
    /// Manage eventhouses (real-time analytics)
    #[command(display_order = 20)]
    Eventhouse {
        #[command(subcommand)]
        command: eventhouse::EventhouseCommand,
    },
    /// Manage eventstreams (real-time data ingestion)
    #[command(display_order = 21)]
    Eventstream {
        #[command(subcommand)]
        command: eventstream::EventstreamCommand,
    },
    /// Manage KQL databases (within eventhouses)
    #[command(display_order = 22)]
    KqlDatabase {
        #[command(subcommand)]
        command: kql_database::KqlDatabaseCommand,
    },
    /// Manage KQL querysets (saved KQL queries)
    #[command(display_order = 23)]
    KqlQueryset {
        #[command(subcommand)]
        command: kql_queryset::KqlQuerysetCommand,
    },
    /// Manage KQL dashboards (real-time dashboards)
    #[command(display_order = 24)]
    KqlDashboard {
        #[command(subcommand)]
        command: kql_dashboard::KqlDashboardCommand,
    },
    /// Real-Time Intelligence copilot (NL-to-KQL)
    #[command(display_order = 24)]
    Rti {
        #[command(subcommand)]
        command: rti::RtiCommand,
    },
    /// Manage mirrored databases (real-time replication)
    #[command(display_order = 25)]
    MirroredDatabase {
        #[command(subcommand)]
        command: mirrored_database::MirroredDatabaseCommand,
    },
    /// Manage Reflex items (Data Activator triggers and alerts)
    #[command(display_order = 26)]
    Reflex {
        #[command(subcommand)]
        command: reflex::ReflexCommand,
    },
    /// Manage ML models (data science)
    #[command(display_order = 27)]
    MlModel {
        #[command(subcommand)]
        command: ml_model::MlModelCommand,
    },
    /// Manage ML experiments (data science)
    #[command(display_order = 28)]
    MlExperiment {
        #[command(subcommand)]
        command: ml_experiment::MlExperimentCommand,
    },
    /// Manage Spark compute (settings, custom pools)
    #[command(display_order = 29)]
    Spark {
        #[command(subcommand)]
        command: spark::SparkCommand,
    },
    /// Manage Spark job definitions (batch Spark jobs)
    #[command(display_order = 30)]
    SparkJobDefinition {
        #[command(subcommand)]
        command: spark_job_definition::SparkJobDefinitionCommand,
    },
    /// Manage GraphQL APIs
    #[command(display_order = 31)]
    GraphqlApi {
        #[command(subcommand)]
        command: graphql_api::GraphqlApiCommand,
    },
    /// Manage Cosmos DB databases (mirrored from Azure Cosmos DB)
    #[command(display_order = 32)]
    CosmosDbDatabase {
        #[command(subcommand)]
        command: cosmos_db_database::CosmosDbDatabaseCommand,
    },
    /// Manage Snowflake databases (mirrored from Snowflake)
    #[command(display_order = 33)]
    SnowflakeDatabase {
        #[command(subcommand)]
        command: snowflake_database::SnowflakeDatabaseCommand,
    },
    /// Manage Digital Twin Builder models
    #[command(display_order = 34)]
    DigitalTwinBuilder {
        #[command(subcommand)]
        command: digital_twin_builder::DigitalTwinBuilderCommand,
    },
    /// Manage Digital Twin Builder flows
    #[command(display_order = 35)]
    DigitalTwinBuilderFlow {
        #[command(subcommand)]
        command: digital_twin_builder_flow::DigitalTwinBuilderFlowCommand,
    },
    /// Manage event schema sets (real-time intelligence)
    #[command(display_order = 36)]
    EventSchemaSet {
        #[command(subcommand)]
        command: event_schema_set::EventSchemaSetCommand,
    },
    /// Manage operations agents (AI-powered operations)
    #[command(display_order = 37)]
    OperationsAgent {
        #[command(subcommand)]
        command: operations_agent::OperationsAgentCommand,
    },
    /// Manage Mounted Data Factories (ADF integration)
    #[command(display_order = 38)]
    MountedDataFactory {
        #[command(subcommand)]
        command: mounted_data_factory::MountedDataFactoryCommand,
    },
    /// Manage user data functions
    #[command(display_order = 39)]
    UserDataFunction {
        #[command(subcommand)]
        command: user_data_function::UserDataFunctionCommand,
    },
    /// Manage variable libraries (shared variables)
    #[command(visible_alias = "var-lib", display_order = 46)]
    VariableLibrary {
        #[command(subcommand)]
        command: variable_library::VariableLibraryCommand,
    },
    /// Manage maps (geospatial)
    #[command(display_order = 47)]
    Map {
        #[command(subcommand)]
        command: map::MapCommand,
    },
    /// Manage graph query sets
    #[command(display_order = 48)]
    GraphQuerySet {
        #[command(subcommand)]
        command: graph_query_set::GraphQuerySetCommand,
    },
    /// Manage graph models (knowledge graph)
    #[command(display_order = 49)]
    GraphModel {
        #[command(subcommand)]
        command: graph_model::GraphModelCommand,
    },
    /// Manage mirrored catalogs (Unity Catalog mirroring)
    #[command(display_order = 52)]
    MirroredCatalog {
        #[command(subcommand)]
        command: mirrored_catalog::MirroredCatalogCommand,
    },
    /// Manage mirrored Azure Databricks catalogs
    #[command(display_order = 53)]
    MirroredDatabricksCatalog {
        #[command(subcommand)]
        command: mirrored_databricks_catalog::MirroredDatabricksCatalogCommand,
    },
    /// Manage warehouse snapshots
    #[command(display_order = 54)]
    WarehouseSnapshot {
        #[command(subcommand)]
        command: warehouse_snapshot::WarehouseSnapshotCommand,
    },
    /// Manage paginated reports
    #[command(display_order = 55)]
    PaginatedReport {
        #[command(subcommand)]
        command: paginated_report::PaginatedReportCommand,
    },
    /// Manage dashboards (Power BI)
    #[command(display_order = 56)]
    Dashboard {
        #[command(subcommand)]
        command: dashboard::DashboardCommand,
    },
    /// Manage datamarts (Power BI)
    #[command(display_order = 57)]
    Datamart {
        #[command(subcommand)]
        command: datamart::DatamartCommand,
    },
    /// Manage mirrored warehouses
    #[command(display_order = 58)]
    MirroredWarehouse {
        #[command(subcommand)]
        command: mirrored_warehouse::MirroredWarehouseCommand,
    },
    /// Manage anomaly detectors
    #[command(display_order = 59)]
    AnomalyDetector {
        #[command(subcommand)]
        command: anomaly_detector::AnomalyDetectorCommand,
    },
    /// Manage Apache Airflow jobs (DAGs, environments, pools)
    #[command(display_order = 60)]
    ApacheAirflowJob {
        #[command(subcommand)]
        command: apache_airflow_job::ApacheAirflowJobCommand,
    },
    /// Manage app backends (Power Apps backend services) [preview]
    #[command(name = "app-backend", display_order = 61)]
    AppBackend {
        #[command(subcommand)]
        command: app_backend::AppBackendCommand,
    },
    /// Manage data build tool jobs (dbt-style transformations) [preview]
    #[command(name = "data-build-tool-job", display_order = 62)]
    DataBuildToolJob {
        #[command(subcommand)]
        command: data_build_tool_job::DataBuildToolJobCommand,
    },
    /// Manage org apps (organizational Power Apps)
    #[command(name = "org-app", display_order = 63)]
    OrgApp {
        #[command(subcommand)]
        command: org_app::OrgAppCommand,
    },
    /// Manage org app audiences (audience definitions for org apps)
    #[command(name = "org-app-audience", display_order = 64)]
    OrgAppAudience {
        #[command(subcommand)]
        command: org_app_audience::OrgAppAudienceCommand,
    },

    // ── Integration ──────────────────────────────────────────────────────
    /// Manage gateways (on-premises, `VNet`, members, role assignments)
    #[command(display_order = 45)]
    Gateway {
        #[command(subcommand)]
        command: gateway::GatewayCommand,
    },
    /// Manage Git integration (connect, commit, pull, status)
    #[command(display_order = 40)]
    Git {
        #[command(subcommand)]
        command: git::GitCommand,
    },
    /// Manage connections (cloud, on-premises, virtual network)
    #[command(display_order = 41)]
    Connection {
        #[command(subcommand)]
        command: connection::ConnectionCommand,
    },
    /// Manage deployment pipelines (CI/CD stages, deploy items)
    #[command(display_order = 42)]
    DeploymentPipeline {
        #[command(subcommand)]
        command: deployment_pipeline::DeploymentPipelineCommand,
    },
    /// Manage domains (organize workspaces into business domains)
    #[command(display_order = 43)]
    Domain {
        #[command(subcommand)]
        command: domain::DomainCommand,
    },
    /// Deploy item definitions from a local directory to a workspace
    #[command(display_order = 44)]
    Deploy {
        #[command(subcommand)]
        command: deploy::DeployCommand,
    },
    /// Manage item job scheduling (run, cancel, schedules)
    #[command(display_order = 45)]
    JobScheduler {
        #[command(subcommand)]
        command: job_scheduler::JobSchedulerCommand,
    },

    // ── Security & Governance ────────────────────────────────────────────
    /// Manage `OneLake` data access roles (row/column-level security)
    #[command(display_order = 50)]
    OnelakeSecurity {
        #[command(subcommand)]
        command: onelake_security::OnelakeSecurityCommand,
    },
    /// Manage workspace managed private endpoints
    #[command(display_order = 51)]
    ManagedPrivateEndpoint {
        #[command(subcommand)]
        command: managed_private_endpoint::ManagedPrivateEndpointCommand,
    },

    // ── Configuration ────────────────────────────────────────────────────
    /// Manage authentication
    #[command(display_order = 60)]
    Auth {
        #[command(subcommand)]
        command: auth::AuthCommand,
    },
    /// Manage saved configuration profiles
    #[command(display_order = 61)]
    Profile {
        #[command(subcommand)]
        command: profile::ProfileCommand,
    },
    /// Inspect and manage async job history
    #[command(display_order = 62)]
    Jobs {
        #[command(subcommand)]
        command: jobs::JobsCommand,
    },
    /// Report CLI friction or issues for improvement
    #[command(display_order = 63)]
    Feedback {
        #[command(subcommand)]
        command: feedback::FeedbackCommand,
    },
    /// Check long-running operation status and results
    #[command(name = "operation", display_order = 63)]
    Lro {
        #[command(subcommand)]
        command: lro::LroCommand,
    },
    /// Send raw REST requests to the Fabric API (like `az rest`)
    #[command(display_order = 64)]
    Rest {
        #[command(subcommand)]
        command: rest::RestCommand,
    },
    /// Machine-readable CLI schema for agent introspection
    #[command(name = "agent-context", display_order = 65)]
    AgentContext,
    /// Generate shell completion scripts
    #[command(display_order = 66)]
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}
