//! Fabric ontology (preview) commands: knowledge-graph schema (entity/relationship
//! types + data bindings), OWL/JSON-LD import & export, and MCP consumption.

use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{enrich_forbidden, enrich_ontology_definition_error};

mod crud;
mod definitions;
pub mod import;
mod mcp;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples ontology\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum OntologyCommand {
    /// List ontologies in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an ontology
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,
    },
    /// Create an ontology
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name (must start with letter, alphanumeric/underscore, <100 chars)
        #[arg(long)]
        name: String,

        /// Description (max 256 characters)
        #[arg(long)]
        description: Option<String>,

        /// Path to definition JSON file (base64-encoded parts format)
        #[arg(long, conflicts_with_all = ["file", "dir"])]
        definition: Option<String>,

        /// Path to a local RDF file (.ttl, .owl, .rdf, .jsonld, .nt, .n3, .trig)
        /// Auto-detects format from extension and wraps into Fabric definition
        #[arg(long, conflicts_with_all = ["definition", "dir"])]
        file: Option<String>,

        /// Path to a directory containing Fabric ontology definition structure
        /// (`EntityTypes/`, `RelationshipTypes/` with definition.json, `DataBindings/`, etc.)
        #[arg(long, conflicts_with_all = ["definition", "file"])]
        dir: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update ontology properties (name and/or description)
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an ontology
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard: bool,
    },
    /// Get the ontology definition (entity types, bindings)
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// Definition format
        #[arg(long)]
        format: Option<String>,

        /// Decode base64 payloads in definition parts to readable JSON/text
        #[arg(long)]
        decode: bool,
    },
    /// Update the ontology definition (replaces current definition)
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// Path to definition JSON file, or - for stdin
        #[arg(long, conflicts_with_all = ["file", "dir"])]
        definition: Option<String>,

        /// Path to a local RDF file (.ttl, .owl, .rdf, .jsonld, .nt, .n3, .trig)
        /// Auto-detects format from extension and wraps into Fabric definition
        #[arg(long, conflicts_with_all = ["definition", "dir"])]
        file: Option<String>,

        /// Path to a directory containing Fabric ontology definition structure
        /// (`EntityTypes/`, `RelationshipTypes/` with definition.json, `DataBindings/`, etc.)
        #[arg(long, conflicts_with_all = ["definition", "file"])]
        dir: Option<String>,

        /// Also update item metadata from .platform file
        #[arg(long)]
        update_metadata: bool,
    },
    /// Import an OWL ontology (RDF/XML or JSON-LD) and convert to Fabric format
    ///
    /// Parses `owl:Class` to `EntityTypes`, `DatatypeProperties` to properties,
    /// `ObjectProperties` to `RelationshipTypes`. Compatible with Ontology Playground
    /// catalogue `.rdf` files.
    #[command(display_order = 10)]
    Import {
        /// Workspace ID (push to Fabric; omit for local export only)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: Option<String>,

        /// Ontology ID (required when pushing to Fabric)
        #[arg(long)]
        id: Option<String>,

        /// Path to OWL file (.rdf, .owl for RDF/XML; .jsonld for JSON-LD)
        #[arg(long)]
        file: String,

        /// Export converted definition to a local directory
        #[arg(long)]
        output_dir: Option<String>,

        /// Lakehouse item ID for the default data source. When set, generates
        /// `DataBindings` (and `Contextualizations`, given `--bindings`) so the
        /// imported ontology is queryable, not just a bare schema. Eventhouse /
        /// `TimeSeries` / composite sources are configured via `--bindings`.
        #[arg(long)]
        lakehouse: Option<String>,

        /// Workspace ID that hosts the Lakehouse default source (defaults to --workspace)
        #[arg(long)]
        lakehouse_workspace: Option<String>,

        /// Schema for the Lakehouse default-source tables (default: dbo)
        #[arg(long)]
        lakehouse_schema: Option<String>,

        /// Eventhouse item ID for a `KustoTable` default source (`TimeSeries`).
        /// Requires --cluster-uri, --database, and --timestamp-column.
        #[arg(long, conflicts_with = "lakehouse")]
        eventhouse: Option<String>,

        /// Workspace ID that hosts the Eventhouse default source (defaults to --workspace)
        #[arg(long)]
        eventhouse_workspace: Option<String>,

        /// Kusto cluster query URI for the Eventhouse default source
        #[arg(long)]
        cluster_uri: Option<String>,

        /// KQL database name for the Eventhouse default source
        #[arg(long)]
        database: Option<String>,

        /// Timestamp column for `TimeSeries` bindings (Eventhouse default source)
        #[arg(long)]
        timestamp_column: Option<String>,

        /// Path to a JSON binding map that overrides table/column names,
        /// selects data sources, and supplies relationship key columns.
        /// See `fabio context examples ontology`.
        #[arg(long)]
        bindings: Option<String>,
    },
    /// Bind an existing ontology's types to data sources (no OWL re-import)
    ///
    /// Fetches the current definition, matches entity/relationship types by
    /// name, and adds `DataBindings` + `Contextualizations` in place. Use this
    /// to bind a portal-authored ontology or add bindings incrementally.
    #[command(display_order = 12)]
    Bind {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// Lakehouse item ID for the default data source
        #[arg(long)]
        lakehouse: Option<String>,

        /// Workspace ID that hosts the Lakehouse default source (defaults to --workspace)
        #[arg(long)]
        lakehouse_workspace: Option<String>,

        /// Schema for the Lakehouse default-source tables (default: dbo)
        #[arg(long)]
        lakehouse_schema: Option<String>,

        /// Eventhouse item ID for a `KustoTable` default source (`TimeSeries`).
        /// Requires --cluster-uri, --database, and --timestamp-column.
        #[arg(long, conflicts_with = "lakehouse")]
        eventhouse: Option<String>,

        /// Workspace ID that hosts the Eventhouse default source (defaults to --workspace)
        #[arg(long)]
        eventhouse_workspace: Option<String>,

        /// Kusto cluster query URI for the Eventhouse default source
        #[arg(long)]
        cluster_uri: Option<String>,

        /// KQL database name for the Eventhouse default source
        #[arg(long)]
        database: Option<String>,

        /// Timestamp column for `TimeSeries` bindings (Eventhouse default source)
        #[arg(long)]
        timestamp_column: Option<String>,

        /// Path to a JSON binding map (table/column overrides, data sources,
        /// relationship key columns). See `fabio context examples ontology`.
        #[arg(long)]
        bindings: Option<String>,
    },
    /// Export a Fabric Ontology to OWL format (RDF/XML or JSON-LD)
    ///
    /// Fetches the ontology definition from Fabric and converts `EntityTypes`
    /// and `RelationshipTypes` back to standard OWL. Compatible with Ontology
    /// Playground and standard RDF tools.
    #[command(display_order = 11)]
    Export {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,

        /// Output format: `rdf` (RDF/XML) or `jsonld` (JSON-LD)
        #[arg(long, default_value = "rdf", value_parser = ["rdf", "jsonld"])]
        format: String,

        /// Output file path (writes to stdout if omitted)
        #[arg(long)]
        file: Option<String>,
    },
    /// Print the Model Context Protocol (MCP) server URL for consuming this ontology
    McpUrl {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Ontology ID
        #[arg(long)]
        id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &OntologyCommand) -> Result<()> {
    match command {
        OntologyCommand::List { workspace } => crud::list(cli, client, workspace).await,
        OntologyCommand::Show { workspace, id } => crud::show(cli, client, workspace, id).await,
        OntologyCommand::Create {
            workspace,
            name,
            description,
            definition,
            file,
            dir,
            sensitivity_label,
        } => crud::create(
            cli,
            client,
            workspace,
            name,
            description.as_deref(),
            definition.as_deref(),
            file.as_deref(),
            dir.as_deref(),
            sensitivity_label.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology create", "Member"))
        .map_err(|e| enrich_ontology_definition_error(e, "ontology create")),
        OntologyCommand::Update {
            workspace,
            id,
            name,
            description,
        } => crud::update(
            cli,
            client,
            workspace,
            id,
            name.as_deref(),
            description.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology update", "Contributor")),
        OntologyCommand::Delete {
            workspace,
            id,
            hard,
        } => crud::delete(cli, client, workspace, id, *hard)
            .await
            .map_err(|e| enrich_forbidden(e, "ontology delete", "Member")),
        OntologyCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => {
            definitions::get_definition(cli, client, workspace, id, format.as_deref(), *decode)
                .await
        }
        OntologyCommand::UpdateDefinition {
            workspace,
            id,
            definition,
            file,
            dir,
            update_metadata,
        } => definitions::update_definition(
            cli,
            client,
            workspace,
            id,
            definition.as_deref(),
            file.as_deref(),
            dir.as_deref(),
            *update_metadata,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology update-definition", "Contributor"))
        .map_err(|e| enrich_ontology_definition_error(e, "ontology update-definition")),
        OntologyCommand::Import {
            workspace,
            id,
            file,
            output_dir,
            lakehouse,
            lakehouse_workspace,
            lakehouse_schema,
            eventhouse,
            eventhouse_workspace,
            cluster_uri,
            database,
            timestamp_column,
            bindings,
        } => {
            import::import_owl(
                cli,
                client,
                workspace.as_deref(),
                id.as_deref(),
                file,
                output_dir.as_deref(),
                lakehouse.as_deref(),
                lakehouse_workspace.as_deref(),
                lakehouse_schema.as_deref(),
                eventhouse.as_deref(),
                eventhouse_workspace.as_deref(),
                cluster_uri.as_deref(),
                database.as_deref(),
                timestamp_column.as_deref(),
                bindings.as_deref(),
            )
            .await
        }
        OntologyCommand::Bind {
            workspace,
            id,
            lakehouse,
            lakehouse_workspace,
            lakehouse_schema,
            eventhouse,
            eventhouse_workspace,
            cluster_uri,
            database,
            timestamp_column,
            bindings,
        } => import::bind_ontology(
            cli,
            client,
            workspace,
            id,
            lakehouse.as_deref(),
            lakehouse_workspace.as_deref(),
            lakehouse_schema.as_deref(),
            eventhouse.as_deref(),
            eventhouse_workspace.as_deref(),
            cluster_uri.as_deref(),
            database.as_deref(),
            timestamp_column.as_deref(),
            bindings.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "ontology bind", "Contributor")),
        OntologyCommand::Export {
            workspace,
            id,
            format,
            file,
        } => import::export_owl(cli, client, workspace, id, format, file.as_deref()).await,
        OntologyCommand::McpUrl { workspace, id } => mcp::mcp_url(cli, client, workspace, id)
            .await
            .map_err(|e| enrich_forbidden(e, "ontology mcp-url", "Viewer")),
    }
}

fn read_file_or_stdin(path: &str) -> Result<String> {
    if path == "-" {
        std::io::read_to_string(std::io::stdin())
            .map_err(|e| anyhow::anyhow!("Failed to read from stdin: {e}"))
    } else {
        std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))
    }
}
