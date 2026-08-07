use anyhow::Result;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples eventstream\nAlso available: fabio context schema Eventstream | fabio context workflow rti-pipeline"
)]
pub enum EventstreamCommand {
    /// List eventstreams in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of an eventstream
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,
    },
    /// Create a new eventstream
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update eventstream properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete an eventstream
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },

    // ── Definitions ──────────────────────────────────────────────────────
    /// Get the definition of an eventstream
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of an eventstream
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Definition file path (reads file content)
        #[arg(long)]
        file: Option<String>,

        /// Definition content (inline)
        #[arg(long)]
        content: Option<String>,
    },

    // ── Topology ─────────────────────────────────────────────────────────
    /// Get the topology of an eventstream
    #[command(display_order = 10)]
    GetTopology {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,
    },

    // ── Stream Control ───────────────────────────────────────────────────
    /// Pause the entire eventstream
    #[command(display_order = 11)]
    Pause {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,
    },
    /// Resume the entire eventstream
    #[command(display_order = 12)]
    Resume {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Start type: `Now` (default), `WhenLastStopped`, or `CustomTime`.
        #[arg(long, default_value = "Now")]
        start_type: String,

        /// Custom UTC start time (`YYYY-MM-DDTHH:mm:ssZ`); required for `--start-type CustomTime`.
        #[arg(long)]
        custom_start_time: Option<String>,
    },

    // ── Destinations ─────────────────────────────────────────────────────
    /// Get details of a destination
    #[command(display_order = 20)]
    GetDestination {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Destination ID
        #[arg(long)]
        destination_id: String,
    },
    /// Get the connection of a destination
    #[command(display_order = 21)]
    GetDestinationConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Destination ID
        #[arg(long)]
        destination_id: String,
    },
    /// Pause a destination
    #[command(display_order = 22)]
    PauseDestination {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Destination ID
        #[arg(long)]
        destination_id: String,
    },
    /// Resume a destination
    #[command(display_order = 23)]
    ResumeDestination {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Destination ID
        #[arg(long)]
        destination_id: String,

        /// Start type: `Now` (default), `WhenLastStopped`, or `CustomTime`.
        #[arg(long, default_value = "Now")]
        start_type: String,

        /// Custom UTC start time (`YYYY-MM-DDTHH:mm:ssZ`); required for `--start-type CustomTime`.
        #[arg(long)]
        custom_start_time: Option<String>,
    },

    // ── Sources ──────────────────────────────────────────────────────────
    /// Get details of a source
    #[command(display_order = 30)]
    GetSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source ID
        #[arg(long)]
        source_id: String,
    },
    /// Get the connection of a source
    #[command(display_order = 31)]
    GetSourceConnection {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source ID
        #[arg(long)]
        source_id: String,
    },
    /// Pause a source
    #[command(display_order = 32)]
    PauseSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source ID
        #[arg(long)]
        source_id: String,
    },
    /// Resume a source
    #[command(display_order = 33)]
    ResumeSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source ID
        #[arg(long)]
        source_id: String,

        /// Start type: `Now` (default), `WhenLastStopped`, or `CustomTime`.
        #[arg(long, default_value = "Now")]
        start_type: String,

        /// Custom UTC start time (`YYYY-MM-DDTHH:mm:ssZ`); required for `--start-type CustomTime`.
        #[arg(long)]
        custom_start_time: Option<String>,
    },

    // ── High-level helpers ───────────────────────────────────────────────
    /// Add a source to an eventstream (fetches current definition, merges, and updates)
    #[command(display_order = 40)]
    AddSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source name (unique within the eventstream)
        #[arg(long)]
        name: String,

        /// Source type (e.g., `CustomEndpoint`, `AzureEventHub`, `SampleData`)
        #[arg(long, visible_alias = "type")]
        source_type: String,

        /// Source properties as JSON string (default: {})
        #[arg(long)]
        properties: Option<String>,
    },

    /// Add a destination to an eventstream (fetches current definition, merges, and updates)
    #[command(display_order = 41)]
    AddDestination {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Destination name (unique within the eventstream)
        #[arg(long)]
        name: String,

        /// Destination type (e.g., `Eventhouse`, `Lakehouse`, `CustomEndpoint`, `Activator`)
        #[arg(long, visible_alias = "type")]
        destination_type: String,

        /// Destination properties as JSON string
        #[arg(long)]
        properties: Option<String>,

        /// Input node name (the stream or operator this destination reads from)
        #[arg(long)]
        input_node: String,
    },

    /// Add a sample data source to an eventstream (high-level helper)
    #[command(name = "add-sample-source", display_order = 42)]
    AddSampleSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Source name (unique within the eventstream)
        #[arg(long)]
        name: String,

        /// Sample dataset: `Bicycles`, `YellowTaxi`, `StockMarket`, `Buses` (default: `Bicycles`)
        #[arg(long = "sample-type", default_value = "Bicycles")]
        sample_type: String,
    },

    /// Add a derived stream (filtered/transformed) between existing nodes
    #[command(name = "add-derived-stream", display_order = 43)]
    AddDerivedStream {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Derived stream name (unique within the eventstream)
        #[arg(long)]
        name: String,

        /// Input node name (the source or stream to derive from)
        #[arg(long)]
        input_node: String,

        /// Stream properties as JSON string (filter/transform config)
        #[arg(long)]
        properties: Option<String>,
    },

    /// Add an event-processor operator (Filter/ManageFields/Aggregate/GroupBy/Join/Union/Expand) node
    #[command(name = "add-operator", display_order = 44)]
    AddOperator {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Eventstream ID
        #[arg(long)]
        id: String,

        /// Operator name (unique within the eventstream)
        #[arg(long)]
        name: String,

        /// Operator type: `Filter`, `ManageFields`, `Aggregate`, `GroupBy`, `Join`, `Union`, `Expand`
        #[arg(long = "type")]
        operator_type: String,

        /// Input node name (the source, stream, or operator to transform)
        #[arg(long)]
        input_node: String,

        /// Operator properties as JSON string (operator-specific, e.g. Filter → {"conditions":[…]})
        #[arg(long)]
        properties: Option<String>,
    },

    /// Validate an eventstream definition (client-side checks, no API call)
    #[command(display_order = 44)]
    Validate {
        /// Workspace ID (used to fetch definition from server if --file not provided)
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: Option<String>,

        /// Eventstream ID (used to fetch definition from server if --file not provided)
        #[arg(long)]
        id: Option<String>,

        /// Path to a local eventstream definition JSON file
        #[arg(long)]
        file: Option<String>,
    },

    /// List available eventstream component types (sources, destinations, operators)
    #[command(name = "list-components", display_order = 45)]
    ListComponents {
        /// Filter by category: source, destination, operator, all (default: all)
        #[arg(long, default_value = "all")]
        category: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &EventstreamCommand) -> Result<()> {
    match command {
        EventstreamCommand::List { workspace } => list(cli, client, workspace).await,
        EventstreamCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        EventstreamCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        EventstreamCommand::Update {
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
        EventstreamCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        EventstreamCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        EventstreamCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        EventstreamCommand::GetTopology { workspace, id } => {
            get_topology(cli, client, workspace, id).await
        }
        EventstreamCommand::Pause { workspace, id } => {
            pause_stream(cli, client, workspace, id).await
        }
        EventstreamCommand::Resume {
            workspace,
            id,
            start_type,
            custom_start_time,
        } => {
            resume_stream(
                cli,
                client,
                workspace,
                id,
                start_type,
                custom_start_time.as_deref(),
            )
            .await
        }
        EventstreamCommand::GetDestination {
            workspace,
            id,
            destination_id,
        } => get_destination(cli, client, workspace, id, destination_id).await,
        EventstreamCommand::GetDestinationConnection {
            workspace,
            id,
            destination_id,
        } => get_destination_connection(cli, client, workspace, id, destination_id).await,
        EventstreamCommand::PauseDestination {
            workspace,
            id,
            destination_id,
        } => pause_destination(cli, client, workspace, id, destination_id).await,
        EventstreamCommand::ResumeDestination {
            workspace,
            id,
            destination_id,
            start_type,
            custom_start_time,
        } => {
            resume_destination(
                cli,
                client,
                workspace,
                id,
                destination_id,
                start_type,
                custom_start_time.as_deref(),
            )
            .await
        }
        EventstreamCommand::GetSource {
            workspace,
            id,
            source_id,
        } => get_source(cli, client, workspace, id, source_id).await,
        EventstreamCommand::GetSourceConnection {
            workspace,
            id,
            source_id,
        } => get_source_connection(cli, client, workspace, id, source_id).await,
        EventstreamCommand::PauseSource {
            workspace,
            id,
            source_id,
        } => pause_source(cli, client, workspace, id, source_id).await,
        EventstreamCommand::ResumeSource {
            workspace,
            id,
            source_id,
            start_type,
            custom_start_time,
        } => {
            resume_source(
                cli,
                client,
                workspace,
                id,
                source_id,
                start_type,
                custom_start_time.as_deref(),
            )
            .await
        }
        EventstreamCommand::AddSource {
            workspace,
            id,
            name,
            source_type,
            properties,
        } => {
            builder::add_source(
                cli,
                client,
                workspace,
                id,
                name,
                source_type,
                properties.as_deref(),
            )
            .await
        }
        EventstreamCommand::AddDestination {
            workspace,
            id,
            name,
            destination_type,
            properties,
            input_node,
        } => {
            builder::add_destination(
                cli,
                client,
                workspace,
                id,
                name,
                destination_type,
                properties.as_deref(),
                input_node,
            )
            .await
        }
        EventstreamCommand::AddSampleSource {
            workspace,
            id,
            name,
            sample_type,
        } => builder::add_sample_source(cli, client, workspace, id, name, sample_type).await,
        EventstreamCommand::AddDerivedStream {
            workspace,
            id,
            name,
            input_node,
            properties,
        } => {
            builder::add_derived_stream(
                cli,
                client,
                workspace,
                id,
                name,
                input_node,
                properties.as_deref(),
            )
            .await
        }
        EventstreamCommand::AddOperator {
            workspace,
            id,
            name,
            operator_type,
            input_node,
            properties,
        } => {
            builder::add_operator(
                cli,
                client,
                workspace,
                id,
                name,
                operator_type,
                input_node,
                properties.as_deref(),
            )
            .await
        }
        EventstreamCommand::Validate {
            workspace,
            id,
            file,
        } => {
            builder::validate(
                cli,
                client,
                workspace.as_deref(),
                id.as_deref(),
                file.as_deref(),
            )
            .await
        }
        EventstreamCommand::ListComponents { category } => {
            builder::list_components(cli, category);
            Ok(())
        }
    }
}

// ─── CRUD ────────────────────────────────────────────────────────────────────

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/eventstreams"),
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
        .get(&format!("/workspaces/{workspace}/eventstreams/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
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
        "eventstream create",
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
            &format!("/workspaces/{workspace}/eventstreams"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream create", "Member"))?;
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
            "Example: fabio eventstream update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "eventstream update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/eventstreams/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream update", "Contributor"))?;
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
        "eventstream delete",
        &serde_json::json!({
            "workspace": workspace,
            "id": id, "hardDelete": hard_delete
        }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/eventstreams/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/eventstreams/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream delete", "Member"))?;

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
            &format!("/workspaces/{workspace}/eventstreams/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream get-definition", "Contributor"))?;
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
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio eventstream update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            ).into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(&script, "eventstream.json");

    if output::dry_run_guard(
        cli,
        "eventstream update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Topology ────────────────────────────────────────────────────────────────

async fn get_topology(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/topology"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

// ─── Stream Control ──────────────────────────────────────────────────────────

async fn pause_stream(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "eventstream pause",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/pause"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream pause", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "paused" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn resume_stream(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    start_type: &str,
    custom_start_time: Option<&str>,
) -> Result<()> {
    let body = build_start_request(start_type, custom_start_time)?;
    if output::dry_run_guard(
        cli,
        "eventstream resume",
        &serde_json::json!({ "workspace": workspace, "id": id, "startType": start_type }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/resume"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream resume", "Contributor"))?;

    let obj = serde_json::json!({ "id": id, "status": "resumed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Build the `DataSourceStartRequest` body for an eventstream/source/destination
/// resume. `startType` is REQUIRED by the API (`Now`, `WhenLastStopped`, or
/// `CustomTime`); `CustomTime` additionally requires `customStartDateTime`.
fn build_start_request(start_type: &str, custom_start_time: Option<&str>) -> Result<Value> {
    let valid = ["Now", "WhenLastStopped", "CustomTime"];
    let matched = valid
        .iter()
        .find(|v| v.eq_ignore_ascii_case(start_type))
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --start-type '{start_type}'"),
                "Valid values: Now, WhenLastStopped, CustomTime",
            )
        })?;

    let mut body = serde_json::json!({ "startType": matched });
    if matched.eq_ignore_ascii_case("CustomTime") {
        let ts = custom_start_time.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--custom-start-time is required when --start-type is CustomTime",
                "Example: --start-type CustomTime --custom-start-time 2026-01-01T00:00:00Z",
            )
        })?;
        body["customStartDateTime"] = Value::from(ts);
    } else if let Some(ts) = custom_start_time {
        // Allow supplying a custom time with any type; the API ignores it unless CustomTime.
        body["customStartDateTime"] = Value::from(ts);
    }
    Ok(body)
}

// ─── Destinations ────────────────────────────────────────────────────────────

async fn get_destination(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    destination_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/destinations/{destination_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_destination_connection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    destination_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/destinations/{destination_id}/connection"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn pause_destination(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    destination_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "eventstream pause-destination",
        &serde_json::json!({ "workspace": workspace, "id": id, "destinationId": destination_id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!(
                "/workspaces/{workspace}/eventstreams/{id}/destinations/{destination_id}/pause"
            ),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream pause-destination", "Contributor"))?;

    let obj = serde_json::json!({ "id": destination_id, "status": "paused" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn resume_destination(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    destination_id: &str,
    start_type: &str,
    custom_start_time: Option<&str>,
) -> Result<()> {
    let body = build_start_request(start_type, custom_start_time)?;
    if output::dry_run_guard(
        cli,
        "eventstream resume-destination",
        &serde_json::json!({ "workspace": workspace, "id": id, "destinationId": destination_id, "startType": start_type }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!(
                "/workspaces/{workspace}/eventstreams/{id}/destinations/{destination_id}/resume"
            ),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream resume-destination", "Contributor"))?;

    let obj = serde_json::json!({ "id": destination_id, "status": "resumed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Sources ─────────────────────────────────────────────────────────────────

async fn get_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/sources/{source_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn get_source_connection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/sources/{source_id}/connection"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn pause_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "eventstream pause-source",
        &serde_json::json!({ "workspace": workspace, "id": id, "sourceId": source_id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/sources/{source_id}/pause"),
            &serde_json::json!({}),
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream pause-source", "Contributor"))?;

    let obj = serde_json::json!({ "id": source_id, "status": "paused" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn resume_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    source_id: &str,
    start_type: &str,
    custom_start_time: Option<&str>,
) -> Result<()> {
    let body = build_start_request(start_type, custom_start_time)?;
    if output::dry_run_guard(
        cli,
        "eventstream resume-source",
        &serde_json::json!({ "workspace": workspace, "id": id, "sourceId": source_id, "startType": start_type }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/sources/{source_id}/resume"),
            &body,
            false,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream resume-source", "Contributor"))?;

    let obj = serde_json::json!({ "id": source_id, "status": "resumed" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

mod builder;

#[cfg(test)]
mod tests {
    use super::build_start_request;

    #[test]
    fn default_now_start_type() {
        let body = build_start_request("Now", None).unwrap();
        assert_eq!(body["startType"], "Now");
        assert!(body.get("customStartDateTime").is_none());
    }

    #[test]
    fn when_last_stopped_is_valid() {
        let body = build_start_request("WhenLastStopped", None).unwrap();
        assert_eq!(body["startType"], "WhenLastStopped");
    }

    #[test]
    fn custom_time_requires_timestamp() {
        assert!(build_start_request("CustomTime", None).is_err());
        let body = build_start_request("CustomTime", Some("2026-01-01T00:00:00Z")).unwrap();
        assert_eq!(body["startType"], "CustomTime");
        assert_eq!(body["customStartDateTime"], "2026-01-01T00:00:00Z");
    }

    #[test]
    fn start_type_is_case_insensitive_and_canonicalized() {
        let body = build_start_request("now", None).unwrap();
        assert_eq!(body["startType"], "Now");
    }

    #[test]
    fn invalid_start_type_rejected() {
        assert!(build_start_request("Later", None).is_err());
    }
}
