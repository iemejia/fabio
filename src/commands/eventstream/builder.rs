//! Eventstream builder helpers: fetch/push definition, add source/destination,
//! add sample source, add derived stream, validate, list components.

use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
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
    validate_source_properties(source_type, &props)?;

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

pub(super) fn validate_source_properties(source_type: &str, props: &Value) -> Result<()> {
    if !props.is_object() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--properties must be a JSON object",
            "Example: --properties '{}'",
        )
        .into());
    }

    if source_type.eq_ignore_ascii_case("ReferenceLakehouse") {
        validate_reference_lakehouse_source(props)?;
    } else if source_type.eq_ignore_ascii_case("FabricCapacityOperationEvents") {
        validate_capacity_operation_source(props)?;
    }
    Ok(())
}

fn required_string<'a>(props: &'a Value, field: &str, source_type: &str) -> Result<&'a str> {
    props.get(field).and_then(Value::as_str).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{source_type} source requires string property '{field}'"),
            format!("Provide '{field}' in --properties."),
        )
        .into()
    })
}

fn validate_reference_lakehouse_source(props: &Value) -> Result<()> {
    let workspace = required_string(props, "workspaceId", "ReferenceLakehouse")?;
    let item = required_string(props, "itemId", "ReferenceLakehouse")?;
    let path = required_string(props, "absoluteOneLakePath", "ReferenceLakehouse")?;
    validate_uuid(workspace, "ReferenceLakehouse workspaceId")?;
    validate_uuid(item, "ReferenceLakehouse itemId")?;

    let url = reqwest::Url::parse(path).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("ReferenceLakehouse absoluteOneLakePath is not a valid URL: {e}"),
            "Use https://onelake.dfs.fabric.microsoft.com/{workspaceId}/{itemId}/Tables/{schema}/{table}",
        )
    })?;
    if url.scheme() != "https" || url.host_str() != Some("onelake.dfs.fabric.microsoft.com") {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "ReferenceLakehouse absoluteOneLakePath must target the OneLake DFS HTTPS endpoint",
            "Use https://onelake.dfs.fabric.microsoft.com/{workspaceId}/{itemId}/Tables/{schema}/{table}",
        )
        .into());
    }
    let segments: Vec<&str> = url.path_segments().into_iter().flatten().collect();
    if segments.len() != 5
        || segments[2] != "Tables"
        || segments.iter().any(|segment| segment.is_empty())
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "ReferenceLakehouse absoluteOneLakePath must identify a Delta table",
            "Expected path: /{workspaceId}/{itemId}/Tables/{schema}/{table}",
        )
        .into());
    }
    if !segments[0].eq_ignore_ascii_case(workspace) || !segments[1].eq_ignore_ascii_case(item) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "ReferenceLakehouse workspaceId and itemId must match the first two absoluteOneLakePath segments",
            "Use the same Lakehouse workspace and item UUIDs in the properties and OneLake URL.",
        )
        .into());
    }

    if let Some(columns) = props.get("referencedColumns")
        && columns
            .as_array()
            .is_none_or(|values| values.iter().any(|value| !value.is_string()))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "ReferenceLakehouse referencedColumns must be an array of strings",
            "Example: \"referencedColumns\":[\"id\",\"name\",\"email\"]",
        )
        .into());
    }
    if let Some(rate) = props.get("refreshRate") {
        let Some(rate) = rate.as_str() else {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "ReferenceLakehouse refreshRate must be a .NET TimeSpan string",
                "Use hh:mm:ss, for example \"00:05:00\"; omit it or use \"00:00:00\" for a static snapshot.",
            )
            .into());
        };
        validate_refresh_rate(rate)?;
    }
    Ok(())
}

fn validate_refresh_rate(rate: &str) -> Result<()> {
    let parts: Vec<&str> = rate.split(':').collect();
    let valid = parts.len() == 3
        && parts[0].len() == 2
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts
            .iter()
            .all(|part| part.chars().all(|character| character.is_ascii_digit()));
    if !valid {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid ReferenceLakehouse refreshRate '{rate}'"),
            "Use hh:mm:ss, for example \"00:05:00\"; omit it or use \"00:00:00\" for a static snapshot.",
        )
        .into());
    }

    let hours = parts[0].parse::<u8>().unwrap_or(u8::MAX);
    let minutes = parts[1].parse::<u8>().unwrap_or(u8::MAX);
    let seconds = parts[2].parse::<u8>().unwrap_or(u8::MAX);
    if hours >= 24 || minutes >= 60 || seconds >= 60 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "ReferenceLakehouse refreshRate must be at least 00:00:00 and less than 24 hours",
            "Use hh:mm:ss with hours 00-23 and minutes/seconds 00-59.",
        )
        .into());
    }
    Ok(())
}

fn validate_capacity_operation_source(props: &Value) -> Result<()> {
    let scope = required_string(props, "eventScope", "FabricCapacityOperationEvents")?;
    let event_scopes = ["Tenant", "Capacity", "Workspace", "Item", "SubItem"];
    if !event_scopes.contains(&scope) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid FabricCapacityOperationEvents eventScope '{scope}'"),
            format!("Valid PascalCase values: {}.", event_scopes.join(", ")),
        )
        .into());
    }
    if let Some(capacity_id) = props.get("capacityId") {
        let capacity_id = capacity_id.as_str().ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "FabricCapacityOperationEvents capacityId must be a UUID string",
                "Provide the capacity UUID as \"capacityId\":\"...\".",
            )
        })?;
        validate_uuid(capacity_id, "FabricCapacityOperationEvents capacityId")?;
    }
    validate_optional_array(props, "includedEventTypes", Value::is_string)?;
    validate_capacity_operation_filters(props)?;
    Ok(())
}

fn validate_capacity_operation_filters(props: &Value) -> Result<()> {
    let Some(filters) = props.get("filters") else {
        return Ok(());
    };
    let filters = filters.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "FabricCapacityOperationEvents filters must be an array of filter objects",
            "Provide 'filters' as a JSON array, for example: [{\"operatorType\":\"StringIn\",\"key\":\"data.operationType\",\"values\":[\"ScaleUp\"]}]",
        )
    })?;

    for (index, filter) in filters.iter().enumerate() {
        let filter_obj = filter.as_object().ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("FabricCapacityOperationEvents filter at index {index} must be an object"),
                "Use an object with 'operatorType', 'key', and either 'value' or 'values'.",
            )
        })?;

        let operator_type = filter_obj
            .get("operatorType")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("FabricCapacityOperationEvents filter at index {index} requires a non-empty 'operatorType'"),
                    "Use a valid filter operator such as 'StringIn', 'Equals', or 'GreaterThan'.",
                )
            })?;

        let key = filter_obj
            .get("key")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("FabricCapacityOperationEvents filter at index {index} requires a non-empty 'key'"),
                    "Provide the event property name in 'key', for example 'data.operationType'.",
                )
            })?;

        let has_value = filter_obj.contains_key("value");
        let has_values = filter_obj.contains_key("values");
        if has_value && has_values {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("FabricCapacityOperationEvents filter '{key}' cannot set both 'value' and 'values'"),
                "Use one discriminator: 'value' for scalar comparisons or 'values' for list-based operators like 'StringIn'.",
            )
            .into());
        }

        let expects_values = operator_type.contains("In") || operator_type.eq_ignore_ascii_case("Between");
        if expects_values && !has_values {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("FabricCapacityOperationEvents filter '{key}' requires 'values' for operator type '{operator_type}'"),
                "Use 'values' for list-based operators such as 'StringIn'.",
            )
            .into());
        }
        if !expects_values && !has_value {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("FabricCapacityOperationEvents filter '{key}' requires 'value' for operator type '{operator_type}'"),
                "Use 'value' for scalar comparisons such as 'Equals' or 'GreaterThan'.",
            )
            .into());
        }

        if let Some(value) = filter_obj.get("value") {
            if value.is_object() || value.is_array() {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("FabricCapacityOperationEvents filter '{key}' 'value' must be a scalar value"),
                    "Set 'value' to a string, number, or boolean, not an object or array.",
                )
                .into());
            }
        }

        if let Some(values) = filter_obj.get("values") {
            let values = values.as_array().ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("FabricCapacityOperationEvents filter '{key}' 'values' must be a non-empty array"),
                    "Use 'values' as an array of scalar values, for example ['ScaleUp', 'ScaleDown'].",
                )
            })?;
            if values.is_empty() || values.iter().any(|entry| entry.is_array() || entry.is_object()) {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("FabricCapacityOperationEvents filter '{key}' 'values' must be a non-empty array of scalar values"),
                    "Use 'values' as an array of strings, numbers, or booleans.",
                )
                .into());
            }
        }
    }
    Ok(())
}

fn validate_optional_array(
    props: &Value,
    field: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Result<()> {
    if let Some(value) = props.get(field)
        && value
            .as_array()
            .is_none_or(|values| values.iter().any(|entry| !predicate(entry)))
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("'{field}' has an invalid value type"),
            format!("Provide '{field}' as a JSON array with the element type documented by the Fabric API."),
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
/// eventstream's `operators` array, wired to `input_nodes`. The `properties` shape
/// is operator-specific (e.g. Filter → `{conditions:[…]}`).
#[allow(clippy::too_many_arguments)]
pub(super) async fn add_operator(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: &str,
    operator_type: &str,
    input_nodes: &[String],
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
    match operator_type {
        "Join" if input_nodes.len() != 2 => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Join requires exactly two --input-node values",
                "Repeat the flag exactly twice, for example: --input-node streaming-input --input-node reference-input",
            )
            .into());
        }
        "Union" if input_nodes.len() < 2 => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Union requires at least two --input-node values",
                "Repeat the flag, for example: --input-node streaming-input --input-node second-stream",
            )
            .into());
        }
        "Filter" | "ManageFields" | "Aggregate" | "GroupBy" | "Expand"
            if input_nodes.len() != 1 =>
        {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{operator_type} requires exactly one --input-node value"),
                "Pass a single --input-node, for example: --input-node streaming-input",
            )
            .into());
        }
        _ => {}
    }

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
        "inputNodes": input_nodes.iter().map(|name| serde_json::json!({"name": name})).collect::<Vec<_>>(),
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
            match src.get("type").and_then(Value::as_str) {
                Some(source_type) => {
                    let empty_properties = serde_json::json!({});
                    let properties = src.get("properties").unwrap_or(&empty_properties);
                    if let Err(error) = validate_source_properties(source_type, properties) {
                        errors.push(error.to_string());
                    }
                }
                None => errors.push(format!(
                    "Source '{}' missing 'type' field.",
                    src.get("name").and_then(Value::as_str).unwrap_or("?")
                )),
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
        {"type": "AmazonKinesis", "category": "source", "description": "Amazon Kinesis Data Streams"},
        {"type": "AmazonMSKKafka", "category": "source", "description": "Amazon Managed Streaming for Apache Kafka"},
        {"type": "ApacheKafka", "category": "source", "description": "Apache Kafka cluster"},
        {"type": "AzureBlobStorageEvents", "category": "source", "description": "Azure Blob Storage events"},
        {"type": "AzureCosmosDBCDC", "category": "source", "description": "Azure Cosmos DB change data capture"},
        {"type": "AzureDataExplorer", "category": "source", "description": "Azure Data Explorer"},
        {"type": "AzureEventGridNamespace", "category": "source", "description": "Azure Event Grid namespace"},
        {"type": "AzureEventHub", "category": "source", "description": "Azure Event Hub"},
        {"type": "AzureEventHubExtended", "category": "source", "description": "Azure Event Hub extended connector"},
        {"type": "AzureIoTHub", "category": "source", "description": "Azure IoT Hub"},
        {"type": "AzureIoTHubExtended", "category": "source", "description": "Azure IoT Hub extended connector"},
        {"type": "AzureSQLDBCDC", "category": "source", "description": "Azure SQL Database change data capture"},
        {"type": "AzureSQLMIDBCDC", "category": "source", "description": "Azure SQL Managed Instance change data capture"},
        {"type": "AzureServiceBus", "category": "source", "description": "Azure Service Bus"},
        {"type": "ConfluentCloud", "category": "source", "description": "Confluent Cloud Kafka"},
        {"type": "Cribl", "category": "source", "description": "Cribl via a Kafka-compatible endpoint"},
        {"type": "CustomEndpoint", "category": "source", "description": "Custom app endpoint (Event Hub-compatible)"},
        {"type": "FabricAnomalyDetectionEvents", "category": "source", "description": "Fabric anomaly detection events"},
        {"type": "FabricCapacityOperationEvents", "category": "source", "description": "Fabric capacity operation started/completed events"},
        {"type": "FabricCapacityOverviewEvents", "category": "source", "description": "Fabric capacity overview events"},
        {"type": "FabricJobEvents", "category": "source", "description": "Fabric job events"},
        {"type": "FabricOneLakeEvents", "category": "source", "description": "Fabric OneLake events"},
        {"type": "FabricWorkspaceItemEvents", "category": "source", "description": "Fabric workspace item events"},
        {"type": "GooglePubSub", "category": "source", "description": "Google Cloud Pub/Sub"},
        {"type": "Http", "category": "source", "description": "HTTP polling endpoint"},
        {"type": "MirroredDatabaseChangeFeed", "category": "source", "description": "Mirrored Database Change Feed"},
        {"type": "MongoDBCDC", "category": "source", "description": "MongoDB change data capture"},
        {"type": "Mqtt", "category": "source", "description": "MQTT broker"},
        {"type": "MySQLCDC", "category": "source", "description": "MySQL Change Data Capture"},
        {"type": "OracleDBCDC", "category": "source", "description": "Oracle DB Change Data Capture"},
        {"type": "PostgreSQLCDC", "category": "source", "description": "PostgreSQL Change Data Capture"},
        {"type": "RealTimeWeather", "category": "source", "description": "Real-time weather data"},
        {"type": "ReferenceLakehouse", "category": "source", "description": "Point-in-time Lakehouse Delta table snapshot for enrichment joins"},
        {"type": "SAPDatasphere", "category": "source", "description": "SAP Datasphere via a Kafka-compatible endpoint"},
        {"type": "SQLServerOnVMDBCDC", "category": "source", "description": "SQL Server on Azure VM change data capture"},
        {"type": "SampleData", "category": "source", "description": "Built-in sample/simulated data"},
        {"type": "SolacePubSub", "category": "source", "description": "Solace PubSub+"},
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
    use super::{
        OPERATOR_TYPES, normalize_sample_type, validate_eventhouse_destination,
        validate_source_properties,
    };
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

    #[test]
    fn reference_lakehouse_properties_accept_spec_example() {
        let props = json!({
            "workspaceId": "cfafbeb1-8037-4d0c-896e-a46fb27ff229",
            "itemId": "11111111-2222-3333-4444-555555555555",
            "absoluteOneLakePath": "https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/11111111-2222-3333-4444-555555555555/Tables/dbo/customers",
            "referencedColumns": ["id", "name", "email"],
            "refreshRate": "00:05:00"
        });
        assert!(validate_source_properties("ReferenceLakehouse", &props).is_ok());
    }

    #[test]
    fn reference_lakehouse_properties_require_matching_onelake_ids() {
        let props = json!({
            "workspaceId": "cfafbeb1-8037-4d0c-896e-a46fb27ff229",
            "itemId": "11111111-2222-3333-4444-555555555555",
            "absoluteOneLakePath": "https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/Tables/dbo/customers"
        });
        let err = validate_source_properties("ReferenceLakehouse", &props)
            .unwrap_err()
            .to_string();
        assert!(err.contains("must match"), "got: {err}");
    }

    #[test]
    fn reference_lakehouse_properties_reject_invalid_refresh_rate() {
        let props = json!({
            "workspaceId": "cfafbeb1-8037-4d0c-896e-a46fb27ff229",
            "itemId": "11111111-2222-3333-4444-555555555555",
            "absoluteOneLakePath": "https://onelake.dfs.fabric.microsoft.com/cfafbeb1-8037-4d0c-896e-a46fb27ff229/11111111-2222-3333-4444-555555555555/Tables/dbo/customers",
            "refreshRate": "24:00:00"
        });
        let err = validate_source_properties("ReferenceLakehouse", &props)
            .unwrap_err()
            .to_string();
        assert!(err.contains("less than 24 hours"), "got: {err}");
    }

    #[test]
    fn capacity_operation_properties_cover_new_fields() {
        let props = json!({
            "eventScope": "Capacity",
            "capacityId": "0b6d8e27-0b7b-4e18-9e5b-2e6b9d9d7e3a",
            "includedEventTypes": [
                "Microsoft.Fabric.Capacity.OperationStarted",
                "Microsoft.Fabric.Capacity.OperationCompleted"
            ],
            "filters": [{
                "operatorType": "StringIn",
                "key": "data.operationType",
                "values": ["ScaleUp", "ScaleDown"]
            }]
        });
        assert!(validate_source_properties("FabricCapacityOperationEvents", &props).is_ok());
    }
}
