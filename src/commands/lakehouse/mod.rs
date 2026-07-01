use std::path::Path;

use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

mod crud;
mod definitions;
mod execution_definitions;
mod files;
mod iceberg;
mod livy;
mod maintenance;
mod shortcuts;
mod sync;
mod tables;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples lakehouse\nAlso available: fabio context schema Lakehouse | fabio context workflow lakehouse-etl"
)]
pub enum LakehouseCommand {
    // ── CRUD ─────────────────────────────────────────────────────────────
    /// List lakehouses in a workspace
    #[command(display_order = 0)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a lakehouse
    #[command(display_order = 0)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },
    /// Create a new lakehouse
    #[command(display_order = 0)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Enable schemas (multi-schema lakehouse)
        #[arg(long)]
        enable_schemas: bool,
    },
    /// Update a lakehouse (rename/redescribe)
    #[command(display_order = 0)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a lakehouse
    #[command(display_order = 0)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── List ─────────────────────────────────────────────────────────────
    /// List tables in a lakehouse
    #[command(visible_alias = "tables", display_order = 1)]
    ListTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },
    /// List files in a lakehouse
    #[command(visible_alias = "files", display_order = 2)]
    ListFiles {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Directory path to list (default: root)
        #[arg(short, long)]
        path: Option<String>,
    },

    // ── Query ────────────────────────────────────────────────────────────
    /// Execute SQL against the lakehouse SQL endpoint
    #[command(display_order = 3)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// SQL query to execute (prefix with @ to read from file, omit to read from stdin)
        #[arg(long)]
        sql: Option<String>,
    },

    // ── Read/Write ───────────────────────────────────────────────────────
    /// Upload files to a lakehouse (supports glob patterns for parallel upload)
    #[command(
        display_order = 10,
        after_help = "TIP: For incremental sync (only upload new/changed files), use: fabio lakehouse sync --local <dir> --dest-workspace <ws> --dest-id <id> --dest-path <path>"
    )]
    Upload {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Local source path (supports glob patterns, e.g. ./data/*.csv)
        #[arg(short = 's', long = "source-path", visible_alias = "source")]
        source_path: String,

        /// Remote destination path (directory when uploading multiple files)
        #[arg(short = 'd', long = "dest-path", visible_alias = "dest")]
        dest_path: String,
    },
    /// Download a file from a lakehouse
    #[command(display_order = 11)]
    Download {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Remote source path
        #[arg(short = 's', long = "source-path", visible_alias = "source")]
        source_path: String,

        /// Local destination path
        #[arg(short = 'd', long = "dest-path", visible_alias = "dest")]
        dest_path: String,
    },
    /// Upload a local file and load it into a Delta table (upload + load-table in one step)
    #[command(display_order = 12)]
    UploadTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Local source file path (e.g., ./data.csv)
        #[arg(short = 's', long = "source-path")]
        source_path: String,

        /// Table name
        #[arg(short = 't', long)]
        table: String,

        /// Load mode: Overwrite or Append
        #[arg(short, long, default_value = "Overwrite")]
        mode: String,

        /// File format: Csv, Parquet (auto-detected from extension if omitted)
        #[arg(short, long)]
        format: Option<String>,
    },
    /// Load a file (already in the lakehouse) into a Delta table
    #[command(display_order = 13)]
    LoadTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Relative path to the source file (e.g., Files/data.csv)
        #[arg(short = 's', long = "source-path", visible_alias = "path")]
        source_path: String,

        /// Table name
        #[arg(short = 't', long)]
        table: String,

        /// Load mode: Overwrite or Append
        #[arg(short, long, default_value = "Overwrite")]
        mode: String,

        /// File format: Csv, Parquet
        #[arg(short, long, default_value = "Csv")]
        format: String,

        /// Wait for completion (no-op: load-table always waits via LRO polling)
        #[arg(long, hide = true)]
        wait: bool,

        /// CSV file does NOT have a header row (by default, header is assumed present)
        #[arg(long = "no-header")]
        no_header: bool,

        /// CSV delimiter character (default: comma)
        #[arg(long, default_value = ",")]
        delimiter: String,

        /// Schema name for multi-schema lakehouses (beta; e.g., dbo)
        #[arg(long)]
        schema: Option<String>,
    },

    // ── Copy/Move/Sync ───────────────────────────────────────────────────
    /// Copy files between lakehouses (supports glob patterns for parallel copy)
    #[command(display_order = 20)]
    CopyFile {
        /// Source workspace ID
        #[arg(long, alias = "source-workspace")]
        source_workspace: String,

        /// Source lakehouse ID
        #[arg(long, alias = "source-id")]
        source_id: String,

        /// Source file path (supports glob patterns, e.g. Files/data/*.csv)
        #[arg(short = 's', long = "source-path")]
        source_path: String,

        /// Destination workspace ID
        #[arg(long, alias = "dest-workspace")]
        dest_workspace: String,

        /// Destination lakehouse ID
        #[arg(long, alias = "dest-id")]
        dest_id: String,

        /// Destination path (directory when copying multiple files)
        #[arg(short = 'd', long = "dest-path")]
        dest_path: String,
    },
    /// Move files between lakehouses (supports glob patterns for parallel move)
    #[command(display_order = 21)]
    MoveFile {
        /// Source workspace ID
        #[arg(long, alias = "source-workspace")]
        source_workspace: String,

        /// Source lakehouse ID
        #[arg(long, alias = "source-id")]
        source_id: String,

        /// Source file path (supports glob patterns, e.g. Files/data/*.csv)
        #[arg(short = 's', long = "source-path")]
        source_path: String,

        /// Destination workspace ID
        #[arg(long, alias = "dest-workspace")]
        dest_workspace: String,

        /// Destination lakehouse ID
        #[arg(long, alias = "dest-id")]
        dest_id: String,

        /// Destination path (directory when moving multiple files)
        #[arg(short = 'd', long = "dest-path")]
        dest_path: String,
    },
    /// Copy a table between lakehouses
    #[command(display_order = 22)]
    CopyTable {
        /// Source workspace ID
        #[arg(long, alias = "source-workspace")]
        source_workspace: String,

        /// Source lakehouse ID
        #[arg(long, alias = "source-id")]
        source_id: String,

        /// Source table name (supports glob patterns)
        #[arg(short = 's', long = "source-table")]
        source_table: String,

        /// Destination workspace ID
        #[arg(long, alias = "dest-workspace")]
        dest_workspace: String,

        /// Destination lakehouse ID
        #[arg(long, alias = "dest-id")]
        dest_id: String,

        /// Destination table name (ignored for glob patterns)
        #[arg(short = 'd', long = "dest-table")]
        dest_table: Option<String>,
    },
    /// Move a table between lakehouses (copy + delete source)
    #[command(display_order = 23)]
    MoveTable {
        /// Source workspace ID
        #[arg(long, alias = "source-workspace")]
        source_workspace: String,

        /// Source lakehouse ID
        #[arg(long, alias = "source-id")]
        source_id: String,

        /// Source table name (supports glob patterns)
        #[arg(short = 's', long = "source-table")]
        source_table: String,

        /// Destination workspace ID
        #[arg(long, alias = "dest-workspace")]
        dest_workspace: String,

        /// Destination lakehouse ID
        #[arg(long, alias = "dest-id")]
        dest_id: String,

        /// Destination table name (ignored for glob patterns)
        #[arg(short = 'd', long = "dest-table")]
        dest_table: Option<String>,
    },
    /// Sync files between lakehouses (parallel, copies new/modified files)
    #[command(display_order = 24)]
    Sync {
        /// Source workspace ID (omit when using --local)
        #[arg(long, alias = "source-workspace", required_unless_present = "local")]
        source_workspace: Option<String>,

        /// Source lakehouse ID (omit when using --local)
        #[arg(long, alias = "source-id", required_unless_present = "local")]
        source_id: Option<String>,

        /// Source path (e.g. Files/data or Tables/mytable; omit when using --local)
        #[arg(short = 's', long = "source-path", required_unless_present = "local")]
        source_path: Option<String>,

        /// Local directory to sync from (alternative to --source-workspace/--source-id)
        #[arg(long, conflicts_with_all = ["source_workspace", "source_id", "source_path"])]
        local: Option<String>,

        /// Destination workspace ID
        #[arg(long, alias = "dest-workspace")]
        dest_workspace: String,

        /// Destination lakehouse ID
        #[arg(long, alias = "dest-id")]
        dest_id: String,

        /// Destination path
        #[arg(short = 'd', long = "dest-path")]
        dest_path: String,

        /// Delete files at destination that don't exist in source
        #[arg(long)]
        delete: bool,

        /// Use Content-MD5 checksums for comparison (slower, requires HEAD per file)
        #[arg(long)]
        checksum: bool,

        /// Include only files matching these glob patterns (semicolon-separated)
        #[arg(long)]
        include: Option<String>,

        /// Exclude files matching these glob patterns (semicolon-separated)
        #[arg(long)]
        exclude: Option<String>,

        /// Compare only by file size (skip ETag/checksum comparison)
        #[arg(long)]
        size_only: bool,

        /// Only copy files that don't exist at destination (skip existing)
        #[arg(long)]
        no_overwrite: bool,

        /// Force overwrite all files regardless of comparison result
        #[arg(long)]
        force: bool,

        /// Sync only top-level files (do not recurse into subdirectories)
        #[arg(long)]
        no_recursive: bool,

        /// Safety limit: abort deletions if more than NUM files would be deleted
        #[arg(long)]
        max_delete: Option<usize>,

        /// Only update files that already exist at destination (don't create new)
        #[arg(long)]
        existing: bool,

        /// Delete source files after successful transfer (move semantics)
        #[arg(long)]
        remove_source_files: bool,

        /// Skip files smaller than SIZE bytes (supports K, M, G suffixes)
        #[arg(long)]
        min_size: Option<String>,

        /// Skip files larger than SIZE bytes (supports K, M, G suffixes)
        #[arg(long)]
        max_size: Option<String>,

        /// Show per-file actions on stderr (copy, skip, rename, delete)
        #[arg(long)]
        itemize: bool,
    },

    // ── Directory ──────────────────────────────────────────────────────
    /// Create a directory in a lakehouse (DFS)
    #[command(display_order = 29)]
    CreateDirectory {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Directory path to create (e.g. "Files/staging/incoming")
        #[arg(short, long)]
        path: String,
    },

    // ── Delete ───────────────────────────────────────────────────────────
    /// Delete a file from a lakehouse
    #[command(display_order = 30)]
    DeleteFile {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// File path to delete
        #[arg(short, long)]
        path: String,
    },
    /// Delete a table from a lakehouse
    #[command(display_order = 31)]
    DeleteTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Table name (supports glob patterns)
        #[arg(short = 't', long = "table")]
        table: String,
    },

    // ── Shortcuts ────────────────────────────────────────────────────────
    /// Create a shortcut
    #[command(display_order = 40)]
    CreateShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Shortcut name
        #[arg(long)]
        name: String,

        /// Shortcut path (e.g., Tables or Files)
        #[arg(short, long)]
        path: String,

        /// Target type: `OneLake`, `AdlsGen2`, S3
        #[arg(long = "target-type")]
        target_type: String,

        /// Target body as JSON string
        #[arg(long = "target")]
        target: String,

        /// Conflict policy: Abort or `GenerateUniqueName`
        #[arg(long)]
        conflict_policy: Option<String>,
    },
    /// Get shortcut details
    #[command(display_order = 41)]
    GetShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Shortcut name
        #[arg(long)]
        name: String,

        /// Shortcut path
        #[arg(short, long)]
        path: String,
    },
    /// Delete a shortcut
    #[command(display_order = 42)]
    DeleteShortcut {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Shortcut name
        #[arg(long)]
        name: String,

        /// Shortcut path
        #[arg(short, long)]
        path: String,
    },

    /// Bulk-create multiple shortcuts (LRO)
    #[command(display_order = 43)]
    BulkCreateShortcuts {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Path to JSON file with array of shortcut requests
        #[arg(long, group = "input")]
        file: Option<String>,

        /// Inline JSON with array of shortcut requests
        #[arg(long, group = "input")]
        content: Option<String>,

        /// Conflict policy: `Abort`, `GenerateUniqueName`, `CreateOrOverwrite`, `OverwriteOnly`
        #[arg(long = "conflict-policy")]
        conflict_policy: Option<String>,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a lakehouse
    #[command(display_order = 50)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a lakehouse
    #[command(display_order = 51)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Materialized Lake Views ──────────────────────────────────────────
    /// Trigger a refresh of materialized lake views
    #[command(display_order = 60)]
    RefreshMaterializedViews {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },
    /// Create a schedule for materialized lake view refresh
    #[command(display_order = 61)]
    CreateMaterializedViewsSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Schedule definition file path (JSON)
        #[arg(long)]
        file: Option<String>,

        /// Schedule definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },
    /// Update a schedule for materialized lake view refresh
    #[command(display_order = 62)]
    UpdateMaterializedViewsSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,

        /// Schedule definition file path (JSON)
        #[arg(long)]
        file: Option<String>,

        /// Schedule definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },
    /// Delete a schedule for materialized lake view refresh
    #[command(display_order = 63)]
    DeleteMaterializedViewsSchedule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Schedule ID
        #[arg(long)]
        schedule_id: String,
    },

    // ── Materialized Lake View Execution Definitions ─────────────────────
    /// List materialized lake view execution definitions for a lakehouse
    #[command(display_order = 64)]
    ListExecutionDefinitions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },
    /// Show a materialized lake view execution definition
    #[command(display_order = 64)]
    ShowExecutionDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Materialized lake view execution definition ID
        #[arg(long)]
        execution_definition_id: String,
    },
    /// Create a materialized lake view execution definition
    #[command(display_order = 64)]
    CreateExecutionDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Execution definition file path (JSON, must include displayName and currentLakehouseExecutionContext)
        #[arg(long)]
        file: Option<String>,

        /// Execution definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },
    /// Update a materialized lake view execution definition
    #[command(display_order = 64)]
    UpdateExecutionDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Materialized lake view execution definition ID
        #[arg(long)]
        execution_definition_id: String,

        /// Execution definition file path (JSON, only provided fields are updated)
        #[arg(long)]
        file: Option<String>,

        /// Execution definition content (inline JSON)
        #[arg(long)]
        content: Option<String>,
    },
    /// Delete a materialized lake view execution definition
    #[command(display_order = 64)]
    DeleteExecutionDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Materialized lake view execution definition ID
        #[arg(long)]
        execution_definition_id: String,
    },

    // ── Table Maintenance ────────────────────────────────────────────────
    /// Run table maintenance on a lakehouse
    #[command(display_order = 70)]
    RunTableMaintenance {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Configuration file path (optional JSON)
        #[arg(long)]
        file: Option<String>,

        /// Configuration content (optional inline JSON)
        #[arg(long)]
        content: Option<String>,
    },

    /// Optimize a Delta table (V-Order compaction + optional Z-Order)
    #[command(display_order = 71)]
    OptimizeTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Table name to optimize
        #[arg(long)]
        table: String,

        /// Schema name (for multi-schema lakehouses)
        #[arg(long)]
        schema: Option<String>,

        /// Enable V-Order optimization
        #[arg(long, default_value_t = true)]
        vorder: bool,

        /// Columns for Z-Order clustering (comma-separated)
        #[arg(long, value_delimiter = ',')]
        zorder: Option<Vec<String>>,
    },

    /// Vacuum a Delta table (remove old files beyond retention period)
    #[command(display_order = 72)]
    VacuumTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Table name to vacuum
        #[arg(long)]
        table: String,

        /// Schema name (for multi-schema lakehouses)
        #[arg(long)]
        schema: Option<String>,

        /// Retention period in hours (default: 168 = 7 days)
        #[arg(long, default_value_t = 168)]
        retain_hours: u64,
    },

    /// Show Delta table schema (reads from `OneLake` `_delta_log` without Spark/SQL)
    #[command(display_order = 73)]
    TableSchema {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    // ── Iceberg REST Catalog (OneLake Table API) ──────────────────────
    /// Get Iceberg REST Catalog configuration for a lakehouse
    #[command(display_order = 74)]
    IcebergConfig {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },

    /// List table namespaces (schemas) via the Iceberg REST Catalog
    #[command(display_order = 75)]
    IcebergNamespaces {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },

    /// Get metadata for a specific namespace via the Iceberg REST Catalog
    #[command(display_order = 76)]
    IcebergNamespace {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,
    },

    /// List tables in a namespace via the Iceberg REST Catalog
    #[command(display_order = 77)]
    IcebergTables {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,
    },

    /// Get table definition (schema, partitions, properties) via the Iceberg REST Catalog
    #[command(display_order = 78)]
    IcebergTable {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    /// Check if a table exists via the Iceberg REST Catalog (lightweight HEAD)
    #[command(display_order = 79)]
    IcebergTableExists {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    /// Check if a namespace exists via the Iceberg REST Catalog (lightweight HEAD)
    #[command(display_order = 79)]
    IcebergNamespaceExists {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,
    },

    /// Load vended storage credentials scoped to a specific table
    #[command(display_order = 79)]
    IcebergCredentials {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    /// Show table statistics from the latest Iceberg snapshot (record/file counts, size)
    #[command(display_order = 79)]
    IcebergStats {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    /// Show snapshot history for a table via the Iceberg REST Catalog
    #[command(display_order = 79)]
    IcebergSnapshots {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Namespace name (e.g. "dbo")
        #[arg(long)]
        namespace: String,

        /// Table name
        #[arg(long)]
        table: String,
    },

    // ── Livy Sessions ────────────────────────────────────────────────────
    /// List Livy sessions for a lakehouse
    #[command(display_order = 80)]
    ListLivySessions {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,
    },
    /// Get details of a Livy session for a lakehouse
    #[command(display_order = 81)]
    GetLivySession {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Lakehouse ID
        #[arg(long, visible_alias = "lakehouse")]
        id: String,

        /// Livy session ID
        #[arg(long)]
        livy_id: String,
    },
}

#[allow(clippy::too_many_lines, clippy::large_futures)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &LakehouseCommand) -> Result<()> {
    match command {
        LakehouseCommand::List { workspace } => crud::list_lakehouses(cli, client, workspace).await,
        LakehouseCommand::Show { workspace, id } => {
            crud::show_lakehouse(cli, client, workspace, id).await
        }
        LakehouseCommand::Create {
            workspace,
            name,
            description,
            enable_schemas,
        } => {
            crud::create_lakehouse(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                *enable_schemas,
            )
            .await
        }
        LakehouseCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            crud::update_lakehouse(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        LakehouseCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => crud::delete_lakehouse(cli, client, workspace, id, *hard_delete).await,
        LakehouseCommand::ListTables { workspace, id } => {
            files::tables(cli, client, workspace, id).await
        }
        LakehouseCommand::ListFiles {
            workspace,
            id,
            path,
        } => files::files(cli, client, workspace, id, path.as_deref()).await,
        LakehouseCommand::Query { workspace, id, sql } => {
            crud::query_lakehouse(cli, client, workspace, id, sql.as_deref()).await
        }
        LakehouseCommand::Upload {
            workspace,
            id,
            source_path,
            dest_path,
        } => files::upload(cli, client, workspace, id, source_path, dest_path)
            .await
            .map_err(|e| enrich_forbidden(e, "lakehouse upload", "Contributor")),
        LakehouseCommand::Download {
            workspace,
            id,
            source_path,
            dest_path,
        } => files::download(cli, client, workspace, id, source_path, dest_path).await,
        LakehouseCommand::UploadTable {
            workspace,
            id,
            source_path,
            table,
            mode,
            format,
        } => tables::upload_table(
            cli,
            client,
            workspace,
            id,
            source_path,
            table,
            mode,
            format.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse upload-table", "Contributor")),
        LakehouseCommand::LoadTable {
            workspace,
            id,
            source_path,
            table,
            mode,
            format,
            wait: _,
            no_header,
            delimiter,
            schema,
        } => tables::load_table(
            cli,
            client,
            workspace,
            id,
            source_path,
            table,
            mode,
            format,
            !*no_header,
            delimiter,
            schema.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse load-table", "Contributor")),
        LakehouseCommand::CopyFile {
            source_workspace,
            source_id,
            source_path,
            dest_workspace,
            dest_id,
            dest_path,
        } => files::copy_file(
            cli,
            client,
            source_workspace,
            source_id,
            source_path,
            dest_workspace,
            dest_id,
            dest_path,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse copy-file", "Contributor")),
        LakehouseCommand::CreateDirectory {
            workspace,
            id,
            path,
        } => files::create_directory(cli, client, workspace, id, path)
            .await
            .map_err(|e| enrich_forbidden(e, "lakehouse create-directory", "Contributor")),
        LakehouseCommand::DeleteFile {
            workspace,
            id,
            path,
        } => files::delete_file(cli, client, workspace, id, path)
            .await
            .map_err(|e| enrich_forbidden(e, "lakehouse delete-file", "Contributor")),
        LakehouseCommand::MoveFile {
            source_workspace,
            source_id,
            source_path,
            dest_workspace,
            dest_id,
            dest_path,
        } => files::move_file(
            cli,
            client,
            source_workspace,
            source_id,
            source_path,
            dest_workspace,
            dest_id,
            dest_path,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse move-file", "Contributor")),
        LakehouseCommand::DeleteTable {
            workspace,
            id,
            table,
        } => tables::delete_table(cli, client, workspace, id, table)
            .await
            .map_err(|e| enrich_forbidden(e, "lakehouse delete-table", "Contributor")),
        LakehouseCommand::CopyTable {
            source_workspace,
            source_id,
            source_table,
            dest_workspace,
            dest_id,
            dest_table,
        } => tables::copy_table(
            cli,
            client,
            source_workspace,
            source_id,
            source_table,
            dest_workspace,
            dest_id,
            dest_table.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse copy-table", "Contributor")),
        LakehouseCommand::MoveTable {
            source_workspace,
            source_id,
            source_table,
            dest_workspace,
            dest_id,
            dest_table,
        } => tables::move_table(
            cli,
            client,
            source_workspace,
            source_id,
            source_table,
            dest_workspace,
            dest_id,
            dest_table.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse move-table", "Contributor")),
        LakehouseCommand::Sync {
            source_workspace,
            source_id,
            source_path,
            local,
            dest_workspace,
            dest_id,
            dest_path,
            delete,
            checksum,
            include,
            exclude,
            size_only,
            no_overwrite,
            force,
            no_recursive,
            max_delete,
            existing,
            remove_source_files,
            min_size,
            max_size,
            itemize,
        } => sync::sync_files(
            cli,
            client,
            source_workspace.as_deref(),
            source_id.as_deref(),
            source_path.as_deref(),
            local.as_deref(),
            dest_workspace,
            dest_id,
            dest_path,
            *delete,
            *checksum,
            include.as_deref(),
            exclude.as_deref(),
            *size_only,
            *no_overwrite,
            *force,
            *no_recursive,
            *max_delete,
            *existing,
            *remove_source_files,
            min_size.as_deref(),
            max_size.as_deref(),
            *itemize,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse sync", "Contributor")),
        LakehouseCommand::CreateShortcut {
            workspace,
            id,
            name,
            path,
            target_type,
            target,
            conflict_policy,
        } => shortcuts::create_shortcut(
            cli,
            client,
            workspace,
            id,
            name,
            path,
            target_type,
            target,
            conflict_policy.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse create-shortcut", "Contributor")),
        LakehouseCommand::GetShortcut {
            workspace,
            id,
            name,
            path,
        } => shortcuts::get_shortcut(cli, client, workspace, id, name, path).await,
        LakehouseCommand::DeleteShortcut {
            workspace,
            id,
            name,
            path,
        } => shortcuts::delete_shortcut(cli, client, workspace, id, name, path)
            .await
            .map_err(|e| enrich_forbidden(e, "lakehouse delete-shortcut", "Contributor")),
        LakehouseCommand::BulkCreateShortcuts {
            workspace,
            id,
            file,
            content,
            conflict_policy,
        } => shortcuts::bulk_create_shortcuts(
            cli,
            client,
            workspace,
            id,
            file.as_deref(),
            content.as_deref(),
            conflict_policy.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse bulk-create-shortcuts", "Contributor")),
        LakehouseCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => definitions::get_definition(cli, client, workspace, id, *decode).await,
        LakehouseCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            definitions::update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::RefreshMaterializedViews { workspace, id } => {
            maintenance::refresh_materialized_views(cli, client, workspace, id).await
        }
        LakehouseCommand::CreateMaterializedViewsSchedule {
            workspace,
            id,
            file,
            content,
        } => {
            maintenance::create_materialized_views_schedule(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::UpdateMaterializedViewsSchedule {
            workspace,
            id,
            schedule_id,
            file,
            content,
        } => {
            maintenance::update_materialized_views_schedule(
                cli,
                client,
                workspace,
                id,
                schedule_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::DeleteMaterializedViewsSchedule {
            workspace,
            id,
            schedule_id,
        } => {
            maintenance::delete_materialized_views_schedule(cli, client, workspace, id, schedule_id)
                .await
        }
        LakehouseCommand::ListExecutionDefinitions { workspace, id } => {
            execution_definitions::list_execution_definitions(cli, client, workspace, id).await
        }
        LakehouseCommand::ShowExecutionDefinition {
            workspace,
            id,
            execution_definition_id,
        } => {
            execution_definitions::show_execution_definition(
                cli,
                client,
                workspace,
                id,
                execution_definition_id,
            )
            .await
        }
        LakehouseCommand::CreateExecutionDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            execution_definitions::create_execution_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::UpdateExecutionDefinition {
            workspace,
            id,
            execution_definition_id,
            file,
            content,
        } => {
            execution_definitions::update_execution_definition(
                cli,
                client,
                workspace,
                id,
                execution_definition_id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::DeleteExecutionDefinition {
            workspace,
            id,
            execution_definition_id,
        } => {
            execution_definitions::delete_execution_definition(
                cli,
                client,
                workspace,
                id,
                execution_definition_id,
            )
            .await
        }
        LakehouseCommand::RunTableMaintenance {
            workspace,
            id,
            file,
            content,
        } => {
            maintenance::run_table_maintenance(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        LakehouseCommand::OptimizeTable {
            workspace,
            id,
            table,
            schema,
            vorder,
            zorder,
        } => {
            maintenance::optimize_table(
                cli,
                client,
                workspace,
                id,
                table,
                schema.as_deref(),
                *vorder,
                zorder.as_deref(),
            )
            .await
        }
        LakehouseCommand::VacuumTable {
            workspace,
            id,
            table,
            schema,
            retain_hours,
        } => {
            maintenance::vacuum_table(
                cli,
                client,
                workspace,
                id,
                table,
                schema.as_deref(),
                *retain_hours,
            )
            .await
        }
        LakehouseCommand::TableSchema {
            workspace,
            id,
            table,
        } => maintenance::table_schema(cli, client, workspace, id, table).await,
        LakehouseCommand::IcebergConfig { workspace, id } => {
            iceberg::iceberg_config(cli, client, workspace, id).await
        }
        LakehouseCommand::IcebergNamespaces { workspace, id } => {
            iceberg::iceberg_namespaces(cli, client, workspace, id).await
        }
        LakehouseCommand::IcebergNamespace {
            workspace,
            id,
            namespace,
        } => iceberg::iceberg_namespace(cli, client, workspace, id, namespace).await,
        LakehouseCommand::IcebergTables {
            workspace,
            id,
            namespace,
        } => iceberg::iceberg_tables(cli, client, workspace, id, namespace).await,
        LakehouseCommand::IcebergTable {
            workspace,
            id,
            namespace,
            table,
        } => iceberg::iceberg_table(cli, client, workspace, id, namespace, table).await,
        LakehouseCommand::IcebergTableExists {
            workspace,
            id,
            namespace,
            table,
        } => iceberg::iceberg_table_exists(cli, client, workspace, id, namespace, table).await,
        LakehouseCommand::IcebergNamespaceExists {
            workspace,
            id,
            namespace,
        } => iceberg::iceberg_namespace_exists(cli, client, workspace, id, namespace).await,
        LakehouseCommand::IcebergCredentials {
            workspace,
            id,
            namespace,
            table,
        } => iceberg::iceberg_credentials(cli, client, workspace, id, namespace, table).await,
        LakehouseCommand::IcebergStats {
            workspace,
            id,
            namespace,
            table,
        } => iceberg::iceberg_stats(cli, client, workspace, id, namespace, table).await,
        LakehouseCommand::IcebergSnapshots {
            workspace,
            id,
            namespace,
            table,
        } => iceberg::iceberg_snapshots(cli, client, workspace, id, namespace, table).await,
        LakehouseCommand::ListLivySessions { workspace, id } => {
            livy::list_livy_sessions(cli, client, workspace, id).await
        }
        LakehouseCommand::GetLivySession {
            workspace,
            id,
            livy_id,
        } => livy::get_livy_session(cli, client, workspace, id, livy_id).await,
    }
}

// ─── Shared Helpers ──────────────────────────────────────────────────────────

/// Detect the file format (Csv, Parquet) from a file extension. JSON is not supported by the API.
pub(super) fn detect_format_from_extension(path: &str) -> Result<String> {
    let ext = Path::new(path)
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "csv" | "tsv" => Ok("Csv".to_string()),
        "parquet" | "pq" => Ok("Parquet".to_string()),
        "json" | "jsonl" | "ndjson" => Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "JSON format is not supported by the Fabric load-table API".to_string(),
            "Convert to CSV or Parquet first. The load-table API only supports Csv and Parquet formats.".to_string(),
        )
        .into()),
        _ => Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            format!("Cannot detect format from extension '.{ext}'"),
            "Use --format to specify one of: Csv, Parquet".to_string(),
        )
        .into()),
    }
}

/// Check if a path contains glob metacharacters.
pub(super) fn is_glob_pattern(path: &str) -> bool {
    path.contains('*') || path.contains('?') || path.contains('[')
}

/// Parse a human-readable size value (e.g., "100", "10K", "5M", "1G") into bytes.
pub(super) fn parse_size_value(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Err(crate::errors::FabioError::invalid_input("Size value cannot be empty").into());
    }

    let (num_str, multiplier) = if s.ends_with('K') || s.ends_with('k') {
        (&s[..s.len() - 1], 1024u64)
    } else if s.ends_with('M') || s.ends_with('m') {
        (&s[..s.len() - 1], 1024 * 1024)
    } else if s.ends_with('G') || s.ends_with('g') {
        (&s[..s.len() - 1], 1024 * 1024 * 1024)
    } else {
        (s, 1u64)
    };

    let num: u64 = num_str.parse().map_err(|_| {
        crate::errors::FabioError::invalid_input(format!(
            "Invalid size value '{s}'. Use a number with optional K, M, or G suffix (e.g., 1024, 10K, 5M, 1G)"
        ))
    })?;

    Ok(num * multiplier)
}

/// Parse a semicolon-separated filter string into glob `Pattern` objects.
pub(super) fn parse_filter_patterns(filter: &str) -> Vec<glob::Pattern> {
    filter
        .split(';')
        .filter(|s| !s.is_empty())
        .filter_map(|p| glob::Pattern::new(p).ok())
        .collect()
}

/// Check whether a relative path matches include/exclude filters.
///
/// Rules (same semantics as `aws s3 sync` / `azcopy sync`):
/// - If `--include` is specified, file must match at least one include pattern
/// - If `--exclude` is specified, file must NOT match any exclude pattern
/// - Patterns are matched against the filename AND the full relative path
pub(super) fn matches_filters(
    rel_path: &str,
    include: Option<&Vec<glob::Pattern>>,
    exclude: Option<&Vec<glob::Pattern>>,
) -> bool {
    // Extract filename for pattern matching against just the name
    let filename = rel_path.rsplit('/').next().unwrap_or(rel_path);

    // Check include: if specified, at least one pattern must match
    if let Some(patterns) = include {
        let included = patterns
            .iter()
            .any(|p| p.matches(filename) || p.matches(rel_path));
        if !included {
            return false;
        }
    }

    // Check exclude: if any pattern matches, file is excluded
    if let Some(patterns) = exclude {
        let excluded = patterns
            .iter()
            .any(|p| p.matches(filename) || p.matches(rel_path));
        if excluded {
            return false;
        }
    }

    true
}

/// Expand a local file glob pattern into a list of matching file paths.
pub(super) fn expand_local_glob(pattern: &str) -> Result<Vec<String>> {
    if !is_glob_pattern(pattern) {
        // Not a glob — treat as a single file or directory
        let path = Path::new(pattern);
        if path.is_dir() {
            // Upload all files in the directory
            let mut files = Vec::new();
            for entry in std::fs::read_dir(path).map_err(|e| {
                crate::errors::FabioError::invalid_input(format!(
                    "Cannot read directory {pattern}: {e}"
                ))
            })? {
                let entry = entry.map_err(|e| {
                    crate::errors::FabioError::invalid_input(format!("Directory read error: {e}"))
                })?;
                if entry
                    .file_type()
                    .is_ok_and(|ft| ft.is_file() && !ft.is_symlink())
                {
                    files.push(entry.path().to_string_lossy().to_string());
                }
            }
            if files.is_empty() {
                return Err(crate::errors::FabioError::invalid_input(format!(
                    "No files found in directory: {pattern}"
                ))
                .into());
            }
            files.sort();
            return Ok(files);
        }
        return Ok(vec![pattern.to_string()]);
    }

    let matches: Vec<String> = glob::glob(pattern)
        .map_err(|e| {
            crate::errors::FabioError::invalid_input(format!("Invalid glob pattern: {e}"))
        })?
        .filter_map(Result::ok)
        .filter(|p| {
            p.is_file()
                && !p
                    .symlink_metadata()
                    .is_ok_and(|m| m.file_type().is_symlink())
        })
        .map(|p| p.to_string_lossy().to_string())
        .collect();

    if matches.is_empty() {
        return Err(crate::errors::FabioError::invalid_input(format!(
            "No files matched pattern: {pattern}"
        ))
        .into());
    }

    Ok(matches)
}

/// Expand a remote glob pattern by listing files and filtering with fnmatch.
pub(super) async fn expand_remote_glob(
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    pattern: &str,
) -> Result<Vec<String>> {
    if !is_glob_pattern(pattern) {
        return Ok(vec![pattern.to_string()]);
    }

    // Extract directory prefix for listing (everything before the first glob char)
    let dir_prefix = pattern
        .find(['*', '?', '['])
        .and_then(|pos| pattern[..pos].rfind('/'))
        .map(|pos| &pattern[..pos]);

    let files = client
        .list_onelake_files(workspace, item_id, dir_prefix)
        .await?;

    let prefix_with_id = format!("{item_id}/");
    let glob_pattern = glob::Pattern::new(pattern)
        .map_err(|e| crate::errors::FabioError::invalid_input(format!("Invalid pattern: {e}")))?;
    let match_opts = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    let matched: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let name = f.get("name").and_then(Value::as_str)?;
            let is_dir = f
                .get("isDirectory")
                .and_then(Value::as_str)
                .unwrap_or("false")
                == "true";
            if is_dir {
                return None;
            }
            // Strip item ID prefix to get the logical path
            let logical_path = name.strip_prefix(&prefix_with_id).unwrap_or(name);
            if glob_pattern.matches_with(logical_path, match_opts) {
                Some(logical_path.to_string())
            } else {
                None
            }
        })
        .collect();

    if matched.is_empty() {
        return Err(crate::errors::FabioError::invalid_input(format!(
            "No remote files matched pattern: {pattern}"
        ))
        .into());
    }

    Ok(matched)
}

/// Expand a table name glob pattern against the lakehouse table list.
pub(super) async fn expand_table_glob(
    client: &FabricClient,
    workspace: &str,
    lakehouse_id: &str,
    pattern: &str,
) -> Result<Vec<String>> {
    if !is_glob_pattern(pattern) {
        return Ok(vec![pattern.to_string()]);
    }

    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/lakehouses/{lakehouse_id}/tables"),
            "data",
            true, // Always paginate for glob expansion
            None,
        )
        .await?;

    let glob_pattern = glob::Pattern::new(pattern)
        .map_err(|e| crate::errors::FabioError::invalid_input(format!("Invalid pattern: {e}")))?;
    let match_opts = glob::MatchOptions {
        case_sensitive: true,
        require_literal_separator: false,
        require_literal_leading_dot: false,
    };

    let matched: Vec<String> = resp
        .items
        .iter()
        .filter_map(|t| {
            let name = t.get("name").and_then(Value::as_str)?;
            if glob_pattern.matches_with(name, match_opts) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .collect();

    if matched.is_empty() {
        return Err(crate::errors::FabioError::invalid_input(format!(
            "No tables matched pattern: {pattern}"
        ))
        .into());
    }

    Ok(matched)
}

/// Render a batch operation result (success or partial failure).
pub(super) fn render_batch_result(
    cli: &Cli,
    summary: &crate::parallel::BatchSummary,
    status_verb: &str,
) -> Result<()> {
    if summary.all_succeeded() {
        let obj = serde_json::json!({
            "filesProcessed": summary.succeeded,
            "status": status_verb
        });
        output::render_object(cli, &obj, "status");
        Ok(())
    } else {
        let obj = serde_json::json!({
            "filesProcessed": summary.succeeded,
            "filesFailed": summary.failed,
            "failures": summary.failures,
            "status": "partial_failure"
        });
        output::render_object(cli, &obj, "status");
        Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::ApiError,
            format!(
                "Operation partially failed: {}/{} files {status_verb}",
                summary.succeeded, summary.total
            ),
            "Retry the operation to process remaining files. Failed files may be locked or have permission issues.",
        )
        .into())
    }
}

/// Build the Iceberg warehouse identifier (`{workspace}/{item}`) used in URL paths.
pub(super) fn iceberg_warehouse(workspace: &str, id: &str) -> String {
    format!(
        "{}/{}",
        urlencoding::encode(workspace),
        urlencoding::encode(id)
    )
}

fn read_json_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    match (file, content) {
        (Some(f), _) => {
            let text = std::fs::read_to_string(f)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{f}': {e}"))?;
            Ok(serde_json::from_str(&text)?)
        }
        (_, Some(c)) => Ok(serde_json::from_str(c)?),
        _ => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Either --file or --content must be provided".to_string(),
            format!(
                "Example: fabio lakehouse {command} --workspace <WS> --id <ID> --file config.json"
            ),
        )
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── detect_format_from_extension ────────────────────────────────────

    #[test]
    fn detect_format_csv() {
        assert_eq!(detect_format_from_extension("data.csv").unwrap(), "Csv");
    }

    #[test]
    fn detect_format_tsv() {
        assert_eq!(detect_format_from_extension("data.tsv").unwrap(), "Csv");
    }

    #[test]
    fn detect_format_parquet() {
        assert_eq!(
            detect_format_from_extension("sales.parquet").unwrap(),
            "Parquet"
        );
    }

    #[test]
    fn detect_format_pq_shorthand() {
        assert_eq!(
            detect_format_from_extension("events.pq").unwrap(),
            "Parquet"
        );
    }

    #[test]
    fn detect_format_json_errors() {
        let err = detect_format_from_extension("logs.json").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(fabio_err.message.contains("JSON format is not supported"));
    }

    #[test]
    fn detect_format_jsonl_errors() {
        let err = detect_format_from_extension("stream.jsonl").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(fabio_err.message.contains("JSON format is not supported"));
    }

    #[test]
    fn detect_format_ndjson_errors() {
        let err = detect_format_from_extension("events.ndjson").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
    }

    #[test]
    fn detect_format_case_insensitive() {
        assert_eq!(detect_format_from_extension("DATA.CSV").unwrap(), "Csv");
        assert_eq!(
            detect_format_from_extension("big.PARQUET").unwrap(),
            "Parquet"
        );
        // JSON should error regardless of case
        assert!(detect_format_from_extension("log.JSON").is_err());
    }

    #[test]
    fn detect_format_mixed_case() {
        assert_eq!(detect_format_from_extension("file.Csv").unwrap(), "Csv");
        assert_eq!(
            detect_format_from_extension("file.Parquet").unwrap(),
            "Parquet"
        );
        // JSON should error regardless of case
        assert!(detect_format_from_extension("file.JsonL").is_err());
    }

    #[test]
    fn detect_format_with_path_components() {
        assert_eq!(
            detect_format_from_extension("/tmp/dir/file.csv").unwrap(),
            "Csv"
        );
        assert_eq!(
            detect_format_from_extension("./relative/path.parquet").unwrap(),
            "Parquet"
        );
        // JSON should error even with path components
        assert!(detect_format_from_extension("C:\\Users\\data\\file.json").is_err());
    }

    #[test]
    fn detect_format_with_dots_in_path() {
        assert_eq!(
            detect_format_from_extension("/tmp/my.dir/v1.2/data.csv").unwrap(),
            "Csv"
        );
    }

    #[test]
    fn detect_format_unknown_extension_errors() {
        let err = detect_format_from_extension("data.xlsx").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        assert!(
            fabio_err.message.contains("xlsx"),
            "error message should mention the extension"
        );
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(hint.contains("--format"), "hint should suggest --format");
        assert!(hint.contains("Csv"), "hint should list valid formats");
    }

    #[test]
    fn detect_format_no_extension_errors() {
        let err = detect_format_from_extension("Makefile").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(hint.contains("--format"), "hint should suggest --format");
    }

    #[test]
    fn detect_format_hidden_file_no_extension_errors() {
        let err = detect_format_from_extension(".gitignore").unwrap_err();
        let fabio_err = err.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::InvalidInput);
    }

    // ─── upload_table staging path derivation ────────────────────────────

    #[test]
    fn staging_path_uses_filename_only() {
        let source = "/home/user/data/sales_2024.csv";
        let filename = Path::new(source)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let staging = format!("Files/.staging/{filename}");
        assert_eq!(staging, "Files/.staging/sales_2024.csv");
    }

    #[test]
    fn staging_path_handles_deep_nesting() {
        let source = "/home/user/projects/etl/output/2024/01/report.parquet";
        let filename = Path::new(source)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let staging = format!("Files/.staging/{filename}");
        assert_eq!(staging, "Files/.staging/report.parquet");
    }

    #[test]
    fn staging_path_handles_relative_paths() {
        let source = "./data/events.json";
        let filename = Path::new(source)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let staging = format!("Files/.staging/{filename}");
        assert_eq!(staging, "Files/.staging/events.json");
    }

    // ─── upload_table mode/format validation (inline logic) ──────────────

    #[test]
    fn valid_modes_accepted() {
        const VALID_MODES: &[&str] = &["Overwrite", "Append"];
        assert!(VALID_MODES.contains(&"Overwrite"));
        assert!(VALID_MODES.contains(&"Append"));
        assert!(!VALID_MODES.contains(&"Upsert"));
        assert!(!VALID_MODES.contains(&"Replace"));
    }

    #[test]
    fn case_insensitive_mode_normalization() {
        const VALID_MODES: &[&str] = &["Overwrite", "Append"];
        let normalize = |input: &str| -> String {
            VALID_MODES
                .iter()
                .find(|v| v.eq_ignore_ascii_case(input))
                .map_or_else(|| input.to_string(), |v| (*v).to_string())
        };
        assert_eq!(normalize("overwrite"), "Overwrite");
        assert_eq!(normalize("OVERWRITE"), "Overwrite");
        assert_eq!(normalize("append"), "Append");
        assert_eq!(normalize("APPEND"), "Append");
        assert_eq!(normalize("Upsert"), "Upsert");
    }

    #[test]
    fn case_insensitive_format_normalization() {
        const VALID_FORMATS: &[&str] = &["Csv", "Parquet"];
        let normalize = |input: &str| -> String {
            VALID_FORMATS
                .iter()
                .find(|v| v.eq_ignore_ascii_case(input))
                .map_or_else(|| input.to_string(), |v| (*v).to_string())
        };
        assert_eq!(normalize("csv"), "Csv");
        assert_eq!(normalize("CSV"), "Csv");
        assert_eq!(normalize("parquet"), "Parquet");
        assert_eq!(normalize("PARQUET"), "Parquet");
        assert_eq!(normalize("Json"), "Json");
    }

    #[test]
    fn valid_formats_accepted() {
        const VALID_FORMATS: &[&str] = &["Csv", "Parquet"];
        assert!(VALID_FORMATS.contains(&"Csv"));
        assert!(VALID_FORMATS.contains(&"Parquet"));
        assert!(!VALID_FORMATS.contains(&"Json"));
        assert!(!VALID_FORMATS.contains(&"Avro"));
        assert!(!VALID_FORMATS.contains(&"XML"));
    }

    // ─── is_glob_pattern ─────────────────────────────────────────────────

    #[test]
    fn glob_pattern_detection() {
        assert!(is_glob_pattern("Files/*.csv"));
        assert!(is_glob_pattern("Tables/[a-z]*"));
        assert!(is_glob_pattern("data?.parquet"));
        assert!(!is_glob_pattern("Files/data.csv"));
        assert!(!is_glob_pattern("/plain/path/file.txt"));
    }

    #[test]
    fn glob_pattern_in_directory() {
        assert!(is_glob_pattern("Files/subdir/*.parquet"));
        assert!(is_glob_pattern("Tables/sales_*"));
        assert!(!is_glob_pattern("Tables/sales_2024"));
    }

    // ─── matches_filters ─────────────────────────────────────────────────

    #[test]
    fn filters_include_matches_filename() {
        let include = Some(parse_filter_patterns("*.csv"));
        let result = matches_filters("data/report.csv", include.as_ref(), None);
        assert!(result);
    }

    #[test]
    fn filters_include_rejects_non_match() {
        let include = Some(parse_filter_patterns("*.csv"));
        let result = matches_filters("data/report.parquet", include.as_ref(), None);
        assert!(!result);
    }

    #[test]
    fn filters_exclude_rejects_match() {
        let exclude = Some(parse_filter_patterns("*.tmp"));
        let result = matches_filters("cache/data.tmp", None, exclude.as_ref());
        assert!(!result);
    }

    #[test]
    fn filters_exclude_allows_non_match() {
        let exclude = Some(parse_filter_patterns("*.tmp"));
        let result = matches_filters("data/report.csv", None, exclude.as_ref());
        assert!(result);
    }

    #[test]
    fn filters_include_and_exclude_combined() {
        let include = Some(parse_filter_patterns("*.csv;*.parquet"));
        let exclude = Some(parse_filter_patterns("temp_*"));
        assert!(matches_filters(
            "report.csv",
            include.as_ref(),
            exclude.as_ref()
        ));
        assert!(matches_filters(
            "data.parquet",
            include.as_ref(),
            exclude.as_ref()
        ));
        assert!(!matches_filters(
            "temp_data.csv",
            include.as_ref(),
            exclude.as_ref()
        ));
        assert!(!matches_filters(
            "config.json",
            include.as_ref(),
            exclude.as_ref()
        ));
    }

    #[test]
    fn filters_no_filters_allows_all() {
        assert!(matches_filters("anything.txt", None, None));
        assert!(matches_filters("deep/nested/path.csv", None, None));
    }

    #[test]
    fn filters_multiple_include_patterns() {
        let include = Some(parse_filter_patterns("*.csv;*.parquet;exact.txt"));
        assert!(matches_filters("data.csv", include.as_ref(), None));
        assert!(matches_filters("data.parquet", include.as_ref(), None));
        assert!(matches_filters("exact.txt", include.as_ref(), None));
        assert!(!matches_filters("data.json", include.as_ref(), None));
    }

    #[test]
    fn filters_match_against_full_path() {
        let include = Some(parse_filter_patterns("subdir/*"));
        assert!(matches_filters("subdir/file.txt", include.as_ref(), None));
        assert!(!matches_filters("other/file.txt", include.as_ref(), None));
    }

    #[test]
    fn filters_exclude_directory_pattern() {
        let exclude = Some(parse_filter_patterns("_delta_log/*"));
        assert!(!matches_filters(
            "_delta_log/00000.json",
            None,
            exclude.as_ref()
        ));
        assert!(matches_filters("data/file.parquet", None, exclude.as_ref()));
    }

    // ─── parse_filter_patterns ───────────────────────────────────────────

    #[test]
    fn parse_filters_semicolon_separated() {
        let patterns = parse_filter_patterns("*.csv;*.parquet;*.json");
        assert_eq!(patterns.len(), 3);
    }

    #[test]
    fn parse_filters_empty_segments_skipped() {
        let patterns = parse_filter_patterns("*.csv;;*.parquet;");
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn parse_filters_empty_string() {
        let patterns = parse_filter_patterns("");
        assert!(patterns.is_empty());
    }

    // ─── parse_size_value ────────────────────────────────────────────────

    #[test]
    fn parse_size_plain_bytes() {
        assert_eq!(parse_size_value("1024").unwrap(), 1024);
        assert_eq!(parse_size_value("0").unwrap(), 0);
        assert_eq!(parse_size_value("500").unwrap(), 500);
    }

    #[test]
    fn parse_size_kilobytes() {
        assert_eq!(parse_size_value("1K").unwrap(), 1024);
        assert_eq!(parse_size_value("10k").unwrap(), 10 * 1024);
    }

    #[test]
    fn parse_size_megabytes() {
        assert_eq!(parse_size_value("1M").unwrap(), 1024 * 1024);
        assert_eq!(parse_size_value("5m").unwrap(), 5 * 1024 * 1024);
    }

    #[test]
    fn parse_size_gigabytes() {
        assert_eq!(parse_size_value("1G").unwrap(), 1024 * 1024 * 1024);
        assert_eq!(parse_size_value("2g").unwrap(), 2 * 1024 * 1024 * 1024);
    }

    #[test]
    fn parse_size_invalid() {
        assert!(parse_size_value("abc").is_err());
        assert!(parse_size_value("").is_err());
        assert!(parse_size_value("10X").is_err());
    }

    #[test]
    fn parse_size_with_whitespace() {
        assert_eq!(parse_size_value(" 100 ").unwrap(), 100);
        assert_eq!(parse_size_value(" 5M ").unwrap(), 5 * 1024 * 1024);
    }

    // ─── iceberg_warehouse ──────────────────────────────────────────────

    #[test]
    fn iceberg_warehouse_plain_guids() {
        let result = iceberg_warehouse(
            "aaaaaaaa-1111-2222-3333-444444444444",
            "bbbbbbbb-5555-6666-7777-888888888888",
        );
        assert_eq!(
            result,
            "aaaaaaaa-1111-2222-3333-444444444444/bbbbbbbb-5555-6666-7777-888888888888"
        );
    }

    #[test]
    fn iceberg_warehouse_encodes_special_chars() {
        let result = iceberg_warehouse("ws with spaces", "item/slash");
        assert_eq!(result, "ws%20with%20spaces/item%2Fslash");
    }
}
