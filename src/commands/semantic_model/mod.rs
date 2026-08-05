mod analyze;
mod authoring;
mod calc_groups;
mod columns;
mod crud;
mod definitions;
mod expressions;
mod functions;
mod generate;
mod hierarchies;
pub mod operations;
mod partitions;
mod perspectives;
mod powerbi;
mod relationships;
mod roles;
mod tables;
mod tmdl;
mod translations;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples semantic-model\nAlso available: fabio context schema SemanticModel | fabio context workflow direct-lake-report"
)]
pub enum SemanticModelCommand {
    /// List semantic models in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a semantic model
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Create a new semantic model from a definition file (model.bim)
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Path to a single model definition file (model.bim TMSL or a single .tmdl)
        #[arg(long, required_unless_present = "definition")]
        file: Option<String>,

        /// Path to a FULL model definition folder (a `.SemanticModel` folder or any
        /// folder with definition.pbism + definition/ TMDL files or model.bim). All
        /// files are gathered recursively — the way a real multi-file TMDL model ships.
        #[arg(long)]
        definition: Option<String>,

        /// SQL endpoint or lakehouse ID for live connection (generates definition.pbism)
        #[arg(long, visible_alias = "connection-id")]
        connection: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Generate a Direct Lake semantic model from a lakehouse or warehouse
    /// (reads the SQL analytics endpoint schema and picks tables, like the
    /// Fabric portal's "New semantic model")
    #[command(display_order = 3)]
    Generate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model display name
        #[arg(long)]
        name: String,

        /// Source lakehouse ID (mutually exclusive with --warehouse)
        #[arg(long, conflicts_with = "warehouse")]
        lakehouse: Option<String>,

        /// Source warehouse ID (mutually exclusive with --lakehouse)
        #[arg(long)]
        warehouse: Option<String>,

        /// Comma-separated table names to include (default: all base tables)
        #[arg(long)]
        tables: Option<String>,

        /// SQL schema to read tables from (default: dbo)
        #[arg(long, default_value = "dbo")]
        schema: String,

        /// Skip the framing refresh (you must run `semantic-model refresh` before querying)
        #[arg(long)]
        no_refresh: bool,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update semantic model properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a semantic model
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a semantic model
    #[command(name = "get-definition", display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a semantic model from a file
    #[command(name = "update-definition", display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Path to model definition file (model.bim TMSL/TMDL format)
        #[arg(long)]
        file: String,
    },
    /// Execute a DAX query against a semantic model
    #[command(display_order = 8)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// DAX query (e.g., "EVALUATE Sales"). If omitted, reads from stdin.
        #[arg(long)]
        dax: Option<String>,

        /// Read DAX query from a file
        #[arg(long, conflicts_with = "dax")]
        file: Option<String>,
    },
    /// Bind a semantic model to a connection
    #[command(name = "bind-connection", display_order = 10)]
    BindConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Connection ID to bind
        #[arg(long)]
        connection_id: String,
    },
    /// Unbind a connection from a semantic model
    #[command(name = "unbind-connection", display_order = 10)]
    UnbindConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Refresh a semantic model (required to frame Direct Lake models after creation)
    ///
    /// Basic refresh sends just --type. Passing --objects / --commit-mode /
    /// --max-parallelism / --retry-count triggers an ENHANCED refresh (the TMSL
    /// refresh command's granular options over the Power BI enhanced-refresh API).
    #[command(display_order = 11)]
    Refresh {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh type
        #[arg(long, default_value = "Full")]
        r#type: String,

        /// Enhanced refresh: JSON array of tables/partitions to refresh, e.g.
        /// '[{"table":"Sales"},{"table":"Sales","partition":"2024"}]'
        #[arg(long)]
        objects: Option<String>,

        /// Enhanced refresh commit mode: transactional | partialBatch
        #[arg(long)]
        commit_mode: Option<String>,

        /// Enhanced refresh: maximum number of parallel processing threads
        #[arg(long)]
        max_parallelism: Option<u32>,

        /// Enhanced refresh: number of times to retry on transient failure
        #[arg(long)]
        retry_count: Option<u32>,
    },
    /// Take over a semantic model (converts definition-managed to service-managed for portal editing)
    #[command(display_order = 12)]
    Takeover {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List parameters of a semantic model
    #[command(name = "list-parameters", display_order = 13)]
    ListParameters {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List tables of a semantic model (via DAX INFO.VIEW.TABLES — no definition parsing)
    #[command(name = "list-tables", display_order = 13)]
    ListTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List columns of a semantic model (via DAX INFO.VIEW.COLUMNS)
    #[command(name = "list-columns", display_order = 13)]
    ListColumns {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List measures of a semantic model (via DAX INFO.VIEW.MEASURES)
    #[command(name = "list-measures", display_order = 13)]
    ListMeasures {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List relationships of a semantic model (via DAX INFO.VIEW.RELATIONSHIPS)
    #[command(name = "list-relationships", display_order = 13)]
    ListRelationships {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Analyze a model against best-practice rules (Best Practice Analyzer /
    /// Memory Analyzer over INFO.VIEW metadata) — descriptions, naming,
    /// implicit aggregation, duplicate measures, relationship hygiene, star
    /// schema, calculated columns, and (opt-in) high cardinality
    #[command(display_order = 13)]
    Analyze {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Also probe column cardinality via DISTINCTCOUNT (extra DAX; flags high-cardinality columns)
        #[arg(long)]
        with_cardinality: bool,

        /// Minimum severity to report: info, warning, error
        #[arg(long, default_value = "info")]
        severity: String,

        /// Exit non-zero if any issue at/above the severity threshold is found (CI gate)
        #[arg(long)]
        strict: bool,

        /// Auto-fix the SAFE, mechanical issues (currently: set default
        /// summarization to None on identifier columns). Overwrites the model
        /// definition (irreversible) — dry-run guarded. Naming/schema/dedup
        /// issues are NOT auto-fixed (they need human judgment).
        #[arg(long)]
        fix: bool,
    },
    /// List each measure's dependencies (the measures/columns/tables its DAX
    /// references) — useful for including dependent objects in an AI data schema
    #[command(name = "measure-dependencies", display_order = 13)]
    MeasureDependencies {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Set the description of a table, column, or measure by editing the model
    /// definition (getDefinition → edit TMDL/model.bim → updateDefinition).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "set-description", display_order = 13)]
    SetDescription {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Target table (a table itself, or the owner of --column)
        #[arg(long)]
        table: Option<String>,

        /// Target column (requires --table)
        #[arg(long)]
        column: Option<String>,

        /// Target measure (model-unique; --table not needed)
        #[arg(long)]
        measure: Option<String>,

        /// Description text (use \n for multi-line)
        #[arg(long)]
        description: String,
    },
    /// Add a measure to a table by editing the model definition
    /// (getDefinition → edit TMDL/model.bim → updateDefinition). Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "add-measure", display_order = 13)]
    AddMeasure {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table to add the measure to
        #[arg(long)]
        table: String,

        /// Measure name (must be model-unique)
        #[arg(long)]
        name: String,

        /// DAX expression (e.g. "SUM('Sales'[Amount])")
        #[arg(long)]
        expression: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Optional format string (e.g. "0.00", "$#,0")
        #[arg(long)]
        format_string: Option<String>,

        /// Optional display folder
        #[arg(long)]
        display_folder: Option<String>,
    },
    /// Update an existing measure's expression and/or properties by editing the
    /// model definition (getDefinition → edit TMDL/model.bim → updateDefinition).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "update-measure", display_order = 13)]
    UpdateMeasure {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Measure name to update (model-unique)
        #[arg(long)]
        measure: String,

        /// New DAX expression
        #[arg(long)]
        expression: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,

        /// New format string
        #[arg(long)]
        format_string: Option<String>,

        /// New display folder
        #[arg(long)]
        display_folder: Option<String>,
    },
    /// Delete a measure from the model by editing the definition. Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "delete-measure", display_order = 13)]
    DeleteMeasure {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Measure name to delete (model-unique)
        #[arg(long)]
        measure: String,
    },
    /// Rename a measure (its declaration only; DAX references are NOT rewritten).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "rename-measure", display_order = 13)]
    RenameMeasure {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Current measure name
        #[arg(long)]
        measure: String,

        /// New measure name
        #[arg(long)]
        new_name: String,
    },
    /// Move a measure to a different home table (name and definition preserved).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "move-measure", display_order = 13)]
    MoveMeasure {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Measure name to move (model-unique)
        #[arg(long)]
        measure: String,

        /// Destination table
        #[arg(long)]
        to_table: String,
    },
    /// Add a relationship between two tables by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-relationship", display_order = 13)]
    AddRelationship {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// "Many" side table (foreign key)
        #[arg(long)]
        from_table: String,

        /// "Many" side column (foreign key)
        #[arg(long)]
        from_column: String,

        /// "One" side table (primary key)
        #[arg(long)]
        to_table: String,

        /// "One" side column (primary key)
        #[arg(long)]
        to_column: String,

        /// Cross-filter direction: oneDirection (default), bothDirections, automatic
        #[arg(long)]
        cross_filter: Option<String>,

        /// Create the relationship inactive
        #[arg(long)]
        inactive: bool,

        /// From-side cardinality: one | many (default many)
        #[arg(long)]
        from_cardinality: Option<String>,

        /// To-side cardinality: one | many (default one)
        #[arg(long)]
        to_cardinality: Option<String>,
    },
    /// Delete a relationship (by --relationship-id or by the from/to columns).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-relationship", display_order = 13)]
    DeleteRelationship {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Relationship id (GUID) to delete
        #[arg(long)]
        relationship_id: Option<String>,

        /// "Many" side table (used with the other from/to flags to match by columns)
        #[arg(long)]
        from_table: Option<String>,

        /// "Many" side column
        #[arg(long)]
        from_column: Option<String>,

        /// "One" side table
        #[arg(long)]
        to_table: Option<String>,

        /// "One" side column
        #[arg(long)]
        to_column: Option<String>,
    },
    /// Update a relationship's active state and/or cross-filter direction
    /// (by --relationship-id or by the from/to columns). Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "update-relationship", display_order = 13)]
    UpdateRelationship {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Relationship id (GUID) to update
        #[arg(long)]
        relationship_id: Option<String>,

        /// "Many" side table (match by columns)
        #[arg(long)]
        from_table: Option<String>,

        /// "Many" side column
        #[arg(long)]
        from_column: Option<String>,

        /// "One" side table
        #[arg(long)]
        to_table: Option<String>,

        /// "One" side column
        #[arg(long)]
        to_column: Option<String>,

        /// Set the relationship active
        #[arg(long, conflicts_with = "inactive")]
        active: bool,

        /// Set the relationship inactive
        #[arg(long)]
        inactive: bool,

        /// New cross-filter direction: oneDirection, bothDirections, automatic
        #[arg(long)]
        cross_filter: Option<String>,
    },
    /// List security roles (RLS) of a semantic model (name, model permission,
    /// and per-table filters) — read-only.
    #[command(name = "list-roles", display_order = 13)]
    ListRoles {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a security role by editing the model definition. Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "add-role", display_order = 13)]
    AddRole {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Role name
        #[arg(long)]
        name: String,

        /// Model permission: read (default), none, readRefresh, refresh
        #[arg(long, default_value = "read")]
        model_permission: String,
    },
    /// Delete a security role (and its RLS filters) by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-role", display_order = 13)]
    DeleteRole {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Role name to delete
        #[arg(long)]
        name: String,
    },
    /// Set a row-level-security (RLS) filter on a table for a role (a DAX
    /// predicate). Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "set-rls", display_order = 13)]
    SetRls {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Role to add the filter to
        #[arg(long)]
        role: String,

        /// Table the filter applies to
        #[arg(long)]
        table: String,

        /// DAX filter predicate (e.g. "'Sales'[Region] = \"West\"")
        #[arg(long)]
        filter: String,
    },
    /// Remove a row-level-security (RLS) filter from a table for a role.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-rls", display_order = 13)]
    DeleteRls {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Role to remove the filter from
        #[arg(long)]
        role: String,

        /// Table whose filter to remove
        #[arg(long)]
        table: String,
    },
    /// Add a calculated column (a DAX-defined column) to a table by editing the
    /// model definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-calculated-column", display_order = 13)]
    AddCalculatedColumn {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table to add the column to
        #[arg(long)]
        table: String,

        /// Column name (must be unique in the table)
        #[arg(long)]
        name: String,

        /// DAX expression (e.g. "UPPER('Sales'[Region])")
        #[arg(long)]
        expression: String,

        /// Data type: string (default), int64, double, decimal, dateTime, boolean
        #[arg(long)]
        data_type: Option<String>,

        /// Format string
        #[arg(long)]
        format_string: Option<String>,

        /// Default summarization: none, sum, count, min, max, average, distinctCount
        #[arg(long)]
        summarize_by: Option<String>,

        /// Display folder
        #[arg(long)]
        display_folder: Option<String>,

        /// Description
        #[arg(long)]
        description: Option<String>,

        /// Hide the column
        #[arg(long)]
        hidden: bool,
    },
    /// Delete a column from a table by editing the model definition. Overwrites
    /// the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-column", display_order = 13)]
    DeleteColumn {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the column
        #[arg(long)]
        table: String,

        /// Column name to delete
        #[arg(long)]
        name: String,
    },
    /// Rename a column (declaration only; DAX/relationship references are NOT
    /// rewritten). Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "rename-column", display_order = 13)]
    RenameColumn {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the column
        #[arg(long)]
        table: String,

        /// Current column name
        #[arg(long)]
        name: String,

        /// New column name
        #[arg(long)]
        new_name: String,
    },
    /// Update a column's properties (data type, format, summarization, display
    /// folder, description, hidden). Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "update-column", display_order = 13)]
    UpdateColumn {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the column
        #[arg(long)]
        table: String,

        /// Column name to update
        #[arg(long)]
        name: String,

        /// New data type: string, int64, double, decimal, dateTime, boolean
        #[arg(long)]
        data_type: Option<String>,

        /// New format string
        #[arg(long)]
        format_string: Option<String>,

        /// New default summarization: none, sum, count, min, max, average, distinctCount
        #[arg(long)]
        summarize_by: Option<String>,

        /// New display folder
        #[arg(long)]
        display_folder: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,

        /// Set hidden state (true/false)
        #[arg(long)]
        hidden: Option<bool>,
    },
    /// Add a calculated table (a DAX table expression) by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-table", display_order = 13)]
    AddTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table name (must be model-unique)
        #[arg(long)]
        name: String,

        /// DAX table expression (e.g. "CALENDAR(DATE(2020,1,1), DATE(2020,12,31))")
        #[arg(long)]
        expression: String,
    },
    /// Delete a table by editing the model definition. CASCADES: also removes
    /// relationships and role RLS filters that reference the table. Overwrites
    /// the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-table", display_order = 13)]
    DeleteTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table name to delete
        #[arg(long)]
        name: String,
    },
    /// Rename a table (declaration, file, and model.tmdl ref; DAX/relationship
    /// references are NOT rewritten). Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "rename-table", display_order = 13)]
    RenameTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Current table name
        #[arg(long)]
        name: String,

        /// New table name
        #[arg(long)]
        new_name: String,
    },
    /// Update a table's properties (hidden state, data category, description) by
    /// editing the model definition. Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "update-table", display_order = 13)]
    UpdateTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table name to update
        #[arg(long)]
        name: String,

        /// Set the table hidden state (true/false)
        #[arg(long)]
        hidden: Option<bool>,

        /// Data category (e.g. "Time" to mark a date table)
        #[arg(long)]
        data_category: Option<String>,

        /// Table description
        #[arg(long)]
        description: Option<String>,
    },
    /// List translation cultures of a semantic model (culture + translation
    /// count) — read-only.
    #[command(name = "list-cultures", display_order = 13)]
    ListCultures {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a translation culture (e.g. fr-FR) by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-culture", display_order = 13)]
    AddCulture {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Culture / locale name (e.g. fr-FR, es-ES)
        #[arg(long)]
        culture: String,
    },
    /// Delete a translation culture by editing the model definition. Overwrites
    /// the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-culture", display_order = 13)]
    DeleteCulture {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Culture / locale name to delete
        #[arg(long)]
        culture: String,
    },
    /// Set a translated caption for a table/column/measure in a culture.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "set-translation", display_order = 13)]
    SetTranslation {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Culture / locale name (must already exist — add it with add-culture)
        #[arg(long)]
        culture: String,

        /// Table to translate (or the owner of --column/--measure)
        #[arg(long)]
        table: String,

        /// Column to translate (requires --table)
        #[arg(long)]
        column: Option<String>,

        /// Measure to translate
        #[arg(long)]
        measure: Option<String>,

        /// Translated caption (the display name in this culture)
        #[arg(long)]
        caption: String,
    },
    /// List user hierarchies of a semantic model (table, name, level count) —
    /// read-only.
    #[command(name = "list-hierarchies", display_order = 13)]
    ListHierarchies {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Only list hierarchies of this table
        #[arg(long)]
        table: Option<String>,
    },
    /// Add a user (drill-down) hierarchy to a table by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-hierarchy", display_order = 13)]
    AddHierarchy {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table to add the hierarchy to
        #[arg(long)]
        table: String,

        /// Hierarchy name (must be unique in the table)
        #[arg(long)]
        name: String,

        /// A level, top-to-bottom. Repeat for each level. Forms: "Column",
        /// "LevelName=Column", or "LevelName:Column".
        #[arg(long = "level", required = true)]
        levels: Vec<String>,
    },
    /// Delete a hierarchy from a table by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-hierarchy", display_order = 13)]
    DeleteHierarchy {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the hierarchy
        #[arg(long)]
        table: String,

        /// Hierarchy name to delete
        #[arg(long)]
        name: String,
    },
    /// List a table's partitions (name + mode) — read-only.
    #[command(name = "list-partitions", display_order = 13)]
    ListPartitions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Only list partitions of this table
        #[arg(long)]
        table: Option<String>,
    },
    /// Add a partition to a table (an extra data-source query, e.g. for
    /// incremental refresh) by editing the model definition. Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "add-partition", display_order = 13)]
    AddPartition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table to add the partition to
        #[arg(long)]
        table: String,

        /// Partition name (must be unique in the table)
        #[arg(long)]
        name: String,

        /// Power Query M source expression (mutually exclusive with --dax)
        #[arg(long, conflicts_with = "dax")]
        m: Option<String>,

        /// DAX table expression for a calculated partition (mutually exclusive with --m)
        #[arg(long)]
        dax: Option<String>,
    },
    /// Update a partition's source expression by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "update-partition", display_order = 13)]
    UpdatePartition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the partition
        #[arg(long)]
        table: String,

        /// Partition name to update
        #[arg(long)]
        name: String,

        /// New Power Query M source expression (mutually exclusive with --dax)
        #[arg(long, conflicts_with = "dax")]
        m: Option<String>,

        /// New DAX expression for a calculated partition (mutually exclusive with --m)
        #[arg(long)]
        dax: Option<String>,
    },
    /// Delete a partition from a table by editing the model definition (a table
    /// must keep at least one). Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "delete-partition", display_order = 13)]
    DeletePartition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Table containing the partition
        #[arg(long)]
        table: String,

        /// Partition name to delete
        #[arg(long)]
        name: String,
    },
    /// List calculation groups of a semantic model (name + item count) —
    /// read-only.
    #[command(name = "list-calculation-groups", display_order = 13)]
    ListCalculationGroups {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a calculation group (for time intelligence, etc.) by editing the
    /// model definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-calculation-group", display_order = 13)]
    AddCalculationGroup {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Calculation group name (also the table name)
        #[arg(long)]
        name: String,

        /// The group's column name (defaults to the group name)
        #[arg(long)]
        column_name: Option<String>,
    },
    /// Delete a calculation group (its whole table) by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-calculation-group", display_order = 13)]
    DeleteCalculationGroup {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Calculation group name to delete
        #[arg(long)]
        name: String,
    },
    /// Add a calculation item (a DAX time-intelligence variant) to a calculation
    /// group. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-calculation-item", display_order = 13)]
    AddCalculationItem {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Calculation group name
        #[arg(long)]
        group: String,

        /// Calculation item name
        #[arg(long)]
        name: String,

        /// DAX expression, e.g. `CALCULATE(SELECTEDMEASURE(), DATESYTD('Date'[Date]))`
        #[arg(long)]
        expression: String,

        /// Optional ordinal (sort position)
        #[arg(long)]
        ordinal: Option<i64>,
    },
    /// Delete a calculation item from a calculation group by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-calculation-item", display_order = 13)]
    DeleteCalculationItem {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Calculation group name
        #[arg(long)]
        group: String,

        /// Calculation item name to delete
        #[arg(long)]
        name: String,
    },
    /// List named expressions / Power Query parameters of a semantic model —
    /// read-only.
    #[command(name = "list-expressions", display_order = 13)]
    ListExpressions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a named expression (a shared M query) or Power Query parameter by
    /// editing the model definition. Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "add-expression", display_order = 13)]
    AddExpression {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Expression name
        #[arg(long)]
        name: String,

        /// Raw Power Query M expression (mutually exclusive with --parameter-value)
        #[arg(long, conflicts_with = "parameter_value")]
        expression: Option<String>,

        /// Make it a Power Query PARAMETER with this default value
        #[arg(long)]
        parameter_value: Option<String>,

        /// Parameter type: `Text` (default), `Number`, `Logical`, `DateTime`
        #[arg(long)]
        parameter_type: Option<String>,
    },
    /// Update a named expression / parameter's value by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "update-expression", display_order = 13)]
    UpdateExpression {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Expression name to update
        #[arg(long)]
        name: String,

        /// New raw Power Query M expression (mutually exclusive with --parameter-value)
        #[arg(long, conflicts_with = "parameter_value")]
        expression: Option<String>,

        /// New parameter default value
        #[arg(long)]
        parameter_value: Option<String>,

        /// Parameter type: `Text` (default), `Number`, `Logical`, `DateTime`
        #[arg(long)]
        parameter_type: Option<String>,
    },
    /// Delete a named expression / parameter by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-expression", display_order = 13)]
    DeleteExpression {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Expression name to delete
        #[arg(long)]
        name: String,
    },
    /// List DAX user-defined functions (UDFs) of a semantic model — read-only.
    #[command(name = "list-functions", display_order = 13)]
    ListFunctions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a DAX user-defined function (UDF) by editing the model definition
    /// (requires model compatibility level >=1702; bumped automatically).
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-function", display_order = 13)]
    AddFunction {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Function name
        #[arg(long)]
        name: String,

        /// DAX UDF expression, e.g. `(x: INT64) => RETURN x + 1`
        #[arg(long)]
        expression: String,
    },
    /// Update a DAX user-defined function by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "update-function", display_order = 13)]
    UpdateFunction {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Function name to update
        #[arg(long)]
        name: String,

        /// New DAX UDF expression
        #[arg(long)]
        expression: String,
    },
    /// Delete a DAX user-defined function by editing the model definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-function", display_order = 13)]
    DeleteFunction {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Function name to delete
        #[arg(long)]
        name: String,
    },
    /// List perspectives (filtered model views) of a semantic model — read-only.
    #[command(name = "list-perspectives", display_order = 13)]
    ListPerspectives {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a perspective (a filtered view of the model) by editing the model
    /// definition. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-perspective", display_order = 13)]
    AddPerspective {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Perspective name
        #[arg(long)]
        name: String,
    },
    /// Delete a perspective by editing the model definition. Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "delete-perspective", display_order = 13)]
    DeletePerspective {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Perspective name to delete
        #[arg(long)]
        name: String,
    },
    /// Add a member (a table, or a column/measure/hierarchy of a table) to a
    /// perspective. Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "add-perspective-member", display_order = 13)]
    AddPerspectiveMember {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Perspective name (must already exist — add it with add-perspective)
        #[arg(long)]
        perspective: String,

        /// Table to include (or the owner of --column/--measure/--hierarchy)
        #[arg(long)]
        table: String,

        /// Include this column of the table
        #[arg(long)]
        column: Option<String>,

        /// Include this measure of the table
        #[arg(long)]
        measure: Option<String>,

        /// Include this hierarchy of the table
        #[arg(long)]
        hierarchy: Option<String>,
    },
    /// Remove a member from a perspective (a whole table, or one of its
    /// members). Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "remove-perspective-member", display_order = 13)]
    RemovePerspectiveMember {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Perspective name
        #[arg(long)]
        perspective: String,

        /// Table to remove (or the owner of --column/--measure/--hierarchy)
        #[arg(long)]
        table: String,

        /// Remove this column (instead of the whole table)
        #[arg(long)]
        column: Option<String>,

        /// Remove this measure (instead of the whole table)
        #[arg(long)]
        measure: Option<String>,

        /// Remove this hierarchy (instead of the whole table)
        #[arg(long)]
        hierarchy: Option<String>,
    },
    /// Update parameters of a semantic model
    #[command(name = "update-parameters", display_order = 14)]
    UpdateParameters {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// JSON content with parameter updates (inline or @file or @- for stdin)
        #[arg(long)]
        content: String,
    },
    /// List datasources of a semantic model
    #[command(name = "list-datasources", display_order = 15)]
    ListDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// List the gateway datasources bound to a semantic model
    #[command(name = "get-bound-gateway-datasources", display_order = 15)]
    GetBoundGatewayDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Bind a semantic model's data sources to an on-premises/VNet data gateway
    #[command(name = "bind-to-gateway", display_order = 15)]
    BindToGateway {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Gateway object id to bind to
        #[arg(long)]
        gateway_id: String,

        /// Optional comma-separated gateway datasource object ids to bind (defaults to all)
        #[arg(long)]
        datasource_ids: Option<String>,
    },
    /// Update datasources of a semantic model
    #[command(name = "update-datasources", display_order = 16)]
    UpdateDatasources {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// JSON content with datasource updates (inline or @file or @- for stdin)
        #[arg(long)]
        content: String,
    },
    /// List users (permissions) of a semantic model
    #[command(name = "list-users", display_order = 17)]
    ListUsers {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Add a user to a semantic model
    #[command(name = "add-user", display_order = 18)]
    AddUser {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Principal identifier (email, OID, or group ID)
        #[arg(long)]
        principal: String,

        /// Principal type
        #[arg(long, value_parser = ["User", "Group", "App"])]
        principal_type: String,

        /// Access right for the dataset
        #[arg(long, value_parser = ["Read", "ReadExplore", "ReadReshare", "ReadReshareExplore"])]
        access_right: String,
    },
    /// Remove a user from a semantic model
    #[command(name = "delete-user", display_order = 19)]
    DeleteUser {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// User email or principal ID to remove
        #[arg(long)]
        user: String,
    },
    /// Get refresh history and status for a semantic model
    #[command(name = "refresh-status", display_order = 20)]
    RefreshStatus {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Maximum number of refresh entries to return (default: 10)
        #[arg(long, default_value = "10")]
        top: u32,
    },
    /// Get execution details of a specific (enhanced) refresh by its request id
    #[command(name = "refresh-details", display_order = 20)]
    RefreshDetails {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh request id (the `requestId` from refresh-status)
        #[arg(long)]
        refresh_id: String,
    },
    /// Cancel an in-progress enhanced refresh by its request id
    #[command(name = "cancel-refresh", display_order = 20)]
    CancelRefresh {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Refresh request id (the `requestId` from refresh-status) to cancel
        #[arg(long)]
        refresh_id: String,
    },
    /// Get the scheduled (automatic) refresh configuration
    #[command(name = "get-refresh-schedule", display_order = 20)]
    GetRefreshSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Update the scheduled (automatic) refresh configuration
    ///
    /// Import / Direct Lake models. Times must be on the full or half hour
    /// (HH:00 / HH:30). To disable, pass ONLY --enabled false (the API rejects
    /// changing other settings while disabling).
    #[command(name = "update-refresh-schedule", display_order = 20)]
    UpdateRefreshSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Enable or disable the schedule
        #[arg(long)]
        enabled: Option<bool>,

        /// Comma-separated weekday names (e.g. "Monday,Thursday")
        #[arg(long)]
        days: Option<String>,

        /// Comma-separated times on the full/half hour (e.g. "07:00,13:30")
        #[arg(long)]
        times: Option<String>,

        /// Local time zone id (e.g. "UTC")
        #[arg(long)]
        local_time_zone_id: Option<String>,

        /// Failure/completion notification: NoNotification | MailOnFailure | MailOnCompletion
        #[allow(clippy::doc_markdown)]
        #[arg(long)]
        notify_option: Option<String>,
    },
    /// List upstream (lineage) datasets that this semantic model depends on
    #[command(name = "list-upstream", display_order = 21)]
    ListUpstream {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,
    },
    /// Clone a semantic model to the same or different workspace
    #[command(display_order = 22)]
    Clone {
        /// Source workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID to clone
        #[arg(long)]
        id: String,

        /// Display name for the cloned model
        #[arg(long)]
        name: String,

        /// Destination workspace ID (defaults to same workspace)
        #[arg(long, visible_alias = "dest-workspace")]
        target_workspace: Option<String>,
    },
    /// Export a semantic model as a .pbix file
    #[command(name = "export-pbix", display_order = 23)]
    ExportPbix {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Semantic model ID
        #[arg(long)]
        id: String,

        /// Output file path (e.g., model.pbix)
        #[arg(long)]
        file: String,
    },
    /// Import a .pbix file as a new semantic model
    #[command(name = "import-pbix", display_order = 24)]
    ImportPbix {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name for the imported model
        #[arg(long)]
        name: String,

        /// Path to the .pbix file to import
        #[arg(long)]
        file: String,

        /// Conflict resolution: Abort, Overwrite, `CreateOrOverwrite`, `GenerateUniqueName`
        #[arg(long, default_value = "Abort")]
        name_conflict: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &SemanticModelCommand,
) -> Result<()> {
    match command {
        SemanticModelCommand::List { workspace } => crud::list(cli, client, workspace).await,
        SemanticModelCommand::Show { workspace, id } => {
            crud::show(cli, client, workspace, id).await
        }
        SemanticModelCommand::Create {
            workspace,
            name,
            description,
            file,
            definition,
            connection,
            sensitivity_label,
        } => {
            crud::create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                file.as_deref(),
                definition.as_deref(),
                connection.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        SemanticModelCommand::Generate {
            workspace,
            name,
            lakehouse,
            warehouse,
            tables,
            schema,
            no_refresh,
            description,
            sensitivity_label,
        } => {
            generate::generate(
                cli,
                client,
                workspace,
                lakehouse.as_deref(),
                warehouse.as_deref(),
                name,
                tables.as_deref(),
                schema,
                *no_refresh,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        SemanticModelCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crud::update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        SemanticModelCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete(cli, client, workspace, id, *hard_delete).await,
        SemanticModelCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => definitions::get_definition(cli, client, workspace, id, *decode).await,
        SemanticModelCommand::UpdateDefinition {
            workspace,
            id,
            file,
        } => definitions::update_definition(cli, client, workspace, id, file).await,
        SemanticModelCommand::Query {
            workspace,
            id,
            dax,
            file,
        } => operations::query(cli, client, workspace, id, dax.as_deref(), file.as_deref()).await,
        SemanticModelCommand::BindConnection {
            workspace,
            id,
            connection_id,
        } => operations::bind_connection(cli, client, workspace, id, connection_id).await,
        SemanticModelCommand::UnbindConnection { workspace, id } => {
            operations::unbind_connection(cli, client, workspace, id).await
        }
        SemanticModelCommand::Refresh {
            workspace,
            id,
            r#type,
            objects,
            commit_mode,
            max_parallelism,
            retry_count,
        } => {
            operations::refresh(
                cli,
                client,
                workspace,
                id,
                r#type,
                objects.as_deref(),
                commit_mode.as_deref(),
                *max_parallelism,
                *retry_count,
            )
            .await
        }
        SemanticModelCommand::Takeover { workspace, id } => {
            operations::takeover(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListParameters { workspace, id } => {
            powerbi::list_parameters(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListTables { workspace, id } => {
            operations::list_tables(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListColumns { workspace, id } => {
            operations::list_columns(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListMeasures { workspace, id } => {
            operations::list_measures(cli, client, workspace, id).await
        }
        SemanticModelCommand::ListRelationships { workspace, id } => {
            operations::list_relationships(cli, client, workspace, id).await
        }
        SemanticModelCommand::Analyze {
            workspace,
            id,
            with_cardinality,
            severity,
            strict,
            fix,
        } => {
            analyze::analyze(
                cli,
                client,
                workspace,
                id,
                *with_cardinality,
                severity,
                *strict,
                *fix,
            )
            .await
        }
        SemanticModelCommand::MeasureDependencies { workspace, id } => {
            analyze::measure_dependencies(cli, client, workspace, id).await
        }
        SemanticModelCommand::UpdateParameters {
            workspace,
            id,
            content,
        } => powerbi::update_parameters(cli, client, workspace, id, content).await,
        SemanticModelCommand::ListDatasources { workspace, id } => {
            powerbi::list_datasources(cli, client, workspace, id).await
        }
        SemanticModelCommand::GetBoundGatewayDatasources { workspace, id } => {
            powerbi::get_bound_gateway_datasources(cli, client, workspace, id).await
        }
        SemanticModelCommand::BindToGateway {
            workspace,
            id,
            gateway_id,
            datasource_ids,
        } => {
            powerbi::bind_to_gateway(
                cli,
                client,
                workspace,
                id,
                gateway_id,
                datasource_ids.as_deref(),
            )
            .await
        }
        SemanticModelCommand::UpdateDatasources {
            workspace,
            id,
            content,
        } => powerbi::update_datasources(cli, client, workspace, id, content).await,
        SemanticModelCommand::ListUsers { workspace, id } => {
            powerbi::list_users(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddUser {
            workspace,
            id,
            principal,
            principal_type,
            access_right,
        } => {
            powerbi::add_user(
                cli,
                client,
                workspace,
                id,
                principal,
                principal_type,
                access_right,
            )
            .await
        }
        SemanticModelCommand::DeleteUser {
            workspace,
            id,
            user,
        } => powerbi::delete_user(cli, client, workspace, id, user).await,
        SemanticModelCommand::RefreshStatus { workspace, id, top } => {
            powerbi::refresh_status(cli, client, workspace, id, *top).await
        }
        SemanticModelCommand::RefreshDetails {
            workspace,
            id,
            refresh_id,
        } => operations::refresh_details(cli, client, workspace, id, refresh_id).await,
        SemanticModelCommand::CancelRefresh {
            workspace,
            id,
            refresh_id,
        } => operations::cancel_refresh(cli, client, workspace, id, refresh_id).await,
        SemanticModelCommand::GetRefreshSchedule { workspace, id } => {
            operations::get_refresh_schedule(cli, client, workspace, id).await
        }
        SemanticModelCommand::UpdateRefreshSchedule {
            workspace,
            id,
            enabled,
            days,
            times,
            local_time_zone_id,
            notify_option,
        } => {
            operations::update_refresh_schedule(
                cli,
                client,
                workspace,
                id,
                *enabled,
                days.as_deref(),
                times.as_deref(),
                local_time_zone_id.as_deref(),
                notify_option.as_deref(),
            )
            .await
        }
        SemanticModelCommand::ListUpstream { workspace, id } => {
            powerbi::list_upstream(cli, client, workspace, id).await
        }
        SemanticModelCommand::Clone {
            workspace,
            id,
            name,
            target_workspace,
        } => {
            powerbi::clone_model(
                cli,
                client,
                workspace,
                id,
                name,
                target_workspace.as_deref(),
            )
            .await
        }
        SemanticModelCommand::ExportPbix {
            workspace,
            id,
            file,
        } => powerbi::export_pbix(cli, client, workspace, id, file).await,
        SemanticModelCommand::ImportPbix {
            workspace,
            name,
            file,
            name_conflict,
        } => powerbi::import_pbix(cli, client, workspace, name, file, name_conflict).await,
        _ => execute_authoring(cli, client, command).await,
    }
}

#[allow(clippy::too_many_lines)]
async fn execute_authoring(
    cli: &Cli,
    client: &FabricClient,
    command: &SemanticModelCommand,
) -> Result<()> {
    match command {
        SemanticModelCommand::SetDescription {
            workspace,
            id,
            table,
            column,
            measure,
            description,
        } => {
            authoring::set_description(
                cli,
                client,
                workspace,
                id,
                table.as_deref(),
                column.as_deref(),
                measure.as_deref(),
                description,
            )
            .await
        }
        SemanticModelCommand::AddMeasure {
            workspace,
            id,
            table,
            name,
            expression,
            description,
            format_string,
            display_folder,
        } => {
            authoring::add_measure(
                cli,
                client,
                workspace,
                id,
                table,
                name,
                &authoring::MeasureFields {
                    expression: Some(expression),
                    description: description.as_deref(),
                    format_string: format_string.as_deref(),
                    display_folder: display_folder.as_deref(),
                },
            )
            .await
        }
        SemanticModelCommand::UpdateMeasure {
            workspace,
            id,
            measure,
            expression,
            description,
            format_string,
            display_folder,
        } => {
            authoring::update_measure(
                cli,
                client,
                workspace,
                id,
                measure,
                &authoring::MeasureFields {
                    expression: expression.as_deref(),
                    description: description.as_deref(),
                    format_string: format_string.as_deref(),
                    display_folder: display_folder.as_deref(),
                },
            )
            .await
        }
        SemanticModelCommand::DeleteMeasure {
            workspace,
            id,
            measure,
        } => authoring::delete_measure(cli, client, workspace, id, measure).await,
        SemanticModelCommand::RenameMeasure {
            workspace,
            id,
            measure,
            new_name,
        } => authoring::rename_measure(cli, client, workspace, id, measure, new_name).await,
        SemanticModelCommand::MoveMeasure {
            workspace,
            id,
            measure,
            to_table,
        } => authoring::move_measure(cli, client, workspace, id, measure, to_table).await,
        SemanticModelCommand::AddRelationship {
            workspace,
            id,
            from_table,
            from_column,
            to_table,
            to_column,
            cross_filter,
            inactive,
            from_cardinality,
            to_cardinality,
        } => {
            relationships::add_relationship(
                cli,
                client,
                workspace,
                id,
                &relationships::RelSpec {
                    from_table,
                    from_column,
                    to_table,
                    to_column,
                },
                &relationships::RelProps {
                    cross_filter: cross_filter.as_deref(),
                    is_active: inactive.then_some(false),
                    from_cardinality: from_cardinality.as_deref(),
                    to_cardinality: to_cardinality.as_deref(),
                },
            )
            .await
        }
        SemanticModelCommand::DeleteRelationship {
            workspace,
            id,
            relationship_id,
            from_table,
            from_column,
            to_table,
            to_column,
        } => {
            let spec = build_rel_spec(
                from_table.as_deref(),
                from_column.as_deref(),
                to_table.as_deref(),
                to_column.as_deref(),
                relationship_id.as_deref(),
            )?;
            let rel = spec.as_ref().map(|s| relationships::RelSpec {
                from_table: &s.0,
                from_column: &s.1,
                to_table: &s.2,
                to_column: &s.3,
            });
            relationships::delete_relationship(
                cli,
                client,
                workspace,
                id,
                relationship_id.as_deref(),
                rel.as_ref(),
            )
            .await
        }
        SemanticModelCommand::UpdateRelationship {
            workspace,
            id,
            relationship_id,
            from_table,
            from_column,
            to_table,
            to_column,
            active,
            inactive,
            cross_filter,
        } => {
            let spec = build_rel_spec(
                from_table.as_deref(),
                from_column.as_deref(),
                to_table.as_deref(),
                to_column.as_deref(),
                relationship_id.as_deref(),
            )?;
            let rel = spec.as_ref().map(|s| relationships::RelSpec {
                from_table: &s.0,
                from_column: &s.1,
                to_table: &s.2,
                to_column: &s.3,
            });
            let is_active = if *active {
                Some(true)
            } else if *inactive {
                Some(false)
            } else {
                None
            };
            relationships::update_relationship(
                cli,
                client,
                workspace,
                id,
                relationship_id.as_deref(),
                rel.as_ref(),
                &relationships::RelProps {
                    cross_filter: cross_filter.as_deref(),
                    is_active,
                    from_cardinality: None,
                    to_cardinality: None,
                },
            )
            .await
        }
        SemanticModelCommand::ListRoles { workspace, id } => {
            roles::list_roles(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddRole {
            workspace,
            id,
            name,
            model_permission,
        } => roles::add_role(cli, client, workspace, id, name, model_permission).await,
        SemanticModelCommand::DeleteRole {
            workspace,
            id,
            name,
        } => roles::delete_role(cli, client, workspace, id, name).await,
        SemanticModelCommand::SetRls {
            workspace,
            id,
            role,
            table,
            filter,
        } => roles::set_rls(cli, client, workspace, id, role, table, filter).await,
        SemanticModelCommand::DeleteRls {
            workspace,
            id,
            role,
            table,
        } => roles::delete_rls(cli, client, workspace, id, role, table).await,
        SemanticModelCommand::AddCalculatedColumn {
            workspace,
            id,
            table,
            name,
            expression,
            data_type,
            format_string,
            summarize_by,
            display_folder,
            description,
            hidden,
        } => {
            columns::add_calculated_column(
                cli,
                client,
                workspace,
                id,
                table,
                name,
                expression,
                &columns::ColumnProps {
                    data_type: data_type.as_deref(),
                    format_string: format_string.as_deref(),
                    summarize_by: summarize_by.as_deref(),
                    display_folder: display_folder.as_deref(),
                    description: description.as_deref(),
                    hidden: hidden.then_some(true),
                },
            )
            .await
        }
        SemanticModelCommand::DeleteColumn {
            workspace,
            id,
            table,
            name,
        } => columns::delete_column(cli, client, workspace, id, table, name).await,
        SemanticModelCommand::RenameColumn {
            workspace,
            id,
            table,
            name,
            new_name,
        } => columns::rename_column(cli, client, workspace, id, table, name, new_name).await,
        SemanticModelCommand::UpdateColumn {
            workspace,
            id,
            table,
            name,
            data_type,
            format_string,
            summarize_by,
            display_folder,
            description,
            hidden,
        } => {
            columns::update_column(
                cli,
                client,
                workspace,
                id,
                table,
                name,
                &columns::ColumnProps {
                    data_type: data_type.as_deref(),
                    format_string: format_string.as_deref(),
                    summarize_by: summarize_by.as_deref(),
                    display_folder: display_folder.as_deref(),
                    description: description.as_deref(),
                    hidden: *hidden,
                },
            )
            .await
        }
        SemanticModelCommand::AddTable {
            workspace,
            id,
            name,
            expression,
        } => tables::add_table(cli, client, workspace, id, name, expression).await,
        SemanticModelCommand::DeleteTable {
            workspace,
            id,
            name,
        } => tables::delete_table(cli, client, workspace, id, name).await,
        SemanticModelCommand::RenameTable {
            workspace,
            id,
            name,
            new_name,
        } => tables::rename_table(cli, client, workspace, id, name, new_name).await,
        SemanticModelCommand::UpdateTable {
            workspace,
            id,
            name,
            hidden,
            data_category,
            description,
        } => {
            tables::update_table(
                cli,
                client,
                workspace,
                id,
                name,
                &tables::TableProps {
                    hidden: *hidden,
                    data_category: data_category.as_deref(),
                    description: description.as_deref(),
                },
            )
            .await
        }
        SemanticModelCommand::ListCultures { workspace, id } => {
            translations::list_cultures(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddCulture {
            workspace,
            id,
            culture,
        } => translations::add_culture(cli, client, workspace, id, culture).await,
        SemanticModelCommand::DeleteCulture {
            workspace,
            id,
            culture,
        } => translations::delete_culture(cli, client, workspace, id, culture).await,
        SemanticModelCommand::SetTranslation {
            workspace,
            id,
            culture,
            table,
            column,
            measure,
            caption,
        } => {
            translations::set_translation(
                cli,
                client,
                workspace,
                id,
                culture,
                table,
                column.as_deref(),
                measure.as_deref(),
                caption,
            )
            .await
        }
        SemanticModelCommand::ListHierarchies {
            workspace,
            id,
            table,
        } => hierarchies::list_hierarchies(cli, client, workspace, id, table.as_deref()).await,
        SemanticModelCommand::AddHierarchy {
            workspace,
            id,
            table,
            name,
            levels,
        } => {
            let parsed: Result<Vec<hierarchies::Level>> = levels
                .iter()
                .map(|s| hierarchies::parse_level_spec(s))
                .collect();
            hierarchies::add_hierarchy(cli, client, workspace, id, table, name, &parsed?).await
        }
        SemanticModelCommand::DeleteHierarchy {
            workspace,
            id,
            table,
            name,
        } => hierarchies::delete_hierarchy(cli, client, workspace, id, table, name).await,
        SemanticModelCommand::ListPartitions {
            workspace,
            id,
            table,
        } => partitions::list_partitions(cli, client, workspace, id, table.as_deref()).await,
        SemanticModelCommand::AddPartition {
            workspace,
            id,
            table,
            name,
            m,
            dax,
        } => {
            let (kind, expr) = resolve_partition_source(m.as_deref(), dax.as_deref())?;
            partitions::add_partition(cli, client, workspace, id, table, name, kind, expr).await
        }
        SemanticModelCommand::UpdatePartition {
            workspace,
            id,
            table,
            name,
            m,
            dax,
        } => {
            let (kind, expr) = resolve_partition_source(m.as_deref(), dax.as_deref())?;
            partitions::update_partition(cli, client, workspace, id, table, name, kind, expr).await
        }
        SemanticModelCommand::DeletePartition {
            workspace,
            id,
            table,
            name,
        } => partitions::delete_partition(cli, client, workspace, id, table, name).await,
        SemanticModelCommand::ListCalculationGroups { workspace, id } => {
            calc_groups::list_calculation_groups(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddCalculationGroup {
            workspace,
            id,
            name,
            column_name,
        } => {
            let col = column_name.as_deref().unwrap_or(name);
            calc_groups::add_calculation_group(cli, client, workspace, id, name, col).await
        }
        SemanticModelCommand::DeleteCalculationGroup {
            workspace,
            id,
            name,
        } => calc_groups::delete_calculation_group(cli, client, workspace, id, name).await,
        SemanticModelCommand::AddCalculationItem {
            workspace,
            id,
            group,
            name,
            expression,
            ordinal,
        } => {
            calc_groups::add_calculation_item(
                cli, client, workspace, id, group, name, expression, *ordinal,
            )
            .await
        }
        SemanticModelCommand::DeleteCalculationItem {
            workspace,
            id,
            group,
            name,
        } => calc_groups::delete_calculation_item(cli, client, workspace, id, group, name).await,
        SemanticModelCommand::ListExpressions { workspace, id } => {
            expressions::list_expressions(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddExpression {
            workspace,
            id,
            name,
            expression,
            parameter_value,
            parameter_type,
        } => {
            let m = expressions::resolve_m(
                expression.as_deref(),
                parameter_value.as_deref(),
                parameter_type.as_deref(),
            )?;
            expressions::add_expression(cli, client, workspace, id, name, &m).await
        }
        SemanticModelCommand::UpdateExpression {
            workspace,
            id,
            name,
            expression,
            parameter_value,
            parameter_type,
        } => {
            let m = expressions::resolve_m(
                expression.as_deref(),
                parameter_value.as_deref(),
                parameter_type.as_deref(),
            )?;
            expressions::update_expression(cli, client, workspace, id, name, &m).await
        }
        SemanticModelCommand::DeleteExpression {
            workspace,
            id,
            name,
        } => expressions::delete_expression(cli, client, workspace, id, name).await,
        SemanticModelCommand::ListFunctions { workspace, id } => {
            functions::list_functions(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddFunction {
            workspace,
            id,
            name,
            expression,
        } => functions::add_function(cli, client, workspace, id, name, expression).await,
        SemanticModelCommand::UpdateFunction {
            workspace,
            id,
            name,
            expression,
        } => functions::update_function(cli, client, workspace, id, name, expression).await,
        SemanticModelCommand::DeleteFunction {
            workspace,
            id,
            name,
        } => functions::delete_function(cli, client, workspace, id, name).await,
        SemanticModelCommand::ListPerspectives { workspace, id } => {
            perspectives::list_perspectives(cli, client, workspace, id).await
        }
        SemanticModelCommand::AddPerspective {
            workspace,
            id,
            name,
        } => perspectives::add_perspective(cli, client, workspace, id, name).await,
        SemanticModelCommand::DeletePerspective {
            workspace,
            id,
            name,
        } => perspectives::delete_perspective(cli, client, workspace, id, name).await,
        SemanticModelCommand::AddPerspectiveMember {
            workspace,
            id,
            perspective,
            table,
            column,
            measure,
            hierarchy,
        } => {
            let member = resolve_perspective_member(
                column.as_deref(),
                measure.as_deref(),
                hierarchy.as_deref(),
            )?;
            perspectives::add_perspective_member(
                cli,
                client,
                workspace,
                id,
                perspective,
                table,
                member,
            )
            .await
        }
        SemanticModelCommand::RemovePerspectiveMember {
            workspace,
            id,
            perspective,
            table,
            column,
            measure,
            hierarchy,
        } => {
            let member = resolve_perspective_member(
                column.as_deref(),
                measure.as_deref(),
                hierarchy.as_deref(),
            )?;
            perspectives::remove_perspective_member(
                cli,
                client,
                workspace,
                id,
                perspective,
                table,
                member,
            )
            .await
        }
        _ => unreachable!("execute_authoring only handles granular-authoring commands"),
    }
}

/// Resolve the relationship match spec for delete/update: either a
/// `--relationship-id` (spec is `None`) or all four from/to column flags.
type RelTuple = (String, String, String, String);

fn build_rel_spec(
    from_table: Option<&str>,
    from_column: Option<&str>,
    to_table: Option<&str>,
    to_column: Option<&str>,
    relationship_id: Option<&str>,
) -> Result<Option<RelTuple>> {
    match (from_table, from_column, to_table, to_column) {
        (Some(ft), Some(fc), Some(tt), Some(tc)) => Ok(Some((
            ft.to_string(),
            fc.to_string(),
            tt.to_string(),
            tc.to_string(),
        ))),
        (None, None, None, None) => {
            if relationship_id.is_some() {
                Ok(None)
            } else {
                Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "Specify which relationship to target.".to_string(),
                    "Pass --relationship-id <guid>, or all of --from-table/--from-column/--to-table/--to-column."
                        .to_string(),
                )
                .into())
            }
        }
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Incomplete relationship column selector.".to_string(),
            "When matching by columns, pass ALL of --from-table/--from-column/--to-table/--to-column."
                .to_string(),
        )
        .into()),
    }
}

/// Resolve the optional column/measure/hierarchy selector for a perspective
/// member (at most one). Returns `None` for a whole-table member.
fn resolve_perspective_member<'a>(
    column: Option<&'a str>,
    measure: Option<&'a str>,
    hierarchy: Option<&'a str>,
) -> Result<Option<(perspectives::MemberKind, &'a str)>> {
    match (column, measure, hierarchy) {
        (None, None, None) => Ok(None),
        (Some(c), None, None) => Ok(Some((perspectives::MemberKind::Column, c))),
        (None, Some(m), None) => Ok(Some((perspectives::MemberKind::Measure, m))),
        (None, None, Some(h)) => Ok(Some((perspectives::MemberKind::Hierarchy, h))),
        _ => Err(FabioError::invalid_input(
            "Specify at most one of --column / --measure / --hierarchy".to_string(),
        )
        .into()),
    }
}

/// Resolve the `--m` / `--dax` choice for partition add/update (exactly one).
fn resolve_partition_source<'a>(
    m: Option<&'a str>,
    dax: Option<&'a str>,
) -> Result<(partitions::SourceKind, &'a str)> {
    match (m, dax) {
        (Some(expr), None) => Ok((partitions::SourceKind::M, expr)),
        (None, Some(expr)) => Ok((partitions::SourceKind::Calculated, expr)),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Specify exactly one partition source.".to_string(),
            "Pass --m <M expression> (Power Query) OR --dax <expression> (calculated).".to_string(),
        )
        .into()),
    }
}

pub(super) fn parse_json_content(content: &str, command: &str) -> Result<Value> {
    serde_json::from_str(content).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON in --content: {e}"),
            format!(
                "Example: fabio semantic-model {command} --content '{{\"updateDetails\":[...]}}'"
            ),
        )
        .into()
    })
}
