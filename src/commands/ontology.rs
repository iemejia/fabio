use std::path::Path;

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden, enrich_ontology_definition_error};
use crate::output;

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
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &OntologyCommand) -> Result<()> {
    match command {
        OntologyCommand::List { workspace } => list(cli, client, workspace).await,
        OntologyCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        OntologyCommand::Create {
            workspace,
            name,
            description,
            definition,
            file,
            dir,
            sensitivity_label,
        } => create(
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
        } => update(
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
        } => delete(cli, client, workspace, id, *hard)
            .await
            .map_err(|e| enrich_forbidden(e, "ontology delete", "Member")),
        OntologyCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        OntologyCommand::UpdateDefinition {
            workspace,
            id,
            definition,
            file,
            dir,
            update_metadata,
        } => update_definition(
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
            crate::commands::ontology_import::import_owl(
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
        } => crate::commands::ontology_import::bind_ontology(
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
        } => {
            crate::commands::ontology_import::export_owl(
                cli,
                client,
                workspace,
                id,
                format,
                file.as_deref(),
            )
            .await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/ontologies"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/ontologies/{id}"))
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    definition_path: Option<&str>,
    file_path: Option<&str>,
    dir_path: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });

    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }

    if let Some(path) = definition_path {
        let content = read_file_or_stdin(path)?;
        let def: Value = serde_json::from_str(&content)
            .map_err(|e| FabioError::with_hint(ErrorCode::InvalidInput, format!("Invalid definition JSON: {e}"), "Provide valid JSON. Inspect format: fabio ontology get-definition --workspace <WS> --id <ID> --decode"))?;
        body["definition"] = def;
    } else if let Some(path) = file_path {
        body["definition"] = build_definition_from_rdf(path)?;
    } else if let Some(path) = dir_path {
        body["definition"] = build_definition_from_dir(path)?;
    }

    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/ontologies"), &body, true)
        .await?;

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
            "Specify at least one of --name or --description to update",
            "Example: fabio ontology update --workspace <WS> --id <ID> --name \"New Name\"",
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

    let data = client
        .patch(&format!("/workspaces/{workspace}/ontologies/{id}"), &body)
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard: bool,
) -> Result<()> {
    let path = if hard {
        format!("/workspaces/{workspace}/ontologies/{id}?hardDelete=True")
    } else {
        format!("/workspaces/{workspace}/ontologies/{id}")
    };

    client.delete(&path).await?;

    output::render_object(
        cli,
        &serde_json::json!({"id": id, "status": "deleted"}),
        "status",
    );
    Ok(())
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let path = format.map_or_else(
        || format!("/workspaces/{workspace}/ontologies/{id}/getDefinition"),
        |f| format!("/workspaces/{workspace}/ontologies/{id}/getDefinition?format={f}"),
    );

    let data = client.post(&path, &serde_json::json!({}), true).await?;

    if decode {
        let decoded = decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

/// Decode base64 payloads in definition parts to readable JSON/text.
fn decode_definition_parts(mut data: Value) -> Value {
    if let Some(parts) = data
        .get_mut("definition")
        .and_then(|d| d.get_mut("parts"))
        .and_then(|p| p.as_array_mut())
    {
        for part in parts {
            if let Some(payload) = part.get("payload").and_then(|p| p.as_str())
                && let Ok(decoded_bytes) = BASE64.decode(payload)
                && let Ok(decoded_str) = String::from_utf8(decoded_bytes)
            {
                // Try parsing as JSON for pretty output
                if let Ok(json_val) = serde_json::from_str::<Value>(&decoded_str) {
                    part["decodedPayload"] = json_val;
                } else {
                    part["decodedPayload"] = Value::from(decoded_str);
                }
            }
        }
    }

    data
}

#[allow(clippy::too_many_arguments)]
async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    definition_path: Option<&str>,
    file_path: Option<&str>,
    dir_path: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let def = if let Some(path) = definition_path {
        let content = read_file_or_stdin(path)?;
        serde_json::from_str::<Value>(&content)
            .map_err(|e| FabioError::with_hint(ErrorCode::InvalidInput, format!("Invalid definition JSON: {e}"), "Provide valid JSON. Inspect format: fabio ontology get-definition --workspace <WS> --id <ID> --decode"))?
    } else if let Some(path) = file_path {
        build_definition_from_rdf(path)?
    } else if let Some(path) = dir_path {
        build_definition_from_dir(path)?
    } else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Specify either --definition, --file, or --dir",
            "Use --definition for inline JSON, --file for RDF, or --dir for Fabric directory format.",
        )
        .into());
    };

    let body = serde_json::json!({"definition": def});

    ensure_platform_when_updating_metadata(&def, update_metadata)?;

    let path = if update_metadata {
        format!("/workspaces/{workspace}/ontologies/{id}/updateDefinition?updateMetadata=True")
    } else {
        format!("/workspaces/{workspace}/ontologies/{id}/updateDefinition")
    };

    let data = client.post(&path, &body, true).await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

/// Fail fast when `--update-metadata` is requested but the definition has no
/// `.platform` part. Fabric requires a `.platform` part (it carries the item's
/// display name, description, and type) to honor `updateMetadata=true`, and
/// otherwise rejects the whole upload with `InvalidInput: UpdateMetadata is true
/// but .platform file was not provided`. That error is only visible after the
/// (potentially large) definition is uploaded and the LRO runs, so we check
/// locally first and return an actionable hint instead — notably, an
/// `ontology import`-generated directory never contains a `.platform`.
fn ensure_platform_when_updating_metadata(def: &Value, update_metadata: bool) -> Result<()> {
    if !update_metadata {
        return Ok(());
    }
    let has_platform = def
        .get("parts")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts
                .iter()
                .any(|p| p.get("path").and_then(Value::as_str) == Some(".platform"))
        });
    if has_platform {
        return Ok(());
    }
    Err(FabioError::with_hint(
        ErrorCode::InvalidInput,
        "--update-metadata requires a .platform part in the definition, but none was found",
        "Fabric rejects updateMetadata=true unless the definition includes a .platform part \
         (it carries the item's display name, description, and type). Add a .platform file to the \
         definition directory, or drop --update-metadata to replace only the definition parts. \
         Note: `fabio ontology import` does not generate a .platform, so an import-generated \
         directory must be updated without --update-metadata.",
    )
    .into())
}

/// Build a Fabric definition payload from a raw RDF file.
/// Auto-detects format from file extension and wraps content as base64-encoded part.
/// Includes the mandatory `definition.json` part that Fabric requires.
fn build_definition_from_rdf(path: &str) -> Result<Value> {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let part_path = match ext.as_str() {
        "ttl" => "ontology.ttl",
        "owl" => "ontology.owl",
        "rdf" | "xml" => "ontology.rdf",
        "jsonld" => "ontology.jsonld",
        "nt" => "ontology.nt",
        "n3" => "ontology.n3",
        "trig" => "ontology.trig",
        _ => return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Unsupported RDF format '.{ext}'"),
            "Supported formats: .ttl, .owl, .rdf, .xml, .jsonld, .nt, .n3, .trig. Or use --dir for Fabric ontology directory format.",
        )
        .into()),
    };

    let content = std::fs::read(path)
        .map_err(|e| anyhow::anyhow!("Failed to read RDF file '{path}': {e}"))?;

    let encoded = BASE64.encode(&content);

    // Fabric requires a definition.json part to exist; include it as empty JSON
    let def_json_payload = BASE64.encode(b"{}");

    Ok(serde_json::json!({
        "parts": [
            {
                "path": "definition.json",
                "payload": def_json_payload,
                "payloadType": "InlineBase64"
            },
            {
                "path": part_path,
                "payload": encoded,
                "payloadType": "InlineBase64"
            }
        ]
    }))
}

/// Build a Fabric definition payload from a directory structure.
/// Expects the Fabric ontology definition layout:
///   definition.json (optional, defaults to `{}`)
///   .platform (optional)
///   EntityTypes/{ID}/definition.json
///   EntityTypes/{ID}/DataBindings/{UUID}.json
///   EntityTypes/{ID}/Documents/{name}.json
///   EntityTypes/{ID}/Overviews/definition.json
///   EntityTypes/{ID}/ResourceLinks/definition.json
///   RelationshipTypes/{ID}/definition.json
///   RelationshipTypes/{ID}/Contextualizations/{UUID}.json
fn build_definition_from_dir(dir_path: &str) -> Result<Value> {
    let dir = Path::new(dir_path);
    if !dir.is_dir() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("'{dir_path}' is not a directory"),
            "Expected Fabric ontology directory with: EntityTypes/<ID>/definition.json, RelationshipTypes/<ID>/definition.json. Export one: fabio ontology get-definition --workspace <WS> --id <ID> --dir ./ontology",
        )
        .into());
    }

    let mut parts: Vec<Value> = Vec::new();

    // Always include definition.json (empty if not present)
    let def_json_path = dir.join("definition.json");
    let def_json_content = if def_json_path.exists() {
        std::fs::read(&def_json_path)
            .map_err(|e| anyhow::anyhow!("Failed to read definition.json: {e}"))?
    } else {
        b"{}".to_vec()
    };
    parts.push(serde_json::json!({
        "path": "definition.json",
        "payload": BASE64.encode(&def_json_content),
        "payloadType": "InlineBase64"
    }));

    // Include .platform if present
    let platform_path = dir.join(".platform");
    if platform_path.exists() {
        let content = std::fs::read(&platform_path)
            .map_err(|e| anyhow::anyhow!("Failed to read .platform: {e}"))?;
        parts.push(serde_json::json!({
            "path": ".platform",
            "payload": BASE64.encode(&content),
            "payloadType": "InlineBase64"
        }));
    }

    // Scan EntityTypes/
    let entity_types_dir = dir.join("EntityTypes");
    if entity_types_dir.is_dir() {
        scan_entity_types(&entity_types_dir, &mut parts)?;
    }

    // Scan RelationshipTypes/
    let rel_types_dir = dir.join("RelationshipTypes");
    if rel_types_dir.is_dir() {
        scan_relationship_types(&rel_types_dir, &mut parts)?;
    }

    Ok(serde_json::json!({ "parts": parts }))
}

/// Scan `EntityTypes` directory and add parts for each entity type and its sub-items.
fn scan_entity_types(entity_types_dir: &Path, parts: &mut Vec<Value>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(entity_types_dir)
        .map_err(|e| anyhow::anyhow!("Failed to read EntityTypes directory: {e}"))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let type_id = entry.file_name().to_string_lossy().to_string();
        let type_dir = entry.path();

        // EntityTypes/{ID}/definition.json
        let def_path = type_dir.join("definition.json");
        if def_path.exists() {
            let content = std::fs::read(&def_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", def_path.display()))?;
            parts.push(serde_json::json!({
                "path": format!("EntityTypes/{type_id}/definition.json"),
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }

        // EntityTypes/{ID}/DataBindings/*.json (needs key-order normalization)
        let bindings_dir = type_dir.join("DataBindings");
        if bindings_dir.is_dir() {
            scan_data_binding_files(
                &bindings_dir,
                &format!("EntityTypes/{type_id}/DataBindings"),
                parts,
            )?;
        }

        // EntityTypes/{ID}/Documents/*.json
        let docs_dir = type_dir.join("Documents");
        if docs_dir.is_dir() {
            scan_json_files(
                &docs_dir,
                &format!("EntityTypes/{type_id}/Documents"),
                parts,
            )?;
        }

        // EntityTypes/{ID}/Overviews/definition.json
        let overviews_path = type_dir.join("Overviews").join("definition.json");
        if overviews_path.exists() {
            let content = std::fs::read(&overviews_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", overviews_path.display()))?;
            parts.push(serde_json::json!({
                "path": format!("EntityTypes/{type_id}/Overviews/definition.json"),
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }

        // EntityTypes/{ID}/ResourceLinks/definition.json
        let links_path = type_dir.join("ResourceLinks").join("definition.json");
        if links_path.exists() {
            let content = std::fs::read(&links_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", links_path.display()))?;
            parts.push(serde_json::json!({
                "path": format!("EntityTypes/{type_id}/ResourceLinks/definition.json"),
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }
    }

    Ok(())
}

/// Scan `RelationshipTypes` directory and add parts.
fn scan_relationship_types(rel_types_dir: &Path, parts: &mut Vec<Value>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(rel_types_dir)
        .map_err(|e| anyhow::anyhow!("Failed to read RelationshipTypes directory: {e}"))?
        .filter_map(std::result::Result::ok)
        .filter(|e| e.path().is_dir())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let type_id = entry.file_name().to_string_lossy().to_string();
        let type_dir = entry.path();

        // RelationshipTypes/{ID}/definition.json
        let def_path = type_dir.join("definition.json");
        if def_path.exists() {
            let content = std::fs::read(&def_path)
                .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", def_path.display()))?;
            parts.push(serde_json::json!({
                "path": format!("RelationshipTypes/{type_id}/definition.json"),
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }

        // RelationshipTypes/{ID}/Contextualizations/*.json
        let ctx_dir = type_dir.join("Contextualizations");
        if ctx_dir.is_dir() {
            scan_json_files(
                &ctx_dir,
                &format!("RelationshipTypes/{type_id}/Contextualizations"),
                parts,
            )?;
        }
    }

    Ok(())
}

/// Scan a directory for .json files and add them as definition parts.
fn scan_json_files(dir: &Path, prefix: &str, parts: &mut Vec<Value>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {e}", dir.display()))?
        .filter_map(std::result::Result::ok)
        .filter(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let content = std::fs::read(entry.path())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", entry.path().display()))?;
        parts.push(serde_json::json!({
            "path": format!("{prefix}/{file_name}"),
            "payload": BASE64.encode(&content),
            "payloadType": "InlineBase64"
        }));
    }

    Ok(())
}

/// Scan `DataBinding` JSON files and normalize key ordering.
///
/// The Fabric Ontology API requires `sourceType` to be the first key in
/// `dataBindingConfiguration.sourceTableProperties` (it uses this as a JSON
/// discriminator for the source type union). Without this ordering, the server
/// throws an import exception.
fn scan_data_binding_files(dir: &Path, prefix: &str, parts: &mut Vec<Value>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {e}", dir.display()))?
        .filter_map(Result::ok)
        .filter(|e| {
            e.path().is_file()
                && e.path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let file_name = entry.file_name().to_string_lossy().to_string();
        let content = std::fs::read(entry.path())
            .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", entry.path().display()))?;

        let normalized = normalize_data_binding(&content)?;

        parts.push(serde_json::json!({
            "path": format!("{prefix}/{file_name}"),
            "payload": BASE64.encode(&normalized),
            "payloadType": "InlineBase64"
        }));
    }

    Ok(())
}

/// Helper struct for ordered serialization of `sourceTableProperties`.
/// Guarantees `sourceType` is serialized first (struct field order), which is
/// required by the Fabric API's ordered JSON deserializer.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct OrderedSourceTableProperties {
    source_type: Value,
    #[serde(flatten)]
    other: std::collections::BTreeMap<String, Value>,
}

/// Normalize a data binding JSON to ensure `sourceType` is the first key in
/// `sourceTableProperties`. The Fabric API uses ordered JSON deserialization
/// with `sourceType` as a discriminator field for the union type.
///
/// IMPORTANT: this round-trips through `serde_json::Value`, so it depends on the
/// crate's `preserve_order` feature (enabled in `[dependencies]`). Without it,
/// `Value` is backed by a `BTreeMap` that alphabetizes keys on re-serialization,
/// pushing `sourceType` out of first position and making Fabric reject the push
/// with a generic `ALMOperationImportFailed`. See the `serde_json` note in
/// `Cargo.toml`.
fn normalize_data_binding(content: &[u8]) -> Result<Vec<u8>> {
    let mut binding: Value = serde_json::from_slice(content)
        .map_err(|e| FabioError::with_hint(ErrorCode::InvalidInput, format!("Invalid JSON in DataBinding file: {e}"), "DataBinding files must be valid JSON. See format: fabio ontology get-definition --workspace <WS> --id <ID> --decode"))?;

    // Validate that the 'id' field is a valid UUID — non-UUID IDs are silently dropped by the server
    if let Some(id_val) = binding.get("id").and_then(Value::as_str)
        && !is_valid_uuid(id_val)
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Data binding 'id' must be UUID format, got: '{id_val}'"),
            "Use UUID format (e.g., c0000001-0001-0001-0001-000000000001). \
                 Non-UUID IDs are silently dropped by the Fabric API with no error.",
        )
        .into());
    }

    // Navigate to dataBindingConfiguration.sourceTableProperties and reorder
    if let Some(config) = binding
        .get_mut("dataBindingConfiguration")
        .and_then(Value::as_object_mut)
        && let Some(source_props) = config
            .get_mut("sourceTableProperties")
            .and_then(Value::as_object_mut)
    {
        // Extract sourceType and rebuild using struct serialization for guaranteed order
        if let Some(source_type) = source_props.remove("sourceType") {
            let remaining: std::collections::BTreeMap<String, Value> = source_props
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            let ordered = OrderedSourceTableProperties {
                source_type,
                other: remaining,
            };
            // Serialize the ordered struct back to a Value and replace
            let ordered_value = serde_json::to_value(&ordered)
                .map_err(|e| anyhow::anyhow!("Failed to reorder sourceTableProperties: {e}"))?;
            if let Value::Object(new_map) = ordered_value {
                *source_props = new_map;
            }
        }
    }

    serde_json::to_vec(&binding)
        .map_err(|e| anyhow::anyhow!("Failed to serialize normalized DataBinding: {e}"))
}

/// Check if a string is a valid UUID (8-4-4-4-12 hex format).
fn is_valid_uuid(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 5 {
        return false;
    }
    let expected_lens = [8, 4, 4, 4, 12];
    parts
        .iter()
        .zip(expected_lens.iter())
        .all(|(part, &len)| part.len() == len && part.chars().all(|c| c.is_ascii_hexdigit()))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_definition_from_rdf_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("schema.ttl");
        std::fs::write(
            &file,
            r#"@prefix rdf: <http://www.w3.org/1999/02/22-rdf-syntax-ns#> .
@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix sales: <http://example.org/sales#> .

sales:SalesOntology a owl:Ontology ;
    rdfs:label "Sales Domain Ontology" .

sales:Customer a owl:Class ;
    rdfs:label "Customer" .

sales:Order a owl:Class ;
    rdfs:label "Order" .

sales:placedBy a owl:ObjectProperty ;
    rdfs:domain sales:Order ;
    rdfs:range sales:Customer .
"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);

        // First part must be definition.json (Fabric requirement)
        assert_eq!(parts[0]["path"], "definition.json");
        assert_eq!(parts[0]["payloadType"], "InlineBase64");

        // Second part is the RDF file
        assert_eq!(parts[1]["path"], "ontology.ttl");
        assert_eq!(parts[1]["payloadType"], "InlineBase64");

        // Verify base64 decodes back to original content
        let payload = parts[1]["payload"].as_str().unwrap();
        let decoded = BASE64.decode(payload).unwrap();
        let content = String::from_utf8(decoded).unwrap();
        assert!(content.contains("sales:Customer a owl:Class"));
        assert!(content.contains("sales:placedBy a owl:ObjectProperty"));
    }

    #[test]
    fn build_definition_from_rdf_owl() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("ontology.owl");
        std::fs::write(
            &file,
            r#"<?xml version="1.0" encoding="UTF-8"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:owl="http://www.w3.org/2002/07/owl#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#">
  <owl:Ontology rdf:about="http://example.org/inventory">
    <rdfs:label>Inventory Ontology</rdfs:label>
  </owl:Ontology>
  <owl:Class rdf:about="http://example.org/inventory#Warehouse">
    <rdfs:label>Warehouse</rdfs:label>
  </owl:Class>
</rdf:RDF>"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.owl");
    }

    #[test]
    fn build_definition_from_rdf_jsonld() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("graph.jsonld");
        std::fs::write(
            &file,
            r#"{
  "@context": {
    "owl": "http://www.w3.org/2002/07/owl#",
    "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
    "hr": "http://example.org/hr#"
  },
  "@graph": [
    {"@id": "hr:HROntology", "@type": "owl:Ontology", "rdfs:label": "HR Ontology"},
    {"@id": "hr:Employee", "@type": "owl:Class", "rdfs:label": "Employee"},
    {"@id": "hr:Department", "@type": "owl:Class", "rdfs:label": "Department"}
  ]
}"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.jsonld");
    }

    #[test]
    fn build_definition_from_rdf_rdf_xml() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.rdf");
        std::fs::write(
            &file,
            r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:rdfs="http://www.w3.org/2000/01/rdf-schema#">
  <rdf:Description rdf:about="http://example.org/Resource">
    <rdfs:label>Example Resource</rdfs:label>
  </rdf:Description>
</rdf:RDF>"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.rdf");
    }

    #[test]
    fn build_definition_from_rdf_xml_ext() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.xml");
        std::fs::write(
            &file,
            r#"<?xml version="1.0"?>
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
         xmlns:owl="http://www.w3.org/2002/07/owl#">
  <owl:Ontology rdf:about="http://example.org/test"/>
</rdf:RDF>"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.rdf");
    }

    #[test]
    fn build_definition_from_rdf_ntriples() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("triples.nt");
        std::fs::write(
            &file,
            r#"<http://example.org/Employee> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#Class> .
<http://example.org/Employee> <http://www.w3.org/2000/01/rdf-schema#label> "Employee" .
<http://example.org/name> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <http://www.w3.org/2002/07/owl#DatatypeProperty> .
"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.nt");
    }

    #[test]
    fn build_definition_from_rdf_n3() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notation.n3");
        std::fs::write(
            &file,
            r#"@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix : <http://example.org/geo#> .

:GeoOntology a owl:Ontology ;
    rdfs:label "Geography Ontology" .

:Country a owl:Class ;
    rdfs:label "Country" .

:City a owl:Class ;
    rdfs:label "City" .

:locatedIn a owl:ObjectProperty ;
    rdfs:domain :City ;
    rdfs:range :Country .
"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.n3");
    }

    #[test]
    fn build_definition_from_rdf_trig() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("named.trig");
        std::fs::write(
            &file,
            r#"@prefix rdfs: <http://www.w3.org/2000/01/rdf-schema#> .
@prefix owl: <http://www.w3.org/2002/07/owl#> .
@prefix : <http://example.org/events#> .

GRAPH :EventGraph {
    :Event a owl:Class ;
        rdfs:label "Event" .
    :Venue a owl:Class ;
        rdfs:label "Venue" .
    :hostedAt a owl:ObjectProperty ;
        rdfs:domain :Event ;
        rdfs:range :Venue .
}
"#,
        )
        .unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        assert_eq!(def["parts"][1]["path"], "ontology.trig");
    }

    #[test]
    fn build_definition_from_rdf_unsupported_extension() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("data.csv");
        std::fs::write(&file, "a,b,c").unwrap();

        let err = build_definition_from_rdf(file.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("Unsupported RDF format"));
        assert!(err.to_string().contains(".csv"));
    }

    #[test]
    fn build_definition_from_rdf_missing_file() {
        let err = build_definition_from_rdf("/nonexistent/path.ttl").unwrap_err();
        assert!(err.to_string().contains("Failed to read RDF file"));
    }

    #[test]
    fn build_definition_from_rdf_binary_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("binary.ttl");
        std::fs::write(&file, [0u8, 1, 2, 255, 254, 253]).unwrap();

        let def = build_definition_from_rdf(file.to_str().unwrap()).unwrap();
        let payload = def["parts"][1]["payload"].as_str().unwrap();
        let decoded = BASE64.decode(payload).unwrap();
        assert_eq!(decoded, &[0u8, 1, 2, 255, 254, 253]);
    }

    // -----------------------------------------------------------------------
    // Tests for build_definition_from_dir
    // -----------------------------------------------------------------------

    #[test]
    fn build_definition_from_dir_minimal() {
        let dir = tempfile::tempdir().unwrap();
        // Just an empty directory — should produce definition.json with {}
        let def = build_definition_from_dir(dir.path().to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "definition.json");
        let payload = BASE64
            .decode(parts[0]["payload"].as_str().unwrap())
            .unwrap();
        assert_eq!(payload, b"{}");
    }

    #[test]
    fn build_definition_from_dir_with_entity_types() {
        let dir = tempfile::tempdir().unwrap();

        // Create entity type structure
        let entity_dir = dir.path().join("EntityTypes").join("1234567890");
        std::fs::create_dir_all(&entity_dir).unwrap();
        std::fs::write(
            entity_dir.join("definition.json"),
            r#"{"id":"1234567890","name":"Equipment","namespace":"usertypes","namespaceType":"Custom"}"#,
        )
        .unwrap();

        // Create data binding
        let bindings_dir = entity_dir.join("DataBindings");
        std::fs::create_dir_all(&bindings_dir).unwrap();
        std::fs::write(
            bindings_dir.join("a0000001-0001-0001-0001-000000000001.json"),
            r#"{"id":"a0000001-0001-0001-0001-000000000001","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries"}}"#,
        )
        .unwrap();

        let def = build_definition_from_dir(dir.path().to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();

        // Should have: definition.json + EntityTypes/{id}/definition.json + DataBindings/{id}.json
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["path"], "definition.json");
        assert_eq!(parts[1]["path"], "EntityTypes/1234567890/definition.json");
        assert_eq!(
            parts[2]["path"],
            "EntityTypes/1234567890/DataBindings/a0000001-0001-0001-0001-000000000001.json"
        );

        // Verify entity type content
        let payload = BASE64
            .decode(parts[1]["payload"].as_str().unwrap())
            .unwrap();
        let entity: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(entity["name"], "Equipment");
        assert_eq!(entity["id"], "1234567890");
    }

    #[test]
    fn build_definition_from_dir_with_relationship_types() {
        let dir = tempfile::tempdir().unwrap();

        // Create relationship type
        let rel_dir = dir.path().join("RelationshipTypes").join("9876543210");
        std::fs::create_dir_all(&rel_dir).unwrap();
        std::fs::write(
            rel_dir.join("definition.json"),
            r#"{"id":"9876543210","name":"contains","namespace":"usertypes","namespaceType":"Custom","source":{"entityTypeId":"111"},"target":{"entityTypeId":"222"}}"#,
        )
        .unwrap();

        // Create contextualization
        let ctx_dir = rel_dir.join("Contextualizations");
        std::fs::create_dir_all(&ctx_dir).unwrap();
        std::fs::write(
            ctx_dir.join("ctx-uuid-1.json"),
            r#"{"id":"ctx-uuid-1","dataBindingTable":{"sourceType":"LakehouseTable"}}"#,
        )
        .unwrap();

        let def = build_definition_from_dir(dir.path().to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();

        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0]["path"], "definition.json");
        assert_eq!(
            parts[1]["path"],
            "RelationshipTypes/9876543210/definition.json"
        );
        assert_eq!(
            parts[2]["path"],
            "RelationshipTypes/9876543210/Contextualizations/ctx-uuid-1.json"
        );
    }

    #[test]
    fn build_definition_from_dir_full_structure() {
        let dir = tempfile::tempdir().unwrap();

        // Custom definition.json
        std::fs::write(dir.path().join("definition.json"), r#"{"custom": true}"#).unwrap();

        // .platform file
        std::fs::write(
            dir.path().join(".platform"),
            r#"{"metadata":{"type":"Ontology","displayName":"Test"}}"#,
        )
        .unwrap();

        // Entity type with overviews and resource links
        let et_dir = dir.path().join("EntityTypes").join("100");
        std::fs::create_dir_all(et_dir.join("Overviews")).unwrap();
        std::fs::create_dir_all(et_dir.join("ResourceLinks")).unwrap();
        std::fs::create_dir_all(et_dir.join("Documents")).unwrap();
        std::fs::write(
            et_dir.join("definition.json"),
            r#"{"id":"100","name":"Thing"}"#,
        )
        .unwrap();
        std::fs::write(
            et_dir.join("Overviews").join("definition.json"),
            r#"{"widgets":[],"settings":{"type":"fixedTime"}}"#,
        )
        .unwrap();
        std::fs::write(
            et_dir.join("ResourceLinks").join("definition.json"),
            r#"{"resourceLinks":[{"type":"PowerBIReport","workspaceId":"ws1","itemId":"item1"}]}"#,
        )
        .unwrap();
        std::fs::write(
            et_dir.join("Documents").join("doc1.json"),
            r#"{"displayText":"Manual","url":"https://example.org"}"#,
        )
        .unwrap();

        let def = build_definition_from_dir(dir.path().to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();

        // definition.json + .platform + entity def + documents + overviews + resource links
        assert_eq!(parts.len(), 6);
        assert_eq!(parts[0]["path"], "definition.json");
        assert_eq!(parts[1]["path"], ".platform");
        assert_eq!(parts[2]["path"], "EntityTypes/100/definition.json");
        assert_eq!(parts[3]["path"], "EntityTypes/100/Documents/doc1.json");
        assert_eq!(
            parts[4]["path"],
            "EntityTypes/100/Overviews/definition.json"
        );
        assert_eq!(
            parts[5]["path"],
            "EntityTypes/100/ResourceLinks/definition.json"
        );

        // Verify custom definition.json was used (not default {})
        let payload = BASE64
            .decode(parts[0]["payload"].as_str().unwrap())
            .unwrap();
        let content: Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(content["custom"], true);
    }

    #[test]
    fn build_definition_from_dir_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not_a_dir.txt");
        std::fs::write(&file, "hello").unwrap();

        let err = build_definition_from_dir(file.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("is not a directory"));
    }

    #[test]
    fn build_definition_from_dir_nonexistent() {
        let err = build_definition_from_dir("/nonexistent/path").unwrap_err();
        assert!(err.to_string().contains("is not a directory"));
    }

    #[test]
    fn build_definition_from_dir_multiple_entity_types_sorted() {
        let dir = tempfile::tempdir().unwrap();

        // Create entity types in non-sorted order
        for id in &["300", "100", "200"] {
            let et_dir = dir.path().join("EntityTypes").join(id);
            std::fs::create_dir_all(&et_dir).unwrap();
            std::fs::write(
                et_dir.join("definition.json"),
                format!(r#"{{"id":"{id}","name":"Type{id}"}}"#),
            )
            .unwrap();
        }

        let def = build_definition_from_dir(dir.path().to_str().unwrap()).unwrap();
        let parts = def["parts"].as_array().unwrap();

        // Should be sorted: 100, 200, 300
        assert_eq!(parts[1]["path"], "EntityTypes/100/definition.json");
        assert_eq!(parts[2]["path"], "EntityTypes/200/definition.json");
        assert_eq!(parts[3]["path"], "EntityTypes/300/definition.json");
    }

    // -----------------------------------------------------------------------
    // Tests for normalize_data_binding
    // -----------------------------------------------------------------------

    #[test]
    fn normalize_data_binding_moves_source_type_first() {
        let input = br#"{"id":"b0000001-0001-0001-0001-000000000001","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries","sourceTableProperties":{"itemId":"abc","sourceTableName":"t","sourceType":"LakehouseTable","workspaceId":"ws"}}}"#;
        let output = normalize_data_binding(input).unwrap();
        let parsed: Value = serde_json::from_slice(&output).unwrap();
        let source_props = parsed["dataBindingConfiguration"]["sourceTableProperties"]
            .as_object()
            .unwrap();
        let keys: Vec<&String> = source_props.keys().collect();
        assert_eq!(keys[0], "sourceType", "sourceType must be the first key");
    }

    #[test]
    fn normalize_data_binding_wire_order_puts_source_type_first() {
        // Regression guard for the `serde_json` `preserve_order` requirement.
        // Fabric's ordered union deserializer reads the FIRST key of
        // `sourceTableProperties` as the `sourceType` discriminator; if any other
        // key precedes it on the wire, the whole push is rejected with a generic
        // `ALMOperationImportFailed`. This asserts the actual serialized BYTES
        // (not a re-parsed Value, which would hide the ordering), so it fails if
        // `preserve_order` is ever dropped and `Value` starts alphabetizing keys.
        let input = br#"{"id":"b0000001-0001-0001-0001-000000000001","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries","sourceTableProperties":{"itemId":"abc","sourceSchema":"dbo","sourceTableName":"t","sourceType":"LakehouseTable","workspaceId":"ws"}}}"#;
        let output = normalize_data_binding(input).unwrap();
        let wire = String::from_utf8(output).unwrap();
        let source_type_pos = wire.find("\"sourceType\"").expect("sourceType present");
        // Every other sourceTableProperties key must appear AFTER sourceType.
        for key in [
            "\"itemId\"",
            "\"sourceSchema\"",
            "\"sourceTableName\"",
            "\"workspaceId\"",
        ] {
            let pos = wire.find(key).unwrap_or_else(|| panic!("{key} present"));
            assert!(
                source_type_pos < pos,
                "sourceType must precede {key} on the wire (preserve_order regression); got: {wire}"
            );
        }
    }

    #[test]
    fn normalize_data_binding_already_ordered() {
        let input = br#"{"id":"b0000001-0001-0001-0001-000000000001","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries","sourceTableProperties":{"sourceType":"LakehouseTable","workspaceId":"ws","itemId":"abc","sourceTableName":"t"}}}"#;
        let output = normalize_data_binding(input).unwrap();
        let parsed: Value = serde_json::from_slice(&output).unwrap();
        let source_props = parsed["dataBindingConfiguration"]["sourceTableProperties"]
            .as_object()
            .unwrap();
        let keys: Vec<&String> = source_props.keys().collect();
        assert_eq!(keys[0], "sourceType");
    }

    #[test]
    fn normalize_data_binding_no_source_type_passthrough() {
        // If sourceType is missing, normalization still succeeds (passthrough)
        let input = br#"{"id":"b0000001-0001-0001-0001-000000000001","dataBindingConfiguration":{"dataBindingType":"NonTimeSeries","sourceTableProperties":{"workspaceId":"ws","itemId":"abc"}}}"#;
        let output = normalize_data_binding(input).unwrap();
        let parsed: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(parsed["id"], "b0000001-0001-0001-0001-000000000001");
    }

    #[test]
    fn normalize_data_binding_no_config_passthrough() {
        // If dataBindingConfiguration is missing, normalization still succeeds
        let input = br#"{"id":"b0000001-0001-0001-0001-000000000001","custom":"field"}"#;
        let output = normalize_data_binding(input).unwrap();
        let parsed: Value = serde_json::from_slice(&output).unwrap();
        assert_eq!(parsed["id"], "b0000001-0001-0001-0001-000000000001");
        assert_eq!(parsed["custom"], "field");
    }

    #[test]
    fn normalize_data_binding_rejects_non_uuid_id() {
        let input = br#"{"id":"not-a-uuid","dataBindingConfiguration":{}}"#;
        let result = normalize_data_binding(input);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("UUID format"),
            "Error should mention UUID: {err_msg}"
        );
    }

    #[test]
    fn normalize_data_binding_allows_missing_id() {
        // If id field is missing entirely, no validation needed (server will reject)
        let input = br#"{"dataBindingConfiguration":{"dataBindingType":"NonTimeSeries"}}"#;
        let result = normalize_data_binding(input);
        assert!(result.is_ok());
    }

    // -----------------------------------------------------------------------
    // Tests for ensure_platform_when_updating_metadata
    // -----------------------------------------------------------------------

    #[test]
    fn platform_check_skipped_when_metadata_not_requested() {
        // Without --update-metadata, a missing .platform is fine.
        let def = serde_json::json!({"parts": [{"path": "definition.json"}]});
        assert!(ensure_platform_when_updating_metadata(&def, false).is_ok());
    }

    #[test]
    fn platform_check_passes_when_platform_present() {
        let def = serde_json::json!({"parts": [
            {"path": "definition.json"},
            {"path": ".platform"},
        ]});
        assert!(ensure_platform_when_updating_metadata(&def, true).is_ok());
    }

    #[test]
    fn platform_check_fails_fast_without_platform() {
        // The import-generated shape: no .platform part.
        let def = serde_json::json!({"parts": [
            {"path": "definition.json"},
            {"path": "EntityTypes/8880000000001/definition.json"},
        ]});
        let err = ensure_platform_when_updating_metadata(&def, true).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(".platform"),
            "error should mention .platform: {msg}"
        );
    }

    // -----------------------------------------------------------------------
    // Tests for decode_definition_parts
    // -----------------------------------------------------------------------

    #[test]
    fn decode_definition_parts_json_payload() {
        let data = serde_json::json!({
            "definition": {
                "parts": [
                    {
                        "path": "definition.json",
                        "payload": BASE64.encode(b"{}"),
                        "payloadType": "InlineBase64"
                    },
                    {
                        "path": "EntityTypes/123/definition.json",
                        "payload": BASE64.encode(br#"{"id":"123","name":"Equipment"}"#),
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        let decoded = decode_definition_parts(data);
        let parts = decoded["definition"]["parts"].as_array().unwrap();

        // First part: empty JSON
        assert_eq!(parts[0]["decodedPayload"], serde_json::json!({}));

        // Second part: parsed JSON object
        assert_eq!(parts[1]["decodedPayload"]["id"], "123");
        assert_eq!(parts[1]["decodedPayload"]["name"], "Equipment");
    }

    #[test]
    fn decode_definition_parts_text_payload() {
        let ttl = "@prefix ex: <http://example.org/> .\nex:A a ex:Class .";
        let data = serde_json::json!({
            "definition": {
                "parts": [
                    {
                        "path": "ontology.ttl",
                        "payload": BASE64.encode(ttl.as_bytes()),
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        let decoded = decode_definition_parts(data);
        let parts = decoded["definition"]["parts"].as_array().unwrap();

        // Non-JSON text is stored as string
        assert_eq!(parts[0]["decodedPayload"].as_str().unwrap(), ttl);
    }

    #[test]
    fn decode_definition_parts_preserves_original_fields() {
        let data = serde_json::json!({
            "definition": {
                "parts": [
                    {
                        "path": "test.json",
                        "payload": BASE64.encode(b"{}"),
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        let decoded = decode_definition_parts(data);
        let part = &decoded["definition"]["parts"][0];

        // Original fields preserved
        assert_eq!(part["path"], "test.json");
        assert_eq!(part["payloadType"], "InlineBase64");
        assert!(part["payload"].is_string()); // original base64 still there
    }

    #[test]
    fn decode_definition_parts_no_definition_field() {
        let data = serde_json::json!({"other": "value"});
        let decoded = decode_definition_parts(data);
        // Should not crash, just return the input unchanged
        assert_eq!(decoded["other"], "value");
    }

    #[test]
    fn decode_definition_parts_binary_payload_skipped() {
        // Invalid UTF-8 bytes should not produce a decodedPayload
        let data = serde_json::json!({
            "definition": {
                "parts": [
                    {
                        "path": "binary.bin",
                        "payload": BASE64.encode([0xFF, 0xFE, 0x00, 0x80]),
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });

        let decoded = decode_definition_parts(data);
        let part = &decoded["definition"]["parts"][0];
        // Binary content cannot be decoded to UTF-8, so no decodedPayload
        assert!(part.get("decodedPayload").is_none());
    }
}
