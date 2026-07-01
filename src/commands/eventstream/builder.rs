//! Eventstream builder helpers: fetch/push definition, add source/destination,
//! add sample source, add derived stream, validate, list components.

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

/// Fetches the current eventstream definition, decodes it, returns the parsed JSON.
pub(super) async fn fetch_current_definition(
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<Value> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await?;

    // Extract the eventstream.json part
    let parts = data["definition"]["parts"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("No definition parts returned"))?;

    for part in parts {
        if part["path"].as_str() == Some("eventstream.json") {
            let payload = part["payload"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing payload in eventstream.json part"))?;
            let decoded = BASE64.decode(payload)?;
            let json_str = String::from_utf8(decoded)?;
            let parsed: Value = serde_json::from_str(&json_str)?;
            return Ok(parsed);
        }
    }

    Err(anyhow::anyhow!(
        "eventstream.json not found in definition parts"
    ))
}

/// Pushes updated definition back to the eventstream.
pub(super) async fn push_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    definition: &Value,
) -> Result<Value> {
    let json_str = serde_json::to_string(definition)?;
    let encoded = BASE64.encode(json_str.as_bytes());

    let body = serde_json::json!({
        "definition": {
            "parts": [
                {
                    "path": "eventstream.json",
                    "payload": encoded,
                    "payloadType": "InlineBase64"
                }
            ]
        }
    });

    let data = client
        .post(
            &format!("/workspaces/{workspace}/eventstreams/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "eventstream update-definition", "Contributor"))?;

    // After update, fetch the topology to return the new source/destination with its server-assigned ID
    let topology = client
        .get(&format!(
            "/workspaces/{workspace}/eventstreams/{id}/topology"
        ))
        .await;

    if let Ok(topo) = topology {
        output::render_object(cli, &topo, "id");
        return Ok(topo);
    }

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
        Ok(obj)
    } else {
        output::render_object(cli, &data, "id");
        Ok(data)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    source_type: &str,
    properties: Option<&str>,
) -> Result<()> {
    let props: Value = match properties {
        Some(p) => serde_json::from_str(p).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --properties: {e}"),
                "Example: --properties '{}'".to_string(),
            )
        })?,
        None => serde_json::json!({}),
    };

    if output::dry_run_guard(
        cli,
        "eventstream add-source",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "source": { "name": name, "type": source_type, "properties": props }
        }),
    ) {
        return Ok(());
    }

    // 1. Fetch current definition
    let mut def = fetch_current_definition(client, workspace, id).await?;

    // 2. Add the new source
    let new_source = serde_json::json!({
        "name": name,
        "type": source_type,
        "properties": props,
    });

    let sources = def["sources"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing sources array"))?;
    sources.push(new_source);

    // 3. Add a default stream for this source if no stream references it yet
    let streams = def["streams"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing streams array"))?;
    let has_stream = streams.iter().any(|s| {
        s["inputNodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|n| n["name"].as_str() == Some(name)))
    });
    if !has_stream {
        let stream_name = format!("{name}-stream");
        streams.push(serde_json::json!({
            "name": stream_name,
            "type": "DefaultStream",
            "properties": {},
            "inputNodes": [{"name": name}]
        }));
    }

    // 4. Push updated definition
    push_definition(cli, client, workspace, id, &def).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_destination(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    destination_type: &str,
    properties: Option<&str>,
    input_node: &str,
) -> Result<()> {
    let props: Value = match properties {
        Some(p) => serde_json::from_str(p).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --properties: {e}"),
                "Example: --properties '{{\"workspaceId\":\"...\",\"itemId\":\"...\"}}'"
                    .to_string(),
            )
        })?,
        None => serde_json::json!({}),
    };

    if output::dry_run_guard(
        cli,
        "eventstream add-destination",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "destination": {
                "name": name,
                "type": destination_type,
                "properties": props,
                "inputNodes": [{"name": input_node}]
            }
        }),
    ) {
        return Ok(());
    }

    // 1. Fetch current definition
    let mut def = fetch_current_definition(client, workspace, id).await?;

    // 2. Add the new destination
    let new_dest = serde_json::json!({
        "name": name,
        "type": destination_type,
        "properties": props,
        "inputNodes": [{"name": input_node}]
    });

    let destinations = def["destinations"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing destinations array"))?;
    destinations.push(new_dest);

    // 3. Push updated definition
    push_definition(cli, client, workspace, id, &def).await?;
    Ok(())
}

// ─── Builder Helpers ─────────────────────────────────────────────────────────

pub(super) async fn add_sample_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "eventstream add-sample-source",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "source": { "name": name, "type": "SampleData" }
        }),
    ) {
        return Ok(());
    }

    let mut def = fetch_current_definition(client, workspace, id).await?;

    // Add sample data source
    let new_source = serde_json::json!({
        "name": name,
        "type": "SampleData",
        "properties": {},
    });

    let sources = def["sources"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing sources array"))?;
    sources.push(new_source);

    // Auto-create default stream
    let streams = def["streams"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing streams array"))?;
    let has_stream = streams.iter().any(|s| {
        s["inputNodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|n| n["name"].as_str() == Some(name)))
    });
    if !has_stream {
        let stream_name = format!("{name}-stream");
        streams.push(serde_json::json!({
            "name": stream_name,
            "type": "DefaultStream",
            "properties": {},
            "inputNodes": [{"name": name}]
        }));
    }

    push_definition(cli, client, workspace, id, &def).await?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn add_derived_stream(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    input_node: &str,
    properties: Option<&str>,
) -> Result<()> {
    let props: Value = match properties {
        Some(p) => serde_json::from_str(p).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --properties: {e}"),
                "Example: --properties '{}'".to_string(),
            )
        })?,
        None => serde_json::json!({}),
    };

    if output::dry_run_guard(
        cli,
        "eventstream add-derived-stream",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "stream": { "name": name, "type": "DerivedStream", "inputNodes": [{"name": input_node}], "properties": props }
        }),
    ) {
        return Ok(());
    }

    let mut def = fetch_current_definition(client, workspace, id).await?;

    let new_stream = serde_json::json!({
        "name": name,
        "type": "DerivedStream",
        "properties": props,
        "inputNodes": [{"name": input_node}]
    });

    let streams = def["streams"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing streams array"))?;
    streams.push(new_stream);

    push_definition(cli, client, workspace, id, &def).await?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
pub(super) async fn validate(
    cli: &Cli,
    client: &FabricClient,
    workspace: Option<&str>,
    id: Option<&str>,
    file: Option<&str>,
) -> Result<()> {
    // Load definition from file or server
    let def: Value = if let Some(path) = file {
        let content = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?;
        serde_json::from_str(&content).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in file: {e}"),
                "File must contain a valid eventstream definition JSON.".to_string(),
            )
        })?
    } else {
        let ws = workspace.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --workspace + --id must be provided.".to_string(),
                "Example: fabio eventstream validate --file definition.json".to_string(),
            )
        })?;
        let item_id = id.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--id is required when fetching definition from server.".to_string(),
                "Example: fabio eventstream validate --workspace <WS> --id <ID>".to_string(),
            )
        })?;
        fetch_current_definition(client, ws, item_id).await?
    };

    // Perform client-side validation
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // Check required top-level arrays
    let sources = def.get("sources").and_then(Value::as_array);
    let streams = def.get("streams").and_then(Value::as_array);
    let destinations = def.get("destinations").and_then(Value::as_array);

    if sources.is_none() {
        errors.push("Missing 'sources' array in definition.".to_string());
    }
    if streams.is_none() {
        errors.push("Missing 'streams' array in definition.".to_string());
    }
    if destinations.is_none() {
        errors.push("Missing 'destinations' array in definition.".to_string());
    }

    if let (Some(srcs), Some(strs), Some(dests)) = (sources, streams, destinations) {
        // Check for at least one source
        if srcs.is_empty() {
            warnings.push("No sources defined.".to_string());
        }
        if dests.is_empty() {
            warnings.push("No destinations defined.".to_string());
        }

        // Collect all node names for reference validation
        let mut all_names: Vec<&str> = Vec::new();
        let mut duplicates: Vec<String> = Vec::new();

        for src in srcs {
            if let Some(name) = src.get("name").and_then(Value::as_str) {
                if all_names.contains(&name) {
                    duplicates.push(format!("Duplicate node name: '{name}'"));
                }
                all_names.push(name);
            } else {
                errors.push("Source missing 'name' field.".to_string());
            }
            if src.get("type").and_then(Value::as_str).is_none() {
                errors.push(format!(
                    "Source '{}' missing 'type' field.",
                    src.get("name").and_then(Value::as_str).unwrap_or("?")
                ));
            }
        }

        for stream in strs {
            if let Some(name) = stream.get("name").and_then(Value::as_str) {
                if all_names.contains(&name) {
                    duplicates.push(format!("Duplicate node name: '{name}'"));
                }
                all_names.push(name);
            } else {
                errors.push("Stream missing 'name' field.".to_string());
            }
        }

        for dest in dests {
            if let Some(name) = dest.get("name").and_then(Value::as_str) {
                if all_names.contains(&name) {
                    duplicates.push(format!("Duplicate node name: '{name}'"));
                }
                all_names.push(name);
            } else {
                errors.push("Destination missing 'name' field.".to_string());
            }
        }

        errors.extend(duplicates);

        // Validate inputNodes references
        let all_nodes_with_inputs: Vec<&Value> = strs.iter().chain(dests.iter()).collect();

        for node in &all_nodes_with_inputs {
            if let Some(inputs) = node.get("inputNodes").and_then(Value::as_array) {
                for input in inputs {
                    if let Some(ref_name) = input.get("name").and_then(Value::as_str)
                        && !all_names.contains(&ref_name)
                    {
                        errors.push(format!(
                            "Node '{}' references non-existent inputNode '{ref_name}'.",
                            node.get("name").and_then(Value::as_str).unwrap_or("?")
                        ));
                    }
                }
            }
        }
    }

    let valid = errors.is_empty();
    let result = serde_json::json!({
        "valid": valid,
        "errors": errors,
        "warnings": warnings,
    });
    output::render_object(cli, &result, "valid");
    Ok(())
}

pub(super) fn list_components(cli: &Cli, category: &str) {
    let sources = serde_json::json!([
        {"type": "CustomEndpoint", "category": "source", "description": "Custom app endpoint (Event Hub-compatible)"},
        {"type": "AzureEventHub", "category": "source", "description": "Azure Event Hub"},
        {"type": "AzureIoTHub", "category": "source", "description": "Azure IoT Hub"},
        {"type": "SampleData", "category": "source", "description": "Built-in sample/simulated data"},
        {"type": "AmazonKinesis", "category": "source", "description": "Amazon Kinesis Data Streams"},
        {"type": "ApacheKafka", "category": "source", "description": "Apache Kafka cluster"},
        {"type": "ConfluentCloud", "category": "source", "description": "Confluent Cloud Kafka"},
        {"type": "GooglePubSub", "category": "source", "description": "Google Cloud Pub/Sub"},
        {"type": "AzureSQLDBCDC", "category": "source", "description": "Azure SQL Database CDC"},
        {"type": "MySQLCDC", "category": "source", "description": "MySQL Change Data Capture"},
        {"type": "PostgreSQLCDC", "category": "source", "description": "PostgreSQL Change Data Capture"},
        {"type": "FabricWorkspaceItemEvents", "category": "source", "description": "Fabric workspace item events"},
        {"type": "FabricJobEvents", "category": "source", "description": "Fabric job events"},
        {"type": "FabricOneLakeEvents", "category": "source", "description": "Fabric OneLake events"},
        {"type": "FabricAnomalyDetectionEvents", "category": "source", "description": "Fabric anomaly detection events"},
    ]);

    let destinations = serde_json::json!([
        {"type": "Eventhouse", "category": "destination", "description": "KQL Database in an Eventhouse"},
        {"type": "Lakehouse", "category": "destination", "description": "Delta tables in a Lakehouse"},
        {"type": "CustomEndpoint", "category": "destination", "description": "Custom app endpoint (Event Hub-compatible)"},
        {"type": "Activator", "category": "destination", "description": "Data Activator (Reflex) trigger"},
    ]);

    let items: Vec<Value> = match category {
        "source" => sources.as_array().cloned().unwrap_or_default(),
        "destination" => destinations.as_array().cloned().unwrap_or_default(),
        _ => {
            let mut all = sources.as_array().cloned().unwrap_or_default();
            all.extend(destinations.as_array().cloned().unwrap_or_default());
            all
        }
    };

    output::render_list(
        cli,
        &items,
        &["type", "category", "description"],
        &["TYPE", "CATEGORY", "DESCRIPTION"],
        "type",
    );
}
