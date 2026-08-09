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

    // Validate mode-specific shape for an Eventhouse destination BEFORE any
    // network call (and before the dry-run guard) — an Eventhouse destination
    // with the wrong property set fails SILENTLY server-side (the destination
    // sits in `Warning`, ingests nothing, logs no ingestion failure).
    if destination_type.eq_ignore_ascii_case("eventhouse") {
        validate_eventhouse_destination(&props)?;
    }

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

/// Validate an Eventhouse destination's `properties` per its `dataIngestionMode`
/// and return a teaching error BEFORE any network call.
///
/// The two Eventhouse ingestion modes require disjoint field sets, and getting
/// them wrong fails SILENTLY server-side (the destination sits in `Warning`,
/// ingests nothing, and logs no ingestion failure):
/// - `ProcessedIngestion` — the eventstream provisions ingestion itself; needs
///   `workspaceId`, `itemId`, `databaseName`, `tableName`, `inputSerialization`.
/// - `DirectIngestion` — references a pre-existing Kusto data connection +
///   mapping you created on the Eventhouse; needs `workspaceId`, `itemId`,
///   `connectionName`, `mappingRuleName`.
pub(super) fn validate_eventhouse_destination(props: &Value) -> Result<()> {
    let Some(mode) = props.get("dataIngestionMode").and_then(Value::as_str) else {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Eventhouse destination requires 'dataIngestionMode' in --properties".to_string(),
            "Set \"dataIngestionMode\" to \"ProcessedIngestion\" (eventstream ingests into a KQL table \
             — needs databaseName, tableName, inputSerialization) or \"DirectIngestion\" (references a \
             pre-existing Kusto data connection — needs connectionName, mappingRuleName)."
                .to_string(),
        )
        .into());
    };

    let required: &[&str] = match mode {
        "ProcessedIngestion" => &[
            "workspaceId",
            "itemId",
            "databaseName",
            "tableName",
            "inputSerialization",
        ],
        "DirectIngestion" => &["workspaceId", "itemId", "connectionName", "mappingRuleName"],
        other => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid Eventhouse dataIngestionMode: '{other}'"),
                "Valid values (PascalCase): ProcessedIngestion, DirectIngestion.".to_string(),
            )
            .into());
        }
    };

    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|k| props.get(*k).is_none_or(Value::is_null))
        .collect();
    if !missing.is_empty() {
        let hint = if mode == "ProcessedIngestion" {
            "ProcessedIngestion example: --properties '{\"dataIngestionMode\":\"ProcessedIngestion\",\
             \"workspaceId\":\"<ws>\",\"itemId\":\"<kqlDbId>\",\"databaseName\":\"<kqlDbName>\",\
             \"tableName\":\"<table>\",\"inputSerialization\":{\"type\":\"Json\",\"properties\":{\"encoding\":\"UTF8\"}}}'"
        } else {
            "DirectIngestion references a Kusto data connection + ingestion mapping you must create on \
             the Eventhouse first. Example: --properties '{\"dataIngestionMode\":\"DirectIngestion\",\
             \"workspaceId\":\"<ws>\",\"itemId\":\"<kqlDbId>\",\"connectionName\":\"<conn>\",\
             \"mappingRuleName\":\"<mapping>\"}'"
        };
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Eventhouse {mode} destination is missing required properties: {}",
                missing.join(", ")
            ),
            hint.to_string(),
        )
        .into());
    }
    Ok(())
}

/// Normalize a sample-data type to its canonical `properties.type` value.
/// Accepts common labels/casing ("yellow taxi", "stock-market", "bicycle").
pub(super) fn normalize_sample_type(input: &str) -> &'static str {
    let key: String = input
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase();
    match key.as_str() {
        "yellowtaxi" | "taxi" => "YellowTaxi",
        "stockmarket" | "stock" | "stocks" => "StockMarket",
        "buses" | "bus" => "Buses",
        // default and "bicycles"/"bicycle"/"bikes"
        _ => "Bicycles",
    }
}

pub(super) async fn add_sample_source(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    sample_type: &str,
) -> Result<()> {
    // The SampleData source's `properties.type` is the dataset name (Bicycles,
    // YellowTaxi, StockMarket, Buses) — accept common aliases/casing.
    let sample_type = normalize_sample_type(sample_type);
    if output::dry_run_guard(
        cli,
        "eventstream add-sample-source",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "source": { "name": name, "type": "SampleData", "properties": { "type": sample_type } }
        }),
    ) {
        return Ok(());
    }

    let mut def = fetch_current_definition(client, workspace, id).await?;

    // Add sample data source
    let new_source = serde_json::json!({
        "name": name,
        "type": "SampleData",
        "properties": { "type": sample_type },
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

/// Valid event-processor operator types.
const OPERATOR_TYPES: &[&str] = &[
    "Filter",
    "ManageFields",
    "Aggregate",
    "GroupBy",
    "Join",
    "Union",
    "Expand",
];

/// Add an event-processor operator (Filter/ManageFields/Aggregate/…) node to the
/// eventstream's `operators` array, wired to `input_node`. The `properties` shape
/// is operator-specific (e.g. Filter → `{conditions:[…]}`).
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_operator(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    operator_type: &str,
    input_node: &str,
    properties: Option<&str>,
) -> Result<()> {
    let operator_type = OPERATOR_TYPES
        .iter()
        .find(|t| t.eq_ignore_ascii_case(operator_type))
        .copied()
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown operator type: '{operator_type}'"),
                format!("Valid operator types: {}", OPERATOR_TYPES.join(", ")),
            )
        })?;

    let props: Value = match properties {
        Some(p) => serde_json::from_str(p).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in --properties: {e}"),
                "Example (Filter): --properties '{\"conditions\":[…]}'. Discover operator types with: fabio eventstream list-components --category operator".to_string(),
            )
        })?,
        None => serde_json::json!({}),
    };

    let new_operator = serde_json::json!({
        "name": name,
        "type": operator_type,
        "inputNodes": [{"name": input_node}],
        "properties": props,
    });

    if output::dry_run_guard(
        cli,
        "eventstream add-operator",
        &serde_json::json!({ "workspace": workspace, "id": id, "operator": new_operator }),
    ) {
        return Ok(());
    }

    let mut def = fetch_current_definition(client, workspace, id).await?;
    let operators = def["operators"]
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("Definition missing operators array"))?;
    operators.push(new_operator);

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
        {"type": "AzureIoTHubExtended", "category": "source", "description": "Azure IoT Hub Extended"},
        {"type": "SampleData", "category": "source", "description": "Built-in sample/simulated data"},
        {"type": "AmazonKinesis", "category": "source", "description": "Amazon Kinesis Data Streams"},
        {"type": "ApacheKafka", "category": "source", "description": "Apache Kafka cluster"},
        {"type": "ConfluentCloud", "category": "source", "description": "Confluent Cloud Kafka"},
        {"type": "GooglePubSub", "category": "source", "description": "Google Cloud Pub/Sub"},
        {"type": "AzureSQLDBCDC", "category": "source", "description": "Azure SQL Database CDC"},
        {"type": "MirroredDatabaseChangeFeed", "category": "source", "description": "Mirrored Database Change Feed"},
        {"type": "MySQLCDC", "category": "source", "description": "MySQL Change Data Capture"},
        {"type": "OracleDBCDC", "category": "source", "description": "Oracle DB Change Data Capture"},
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
        {"type": "Notebook", "category": "destination", "description": "Fabric Notebook (real-time processing)"},
    ]);

    // Event-processor operators (transform nodes between sources and destinations).
    // Authored via `add-derived-stream --properties` (the derived stream carries
    // the operator's `operatorProperties`).
    let operators = serde_json::json!([
        {"type": "Filter", "category": "operator", "description": "Keep only events matching a condition (WHERE)"},
        {"type": "ManageFields", "category": "operator", "description": "Add, remove, or rename output fields (incl. built-in function fields)"},
        {"type": "Aggregate", "category": "operator", "description": "Aggregate a field (SUM/AVG/MIN/MAX/COUNT) over a tumbling time window"},
        {"type": "GroupBy", "category": "operator", "description": "Aggregate over a time window grouped by one or more fields"},
        {"type": "Join", "category": "operator", "description": "Join two input streams on a condition (inner/left)"},
        {"type": "Union", "category": "operator", "description": "Combine multiple streams that share a schema into one"},
        {"type": "Expand", "category": "operator", "description": "Expand (unroll) an array field into multiple events"},
    ]);

    let items: Vec<Value> = match category {
        "source" => sources.as_array().cloned().unwrap_or_default(),
        "destination" => destinations.as_array().cloned().unwrap_or_default(),
        "operator" => operators.as_array().cloned().unwrap_or_default(),
        _ => {
            let mut all = sources.as_array().cloned().unwrap_or_default();
            all.extend(destinations.as_array().cloned().unwrap_or_default());
            all.extend(operators.as_array().cloned().unwrap_or_default());
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

#[cfg(test)]
mod tests {
    use super::{OPERATOR_TYPES, normalize_sample_type, validate_eventhouse_destination};
    use serde_json::json;

    #[test]
    fn sample_type_normalizes_labels_and_casing() {
        assert_eq!(normalize_sample_type("StockMarket"), "StockMarket");
        assert_eq!(normalize_sample_type("stock market"), "StockMarket");
        assert_eq!(normalize_sample_type("stock-market"), "StockMarket");
        assert_eq!(normalize_sample_type("Yellow Taxi"), "YellowTaxi");
        assert_eq!(normalize_sample_type("taxi"), "YellowTaxi");
        assert_eq!(normalize_sample_type("buses"), "Buses");
        assert_eq!(normalize_sample_type("bicycle"), "Bicycles");
        // Unknown falls back to the default sample dataset.
        assert_eq!(normalize_sample_type("whatever"), "Bicycles");
    }

    #[test]
    fn operator_types_cover_the_seven_event_processors() {
        for t in [
            "Filter",
            "ManageFields",
            "Aggregate",
            "GroupBy",
            "Join",
            "Union",
            "Expand",
        ] {
            assert!(OPERATOR_TYPES.contains(&t), "missing operator type {t}");
        }
        assert_eq!(OPERATOR_TYPES.len(), 7);
    }

    #[test]
    fn eventhouse_processed_ingestion_valid() {
        let props = json!({
            "dataIngestionMode": "ProcessedIngestion",
            "workspaceId": "ws", "itemId": "kql", "databaseName": "db",
            "tableName": "bikes",
            "inputSerialization": {"type": "Json", "properties": {"encoding": "UTF8"}}
        });
        assert!(validate_eventhouse_destination(&props).is_ok());
    }

    #[test]
    fn eventhouse_direct_ingestion_valid() {
        let props = json!({
            "dataIngestionMode": "DirectIngestion",
            "workspaceId": "ws", "itemId": "kql",
            "connectionName": "conn", "mappingRuleName": "map"
        });
        assert!(validate_eventhouse_destination(&props).is_ok());
    }

    #[test]
    fn eventhouse_missing_mode_teaches() {
        let err = validate_eventhouse_destination(&json!({"workspaceId": "ws"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("dataIngestionMode"), "got: {err}");
    }

    #[test]
    fn eventhouse_invalid_mode_teaches() {
        let err = validate_eventhouse_destination(&json!({"dataIngestionMode": "Streaming"}))
            .unwrap_err()
            .to_string();
        assert!(err.contains("Streaming"), "got: {err}");
    }

    #[test]
    fn eventhouse_direct_ingestion_with_processed_fields_flags_missing_connection() {
        // The exact silent-Warning trap: DirectIngestion mode but supplying the
        // ProcessedIngestion fields (tableName/inputSerialization) and omitting
        // connectionName/mappingRuleName.
        let props = json!({
            "dataIngestionMode": "DirectIngestion",
            "workspaceId": "ws", "itemId": "kql",
            "tableName": "bikes",
            "inputSerialization": {"type": "Json"}
        });
        let err = validate_eventhouse_destination(&props)
            .unwrap_err()
            .to_string();
        assert!(err.contains("connectionName"), "got: {err}");
        assert!(err.contains("mappingRuleName"), "got: {err}");
    }

    #[test]
    fn eventhouse_processed_missing_table_flags_it() {
        let props = json!({
            "dataIngestionMode": "ProcessedIngestion",
            "workspaceId": "ws", "itemId": "kql", "databaseName": "db"
        });
        let err = validate_eventhouse_destination(&props)
            .unwrap_err()
            .to_string();
        assert!(err.contains("tableName"), "got: {err}");
        assert!(err.contains("inputSerialization"), "got: {err}");
    }
}
