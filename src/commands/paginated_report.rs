use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum PaginatedReportCommand {
    /// List paginated reports in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a paginated report
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,
    },
    /// Create a paginated report in the specified workspace (requires an RDL definition file)
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Display name
        #[arg(long)]
        name: String,

        /// Optional description (max 256 characters)
        #[arg(long)]
        description: Option<String>,

        /// Path to the .rdl definition file (base64-encoded and sent as the definition)
        #[arg(long)]
        file: Option<String>,

        /// Inline base64-encoded RDL content
        #[arg(long)]
        content: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update paginated report properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a paginated report
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the public definition of a paginated report (returns the .rdl file encoded in base64)
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a paginated report
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,

        /// Path to the .rdl definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline base64-encoded RDL content (JSON definition parts array)
        #[arg(long)]
        content: Option<String>,

        /// Update item metadata from .platform file when present in the definition
        #[arg(long)]
        update_metadata: bool,
    },
    /// Export (render) the paginated report to a file (PDF, PPTX, XLSX, DOCX, CSV, IMAGE, ...)
    #[command(display_order = 8)]
    Export {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Paginated report ID
        #[arg(long)]
        id: String,

        /// Output file format (PDF, PPTX, XLSX, DOCX, CSV, XML, MHTML, IMAGE, ACCESSIBLEPDF)
        #[arg(long, default_value = "PDF")]
        format: String,

        /// Destination file path to write the exported report to
        #[arg(long)]
        out: String,

        /// Report parameter as name=value (repeatable)
        #[arg(long = "parameter")]
        parameters: Vec<String>,

        /// Maximum seconds to wait for the export job to complete
        #[arg(long, default_value = "300")]
        timeout: u64,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &PaginatedReportCommand,
) -> Result<()> {
    match command {
        PaginatedReportCommand::List { workspace } => list(cli, client, workspace).await,
        PaginatedReportCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        PaginatedReportCommand::Create {
            workspace,
            name,
            description,
            file,
            content,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                file.as_deref(),
                content.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        PaginatedReportCommand::Update {
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
        PaginatedReportCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        PaginatedReportCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        PaginatedReportCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            update_metadata,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                *update_metadata,
            )
            .await
        }
        PaginatedReportCommand::Export {
            workspace,
            id,
            format,
            out,
            parameters,
            timeout,
        } => {
            crate::commands::powerbi_export::export(
                cli,
                client,
                workspace,
                id,
                format,
                parameters,
                out,
                *timeout,
                crate::commands::powerbi_export::ReportKind::Paginated,
            )
            .await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "paginatedReports",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/paginatedReports/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "paginated-report show", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the single-part RDL definition `parts` array.
///
/// The Fabric paginated-report API requires the RDL part path to equal
/// `<displayName>.rdl`; any other path fails with `MissingDefinitionParts`.
fn single_rdl_part(display_name: &str, payload_b64: &str) -> Value {
    serde_json::json!([{
        "path": format!("{display_name}.rdl"),
        "payload": payload_b64,
        "payloadType": "InlineBase64"
    }])
}

/// Wrap definition `parts` into the item-definition object.
///
/// The paginated-report definition must NOT include a `format` field: sending
/// `format: "PaginatedReportDefinition"` is rejected by the Fabric API with
/// `InvalidDefinitionFormat`. The correct shape is `{ "parts": [...] }`,
/// mirroring the working `report create` body.
fn definition_object(parts: &Value) -> Value {
    serde_json::json!({ "parts": parts.clone() })
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    file: Option<&str>,
    content: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    // Build the definition parts from file or content. The single RDL part must
    // be named `<displayName>.rdl` (see single_rdl_part).
    let parts = match (file, content) {
        (Some(path), _) => {
            let rdl_bytes = std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            single_rdl_part(name, &BASE64.encode(&rdl_bytes))
        }
        (_, Some(c)) => {
            // A ready-made JSON parts array is honored as-is; a raw base64 string
            // becomes a single `<displayName>.rdl` part.
            serde_json::from_str::<Value>(c).unwrap_or_else(|_| single_rdl_part(name, c))
        }
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided for paginated report creation".to_string(),
                "Example: fabio paginated-report create --workspace <WS> --name \"MyReport\" --file report.rdl".to_string(),
            ).into());
        }
    };

    let mut body = serde_json::json!({
        "displayName": name,
        "definition": definition_object(&parts),
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
        "paginated-report create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/paginatedReports"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "paginated-report create", "Contributor"))?;
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
            "Example: fabio paginated-report update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "paginated-report update", &body) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/paginatedReports/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "paginated-report update", "Contributor"))?;
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
    crate::commands::crud::delete(
        cli,
        client,
        "paginated-report",
        "paginatedReports",
        "Contributor",
        workspace,
        id,
        hard_delete,
    )
    .await
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "paginated-report",
        "paginatedReports",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    // Read the RDL payload (no network). `explicit_parts` is Some when the
    // caller passed a ready-made JSON parts array via --content (paths honored
    // as-is); otherwise we synthesize a single part whose path MUST equal
    // `<displayName>.rdl` (resolved below) or the API returns
    // `MissingDefinitionParts`.
    let (encoded_payload, explicit_parts): (Option<String>, Option<Value>) = match (file, content) {
        (Some(path), _) => {
            let rdl_bytes = std::fs::read(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
            (Some(BASE64.encode(&rdl_bytes)), None)
        }
        (_, Some(c)) => serde_json::from_str::<Value>(c)
            .map_or_else(|_| (Some(c.to_string()), None), |v| (None, Some(v))),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio paginated-report update-definition --workspace <WS> --id <ID> --file report.rdl".to_string(),
            ).into());
        }
    };

    if output::dry_run_guard(
        cli,
        "paginated-report update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "updateMetadata": update_metadata
        }),
    ) {
        return Ok(());
    }

    // Resolve the definition parts. When synthesizing, the single .rdl part
    // must be named after the item's current display name.
    let parts = if let Some(parts) = explicit_parts {
        parts
    } else {
        let item = client
            .get(&format!("/workspaces/{workspace}/paginatedReports/{id}"))
            .await
            .map_err(|e| enrich_forbidden(e, "paginated-report update-definition", "Viewer"))?;
        let display_name = item
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("report");
        single_rdl_part(display_name, &encoded_payload.unwrap_or_default())
    };

    // The definition must NOT carry a `format` field (see create()): sending
    // `format: "PaginatedReportDefinition"` is rejected with
    // `InvalidDefinitionFormat`.
    let body = serde_json::json!({
        "definition": definition_object(&parts),
    });

    let url = if update_metadata {
        format!(
            "/workspaces/{workspace}/paginatedReports/{id}/updateDefinition?updateMetadata=true"
        )
    } else {
        format!("/workspaces/{workspace}/paginatedReports/{id}/updateDefinition")
    };

    let data = client
        .post(&url, &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "paginated-report update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_list_command_derives_debug() {
        // Verifies that PaginatedReportCommand derives Debug correctly.
        let cmd = PaginatedReportCommand::List {
            workspace: "test".to_string(),
        };
        assert!(format!("{cmd:?}").contains("List"));
    }

    #[test]
    fn single_rdl_part_names_path_after_display_name() {
        // The Fabric API requires the RDL part path to equal `<displayName>.rdl`;
        // a mismatch fails with `MissingDefinitionParts`.
        let parts = single_rdl_part("SalesReport", "QUJD");
        let part = &parts[0];
        assert_eq!(part["path"], "SalesReport.rdl");
        assert_eq!(part["payload"], "QUJD");
        assert_eq!(part["payloadType"], "InlineBase64");
    }

    #[test]
    fn single_rdl_part_preserves_spaces_in_display_name() {
        let parts = single_rdl_part("Q1 Sales", "eA==");
        assert_eq!(parts[0]["path"], "Q1 Sales.rdl");
    }

    #[test]
    fn definition_object_omits_format_field() {
        // Regression guard: sending `format: "PaginatedReportDefinition"` is
        // rejected by the Fabric API with `InvalidDefinitionFormat`. The
        // definition must be `{ "parts": [...] }` only.
        let def = definition_object(&single_rdl_part("R", "eA=="));
        assert!(
            def.get("format").is_none(),
            "definition must NOT include a `format` field"
        );
        assert!(def["parts"].is_array());
        assert_eq!(def["parts"][0]["path"], "R.rdl");
    }
}
