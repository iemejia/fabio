use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[path = "report_def.rs"]
mod authoring;
#[path = "report_pbir.rs"]
mod pbir;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before creating reports, run: fabio context schema Report | fabio context workflow direct-lake-report\nReturns definition templates and step-by-step creation recipes."
)]
pub enum ReportCommand {
    /// List reports in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a report
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,
    },
    /// Create a new report from a definition file
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Path to report definition file (definition.pbir JSON)
        #[arg(long, required_unless_present_any = ["dataset", "definition"])]
        file: Option<String>,

        /// Path to a full PBIR report folder (a `.Report` folder or any folder
        /// containing definition.pbir). All files are gathered recursively and
        /// sent as the report definition (PBIR enhanced or PBIR-Legacy).
        #[arg(long)]
        definition: Option<String>,

        /// Dataset/semantic model ID to bind report to (auto-generates definition.pbir).
        /// With --definition, rebinds the folder's definition.pbir to this model by connection.
        #[arg(long)]
        dataset: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update report properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a report
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of a report
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a report
    ///
    /// The Fabric API requires definition.pbir in every update. Use --file for the
    /// semantic model binding (always required) and --report-json to include visual
    /// definitions for PBIR-Legacy format reports.
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Path to definition.pbir file (semantic model binding — always required)
        #[arg(long)]
        file: String,

        /// Path to report.json file (visual definitions for PBIR-Legacy format)
        #[arg(long)]
        report_json: Option<String>,
    },

    // ── Validation ───────────────────────────────────────────────────────
    /// Validate a Power BI report definition on disk (PBIR or PBIR-Legacy).
    ///
    /// Offline structural + $schema checks against Microsoft's documented PBIP
    /// report format. Accepts a `.Report` folder, a definition.pbir file, or a
    /// PBIP root (with *.Report subfolders). Use before `report create
    /// --definition` or `deploy` to catch missing files / bad references early.
    #[command(display_order = 8)]
    Validate {
        /// Path to a report folder, a definition.pbir file, or a PBIP root
        #[arg(long)]
        source: String,
    },
    /// Get the synthesized report schema (pages, visuals, field→role bindings, textboxes) from the remote Power BI MCP server — read-only.
    #[command(name = "copilot-metadata", display_order = 8)]
    CopilotMetadata {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,
    },

    // ── PBIR page authoring (definition read-modify-write) ────────────────
    /// List the pages of a report (name, display name, visual count) — read-only.
    #[command(name = "list-pages", display_order = 9)]
    ListPages {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,
    },
    /// List the visuals of a report (page, name, type, title) — read-only.
    #[command(name = "list-visuals", display_order = 9)]
    ListVisuals {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Only list visuals of this page (by page name)
        #[arg(long)]
        page: Option<String>,
    },
    /// Add a page to a PBIR report by editing its definition. Overwrites the
    /// definition (irreversible) — dry-run guarded.
    #[command(name = "add-page", display_order = 9)]
    AddPage {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page display name (the tab label)
        #[arg(long)]
        display_name: String,

        /// Internal page name (default: a generated 20-hex id)
        #[arg(long)]
        name: Option<String>,

        /// Make this the active (default) page
        #[arg(long)]
        active: bool,
    },
    /// Delete a page from a PBIR report by editing its definition (a report must
    /// keep at least one page). Overwrites the definition (irreversible) —
    /// dry-run guarded.
    #[command(name = "delete-page", display_order = 9)]
    DeletePage {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page name to delete
        #[arg(long)]
        name: String,
    },
    /// Rename a page's display name in a PBIR report. Overwrites the definition
    /// (irreversible) — dry-run guarded.
    #[command(name = "rename-page", display_order = 9)]
    RenamePage {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page name to rename
        #[arg(long)]
        name: String,

        /// New display name
        #[arg(long)]
        display_name: String,
    },
    /// Set the active (default) page of a PBIR report. Overwrites the definition
    /// (irreversible) — dry-run guarded.
    #[command(name = "set-active-page", display_order = 9)]
    SetActivePage {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page name to make active
        #[arg(long)]
        name: String,
    },
    /// Show the report-level settings (`report.json` `ExplorationSettings`) of a
    /// PBIR report — read-only.
    #[command(name = "get-settings", display_order = 9)]
    GetSettings {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,
    },
    /// Set a report-level setting (`report.json` `ExplorationSettings`) of a PBIR
    /// report by editing its definition, e.g. `hideVisualContainerHeader`,
    /// `filterPaneHiddenInEditMode`, `allowInlineExploration`,
    /// `isPersistentUserStateDisabled`, `useCrossReportDrillthrough`. Overwrites
    /// the definition (irreversible) — dry-run guarded.
    #[command(name = "set-setting", display_order = 9)]
    SetSetting {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Setting name (a `report.json` `ExplorationSettings` key). Unknown names
        /// are rejected with the valid names enumerated.
        #[arg(long)]
        name: String,

        /// Setting value: true/false for boolean settings, or a string for
        /// string settings
        #[arg(long)]
        value: String,
    },
    /// Add a visual to a page of a PBIR report by editing its definition. Build
    /// a data-bound visual with --category/--measure (fields as Table.Column or
    /// Sum(Table.Column)) or a textbox with --text. Overwrites the definition
    /// (irreversible) — dry-run guarded.
    #[command(name = "add-visual", display_order = 9)]
    AddVisual {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page name to add the visual to
        #[arg(long)]
        page: String,

        /// Visual type: card, clusteredBarChart, clusteredColumnChart, lineChart,
        /// pieChart, tableEx, slicer, textbox, …
        #[arg(long = "type")]
        visual_type: String,

        /// Internal visual name (default: a generated 20-hex id)
        #[arg(long)]
        name: Option<String>,

        /// Visual title text
        #[arg(long)]
        title: Option<String>,

        /// Text content (for a textbox visual)
        #[arg(long)]
        text: Option<String>,

        /// Category / axis / legend field (Table.Column)
        #[arg(long)]
        category: Option<String>,

        /// Value field(s), repeatable: Table.Column (auto-Sum), Sum(Table.Column),
        /// Avg/Min/Max/Count/CountNonNull(…), or Measure(Table.Name)
        #[arg(long = "measure")]
        measures: Vec<String>,

        /// X position (default 40)
        #[arg(long)]
        x: Option<f64>,

        /// Y position (default 40)
        #[arg(long)]
        y: Option<f64>,

        /// Width (default 400)
        #[arg(long)]
        width: Option<f64>,

        /// Height (default 300)
        #[arg(long)]
        height: Option<f64>,
    },
    /// Delete a visual from a page of a PBIR report by editing its definition.
    /// Overwrites the definition (irreversible) — dry-run guarded.
    #[command(name = "delete-visual", display_order = 9)]
    DeleteVisual {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Page name containing the visual
        #[arg(long)]
        page: String,

        /// Visual name to delete
        #[arg(long)]
        name: String,
    },
    /// Scaffold a complete PBIR report from a compact JSON spec (pages + visuals)
    /// and create it — or write the PBIR folder to disk with --out.
    ///
    /// The spec is `{"pages":[{"displayName":..,"visuals":[{"type":..}]}]}` where
    /// each visual has a `type` and, for data-bound visuals, `category` /
    /// `measure(s)` fields (Table.Column or Sum(Table.Column)); a textbox uses
    /// `text`. See `fabio context schema Report`.
    #[command(display_order = 9)]
    Scaffold {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report display name (when creating)
        #[arg(long)]
        name: String,

        /// Report spec JSON (inline, or @file, or @- for stdin)
        #[arg(long)]
        spec: String,

        /// Dataset / semantic model ID to bind the report to
        #[arg(long)]
        dataset: String,

        /// Optional description (when creating)
        #[arg(long)]
        description: Option<String>,

        /// Write the PBIR folder to this directory instead of creating the report
        #[arg(long)]
        out: Option<String>,
    },

    // ── Sharing & Publishing ─────────────────────────────────────────────
    /// Publish a report to the web (generates a publicly accessible embed URL)
    ///
    /// Requires "Publish to web" tenant setting to be enabled by your Power BI admin.
    /// WARNING: The report will be accessible to anyone on the internet without authentication.
    #[command(display_order = 10)]
    PublishToWeb {
        /// Workspace ID (Power BI group ID)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,
    },

    /// Export (render) the Power BI report to a file (PDF, PPTX, PNG)
    #[command(display_order = 11)]
    Export {
        /// Workspace ID (Power BI group ID)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Report ID
        #[arg(long)]
        id: String,

        /// Output file format (PDF, PPTX, PNG)
        #[arg(long, default_value = "PDF")]
        format: String,

        /// Destination file path to write the exported report to
        #[arg(long)]
        out: String,

        /// Maximum seconds to wait for the export job to complete
        #[arg(long, default_value = "300")]
        timeout: u64,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &ReportCommand) -> Result<()> {
    match command {
        ReportCommand::List { workspace } => list(cli, client, workspace).await,
        ReportCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        ReportCommand::Create {
            workspace,
            name,
            description,
            file,
            definition,
            dataset,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                file.as_deref(),
                definition.as_deref(),
                dataset.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        ReportCommand::Update {
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
        ReportCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        ReportCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        ReportCommand::UpdateDefinition {
            workspace,
            id,
            file,
            report_json,
        } => update_definition(cli, client, workspace, id, file, report_json.as_deref()).await,
        ReportCommand::Validate { source } => validate(cli, source),
        ReportCommand::CopilotMetadata { workspace, id } => {
            copilot_metadata(cli, client, workspace, id).await
        }
        ReportCommand::ListPages { workspace, id } => {
            authoring::list_pages(cli, client, workspace, id).await
        }
        ReportCommand::ListVisuals {
            workspace,
            id,
            page,
        } => authoring::list_visuals(cli, client, workspace, id, page.as_deref()).await,
        ReportCommand::AddPage {
            workspace,
            id,
            display_name,
            name,
            active,
        } => {
            authoring::add_page(
                cli,
                client,
                workspace,
                id,
                display_name,
                name.as_deref(),
                *active,
            )
            .await
        }
        ReportCommand::DeletePage {
            workspace,
            id,
            name,
        } => authoring::delete_page(cli, client, workspace, id, name).await,
        ReportCommand::RenamePage {
            workspace,
            id,
            name,
            display_name,
        } => authoring::rename_page(cli, client, workspace, id, name, display_name).await,
        ReportCommand::SetActivePage {
            workspace,
            id,
            name,
        } => authoring::set_active_page(cli, client, workspace, id, name).await,
        ReportCommand::GetSettings { workspace, id } => {
            authoring::get_settings(cli, client, workspace, id).await
        }
        ReportCommand::SetSetting {
            workspace,
            id,
            name,
            value,
        } => authoring::set_setting(cli, client, workspace, id, name, value).await,
        ReportCommand::AddVisual {
            workspace,
            id,
            page,
            visual_type,
            name,
            title,
            text,
            category,
            measures,
            x,
            y,
            width,
            height,
        } => {
            authoring::add_visual(
                cli,
                client,
                workspace,
                id,
                page,
                &authoring::VisualSpec {
                    visual_type,
                    name: name.as_deref(),
                    title: title.as_deref(),
                    text: text.as_deref(),
                    category: category.as_deref(),
                    measures,
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                },
            )
            .await
        }
        ReportCommand::DeleteVisual {
            workspace,
            id,
            page,
            name,
        } => authoring::delete_visual(cli, client, workspace, id, page, name).await,
        ReportCommand::Scaffold {
            workspace,
            name,
            spec,
            dataset,
            description,
            out,
        } => {
            let spec_str = crate::commands::query_input::resolve_query_input(
                Some(spec),
                "JSON",
                "--spec",
                r#"{"pages":[{"displayName":"Overview","visuals":[{"type":"card","measure":"Sum(Sales.Revenue)"}]}]}"#,
            )?;
            let spec_json: Value = serde_json::from_str(&spec_str).map_err(|e| {
                crate::errors::FabioError::with_hint(
                    crate::errors::ErrorCode::InvalidInput,
                    format!("--spec is not valid JSON: {e}"),
                    r#"e.g. --spec '{"pages":[{"displayName":"Overview","visuals":[{"type":"card","measure":"Sum(Sales.Revenue)"}]}]}'"#
                        .to_string(),
                )
            })?;
            authoring::scaffold(
                cli,
                client,
                workspace,
                name,
                &spec_json,
                dataset,
                out.as_deref(),
                description.as_deref(),
            )
            .await
        }
        ReportCommand::PublishToWeb { workspace, id } => {
            publish_to_web(cli, client, workspace, id).await
        }
        ReportCommand::Export {
            workspace,
            id,
            format,
            out,
            timeout,
        } => {
            crate::commands::powerbi_export::export(
                cli,
                client,
                workspace,
                id,
                format,
                &[],
                out,
                *timeout,
                crate::commands::powerbi_export::ReportKind::PowerBi,
            )
            .await
        }
    }
}

// ─── Validation (offline PBIR/PBIP) ──────────────────────────────────────────

fn validate(cli: &Cli, source: &str) -> Result<()> {
    use anyhow::bail;
    let results = pbir::validate(std::path::Path::new(source))?;
    let all_valid = results.iter().all(|r| r.valid);
    let total_errors: usize = results.iter().map(|r| r.errors.len()).sum();
    let total_warnings: usize = results.iter().map(|r| r.warnings.len()).sum();

    let out = if results.len() == 1 {
        serde_json::json!({
            "status": if all_valid { "valid" } else { "invalid" },
            "report": results[0],
        })
    } else {
        serde_json::json!({
            "status": if all_valid { "valid" } else { "invalid" },
            "reports": results,
            "summary": { "count": results.len(), "errors": total_errors, "warnings": total_warnings },
        })
    };
    output::render_object(cli, &out, "status");

    if !all_valid {
        bail!("Report validation failed with {total_errors} error(s)");
    }
    Ok(())
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/reports"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/reports/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the `definition.pbir` binding a report to a semantic model by ID.
///
/// The MS schema (`report/definitionProperties`) marks `$schema` as REQUIRED.
/// The 6-field `byConnection` (binding by `pbiModelDatabaseName`) matches the
/// 1.x shape (2.x allows only `connectionString`), so we reference the 1.0.0
/// schema URL. Fabric normalizes the stored form to 2.0.0 on ingest.
fn build_dataset_pbir(dataset_id: &str) -> Value {
    serde_json::json!({
        "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json",
        "version": "4.0",
        "datasetReference": {
            "byConnection": {
                "connectionString": format!("semanticmodelid={dataset_id}")
            }
        }
    })
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    file: Option<&str>,
    definition: Option<&str>,
    dataset: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut parts: Vec<Value> = Vec::new();

    if let Some(folder) = definition {
        // Gather a full PBIR report folder (definition.pbir + report.json or
        // definition/**). Validate first so agents get a clear structural error
        // instead of an opaque API rejection.
        let dir = std::path::Path::new(folder);
        let report_dir = if dir.join("definition.pbir").exists() {
            dir.to_path_buf()
        } else {
            // Allow pointing at a definition.pbir file directly.
            if dir.is_file() && dir.file_name().and_then(|n| n.to_str()) == Some("definition.pbir")
            {
                dir.parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .to_path_buf()
            } else {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("No definition.pbir found in '{folder}'"),
                    "Point --definition at a report folder containing definition.pbir. Validate first: fabio report validate --source <folder>".to_string(),
                )
                .into());
            }
        };
        let validation = pbir::validate_report_folder(&report_dir);
        if !validation.valid {
            let first = validation
                .errors
                .first()
                .map(|e| format!("{}: {} ({})", e.file, e.message, e.code))
                .unwrap_or_default();
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid PBIR report definition: {first}"),
                "Run 'fabio report validate --source <folder>' to see all issues.".to_string(),
            )
            .into());
        }
        parts = pbir::gather_report_parts(&report_dir)?;
        // Optionally rebind the folder's definition.pbir to a concrete model.
        if let Some(dataset_id) = dataset {
            pbir::rebind_pbir_part(&mut parts, dataset_id)?;
        }
    } else if let Some(dataset_id) = dataset {
        // Auto-generate definition.pbir binding to the specified dataset.
        let pbir = build_dataset_pbir(dataset_id);
        let pbir_encoded = BASE64.encode(pbir.to_string().as_bytes());
        parts.push(serde_json::json!({
            "path": "definition.pbir",
            "payload": pbir_encoded,
            "payloadType": "InlineBase64"
        }));

        // Generate a minimal blank report.json (required by Fabric)
        let report_json = serde_json::json!({
            "config": "{\"version\":\"5.53\",\"themeCollection\":{\"baseTheme\":{\"name\":\"CY24SU06\",\"version\":\"5.53\",\"type\":2}},\"activeSectionIndex\":0}",
            "layoutOptimization": 0,
            "resourcePackages": [],
            "sections": [{
                "name": "ReportSection",
                "displayName": "Page 1",
                "filters": "[]",
                "ordinal": 0,
                "visualContainers": [],
                "config": "{\"name\":\"ReportSection\",\"layouts\":[{\"id\":0,\"position\":{\"x\":0,\"y\":0,\"z\":0,\"width\":1280,\"height\":720,\"tabOrder\":0}}]}",
                "displayOption": 1,
                "width": 1280,
                "height": 720
            }]
        });
        let report_encoded = BASE64.encode(report_json.to_string().as_bytes());
        parts.push(serde_json::json!({
            "path": "report.json",
            "payload": report_encoded,
            "payloadType": "InlineBase64"
        }));
    } else if let Some(file_path) = file {
        let content = std::fs::read_to_string(file_path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{file_path}': {e}"))?;
        let encoded = BASE64.encode(content.as_bytes());
        parts.push(serde_json::json!({
            "path": "definition.pbir",
            "payload": encoded,
            "payloadType": "InlineBase64"
        }));
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide --file, --definition, or --dataset".to_string(),
            "Use --dataset <semantic-model-id> to create a report bound to a model, or \
             --definition <folder> for a full PBIR report folder (validate it first: \
             fabio report validate --source <folder>). See: fabio context schema Report."
                .to_string(),
        )
        .into());
    }

    let mut body = serde_json::json!({
        "displayName": name,
        "definition": {
            "parts": parts
        }
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
        "report create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "dataset": dataset,
            "file": file,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/reports"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "report create", "Member"))?;
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
            "Example: fabio report update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "report update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/reports/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "report update", "Contributor"))?;
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
        "report delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/reports/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/reports/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "report delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/reports/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "report get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: &str,
    report_json: Option<&str>,
) -> Result<()> {
    let mut parts = Vec::new();
    let mut total_len: usize = 0;

    // definition.pbir is always required by the Fabric API
    let content = std::fs::read_to_string(file)
        .map_err(|e| anyhow::anyhow!("Failed to read file '{file}': {e}"))?;
    total_len += content.len();
    let encoded = BASE64.encode(content.as_bytes());
    parts.push(serde_json::json!({
        "path": "definition.pbir",
        "payload": encoded,
        "payloadType": "InlineBase64"
    }));

    if let Some(rj) = report_json {
        let rj_content = std::fs::read_to_string(rj)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{rj}': {e}"))?;
        total_len += rj_content.len();
        let rj_encoded = BASE64.encode(rj_content.as_bytes());
        parts.push(serde_json::json!({
            "path": "report.json",
            "payload": rj_encoded,
            "payloadType": "InlineBase64"
        }));
    }

    let body = serde_json::json!({
        "definition": {
            "parts": parts
        }
    });

    if output::dry_run_guard(
        cli,
        "report update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "parts": parts.len(),
            "contentLength": total_len
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/reports/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "report update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Publish to Web ──────────────────────────────────────────────────────────

/// Publish a report to the web, generating a publicly accessible embed URL.
///
/// Uses the Power BI REST API endpoint for "Publish to Web" which creates an
/// anonymous embed code accessible without authentication.
///
/// Requires the "Publish to web" tenant setting to be enabled by a Power BI admin.
async fn publish_to_web(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "report publish-to-web",
        &serde_json::json!({
            "workspace": workspace,
            "id": id
        }),
    ) {
        return Ok(());
    }

    // Power BI "Publish to Web" API
    // POST https://api.powerbi.com/v1.0/myorg/groups/{groupId}/reports/{reportId}/GenerateToken
    // with accessLevel: "View" creates a public embed token.
    //
    // The actual "Publish to Web" endpoint is:
    // POST /groups/{groupId}/reports/{reportId}/publishtoweb
    let body = serde_json::json!({
        "allowEditMode": false
    });

    let data = client
        .post_powerbi(
            &format!("/groups/{workspace}/reports/{id}/publishtoweb"),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "report publish-to-web",
                "Member (and 'Publish to web' tenant setting must be enabled)",
            )
        })?;

    // The response should contain embedUrl, embedCode, reportId, etc.
    // Construct a user-friendly response
    let embed_url = data
        .get("embedUrl")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let embed_code = data
        .get("embedCode")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let result = serde_json::json!({
        "id": id,
        "status": "published_to_web",
        "embedUrl": embed_url,
        "embedCode": embed_code,
        "warning": "This report is now publicly accessible to anyone on the internet without authentication."
    });
    output::render_object(cli, &result, "embedUrl");
    Ok(())
}

/// `report copilot-metadata` — fetch the synthesized report schema from the remote
/// Power BI MCP server's `GetReportMetadata` tool: workspace + semantic-model
/// details, pages, visuals (with field→role projections/bindings), and textbox
/// content. Distinct from `get-definition` (raw PBIR parts) — this is a grounded,
/// Copilot-oriented view of how the model is used in the report. Read-only.
async fn copilot_metadata(
    cli: &Cli,
    client: &FabricClient,
    _workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "report copilot-metadata",
        &serde_json::json!({ "id": id, "tool": "GetReportMetadata" }),
    ) {
        return Ok(());
    }

    let result = crate::commands::powerbi_mcp::call_powerbi_tool(
        client,
        "GetReportMetadata",
        serde_json::json!({ "reportObjectId": id }),
    )
    .await?;

    if result.is_error {
        anyhow::bail!(
            "Power BI MCP GetReportMetadata returned an error: {}",
            result.text()
        );
    }

    output::render_object(
        cli,
        &crate::commands::powerbi_mcp::tool_text_as_json(&result),
        "ReportMetadata",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dataset_pbir_conforms_to_ms_schema() {
        // MS report/definitionProperties requires $schema + version +
        // datasetReference. The byConnection must be the SIMPLE
        // `{connectionString: "semanticmodelid=<id>"}` shape — the old 6-field
        // XMLA shape (pbiServiceModelId/pbiModelVirtualServerName) is rejected by
        // the 2.0.0 schema (`does not allow additional properties`).
        let pbir = build_dataset_pbir("model-uuid-123");
        let schema = pbir["$schema"].as_str().unwrap();
        assert!(
            schema.contains("report/definitionProperties/2.") && schema.ends_with("/schema.json"),
            "unexpected $schema: {schema}"
        );
        assert_eq!(pbir["version"], "4.0");
        let by_conn = &pbir["datasetReference"]["byConnection"];
        assert_eq!(
            by_conn["connectionString"],
            "semanticmodelid=model-uuid-123"
        );
        assert!(by_conn.get("pbiServiceModelId").is_none());
        assert!(by_conn.get("pbiModelVirtualServerName").is_none());
    }
}
