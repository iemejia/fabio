//! Scan Fabric tenant workspaces to build a relationship graph.
//!
//! Builds a graph of workspace items (nodes) and their relationships (edges)
//! by inspecting item properties, definitions, and connections.

use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, bail};
use base64::prelude::{BASE64_STANDARD, Engine as _};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;
use crate::verbose;

use super::ContextFormat;

// ── Graph data model ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphNode {
    id: String,
    #[serde(rename = "type")]
    item_type: String,
    name: String,
    workspace_id: String,
    workspace_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    properties: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
struct GraphEdge {
    source: String,
    target: String,
    relationship: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<Value>,
    /// Reliability of this edge: `high` (structured property), `medium` (classified definition),
    /// `low` (generic GUID scan).
    #[serde(skip_serializing_if = "Option::is_none")]
    confidence: Option<String>,
    /// How this edge was discovered (e.g. `structured_property`, `definition_classified`,
    /// `guid_scan_property`, `guid_scan_definition`, `connection_api`).
    #[serde(skip_serializing_if = "Option::is_none")]
    discovery_method: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceInfo {
    id: String,
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    capacity_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GraphSummary {
    total_nodes: usize,
    total_edges: usize,
    workspaces_scanned: usize,
    item_types: BTreeMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    relationship_types: Option<BTreeMap<String, usize>>,
}

/// Scan metadata providing freshness/trust signals for agents.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanMetadata {
    /// ISO 8601 timestamp when the scan started.
    scanned_at: String,
    /// Wall-clock time for the scan in milliseconds.
    scan_duration_ms: u64,
    /// Content fingerprint (SHA-256 of sorted node IDs + names). Allows agents to detect drift.
    etag: String,
    /// Whether the result is partial (e.g. `--focus`, `--item-types` filter, `--summary-only`).
    partial: bool,
    /// Scope description: what was actually scanned.
    scope: ScanScope,
}

/// Describes what the scan covered (for partial result interpretation).
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ScanScope {
    workspaces: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    item_types_filter: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_item: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    focus_depth: Option<usize>,
    deep: bool,
    include_connections: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ContextGraph {
    nodes: Vec<GraphNode>,
    edges: Vec<GraphEdge>,
    workspaces: Vec<WorkspaceInfo>,
    summary: GraphSummary,
    /// Scan metadata (freshness, content hash, scope). Always present for `graph` format.
    #[serde(skip_serializing_if = "Option::is_none")]
    meta: Option<ScanMetadata>,
}

// ── Main extraction logic ───────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools)]
pub(super) struct ExtractParams<'a> {
    pub(super) workspaces: &'a [String],
    pub(super) deep: bool,
    pub(super) include_connections: bool,
    pub(super) item_types_filter: Option<&'a str>,
    pub(super) no_properties: bool,
    pub(super) format: ContextFormat,
    pub(super) merge: Option<&'a std::path::Path>,
    pub(super) output_file: Option<&'a std::path::Path>,
    pub(super) concurrency: usize,
    pub(super) summary_only: bool,
    pub(super) resolve: Option<&'a str>,
    pub(super) focus: Option<&'a str>,
    pub(super) depth: usize,
}

#[allow(clippy::too_many_lines)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    params: &ExtractParams<'_>,
) -> Result<()> {
    if params.workspaces.is_empty() {
        bail!("At least one --workspace is required");
    }

    let start = Instant::now();

    // Parse item type filter
    let type_filter: Option<HashSet<String>> = params
        .item_types_filter
        .map(|s| s.split(',').map(|t| t.trim().to_lowercase()).collect());

    // Dry-run: show what would be scanned
    if output::dry_run_guard(
        cli,
        "context tenant",
        &serde_json::json!({
            "workspaces": params.workspaces,
            "deep": params.deep,
            "includeConnections": params.include_connections,
            "itemTypes": params.item_types_filter,
            "noProperties": params.no_properties,
            "merge": params.merge.map(|p| p.display().to_string()),
            "outputFile": params.output_file.map(|p| p.display().to_string()),
            "concurrency": params.concurrency,
            "summaryOnly": params.summary_only,
            "resolve": params.resolve,
            "focus": params.focus,
            "depth": params.depth,
        }),
    ) {
        return Ok(());
    }

    let semaphore = Arc::new(Semaphore::new(params.concurrency));

    // Phase 1: Resolve workspaces
    let workspace_infos = resolve_workspaces(client, params.workspaces).await?;

    // Phase 2: List and filter items
    let all_items = list_workspace_items(client, &workspace_infos, type_filter.as_ref()).await?;

    // ─── Fast-path: --resolve (name-to-ID lookup) ───────────────────────────
    if let Some(resolve_spec) = params.resolve {
        let resolved = execute_resolve(&all_items, resolve_spec);
        output::render_object(cli, &resolved, "resolved");
        return Ok(());
    }

    // ─── Fast-path: --summary-only (inventory probe) ────────────────────────
    if params.summary_only {
        let summary = execute_summary_only(&workspace_infos, &all_items, start);
        output::render_object(cli, &summary, "summary");
        return Ok(());
    }

    // Phase 3-7: Build graph (nodes + edges)
    let graph = build_graph(
        client,
        &workspace_infos,
        &all_items,
        params.deep,
        params.include_connections,
        params.no_properties,
        &semaphore,
    )
    .await?;

    // Phase 8: Merge with existing graph if --merge specified
    let mut final_graph = if let Some(merge_path) = params.merge {
        let existing = load_graph(merge_path)?;
        merge_graphs(existing, graph)
    } else {
        graph
    };

    // ─── Focus mode: prune graph to ego-centric subgraph ────────────────────
    if let Some(focus_id) = params.focus {
        final_graph = focus_subgraph(&final_graph, focus_id, params.depth);
    }

    // Compute metadata
    let is_partial = params.summary_only
        || params.resolve.is_some()
        || params.focus.is_some()
        || params.item_types_filter.is_some();

    let meta = build_scan_metadata(start, &final_graph, is_partial, params);
    final_graph.meta = Some(meta);

    // Phase 9: Output — format selection
    let (json_value, raw_content) = match params.format {
        ContextFormat::Graph => (serde_json::to_value(&final_graph)?, None),
        ContextFormat::Jsonld => (format_as_jsonld(&final_graph), None),
        ContextFormat::Owl => (format_as_owl_jsonld(&final_graph), None),
        ContextFormat::Rdf => {
            let owl_model = build_owl_model_from_graph(&final_graph);
            let rdf_xml =
                crate::commands::ontology::import::serialize_rdf_xml_from_model(&owl_model);
            (Value::Null, Some(rdf_xml))
        }
        ContextFormat::Full => {
            let rdf_xml = format_as_full_rdf(&final_graph);
            (Value::Null, Some(rdf_xml))
        }
    };

    if let Some(file_path) = params.output_file {
        let content = if let Some(ref raw) = raw_content {
            raw.clone()
        } else if matches!(params.format, ContextFormat::Owl) {
            // OWL format: write bare JSON-LD (no envelope) for direct ontology import
            serde_json::to_string_pretty(&json_value)?
        } else {
            serde_json::to_string_pretty(&serde_json::json!({"data": json_value}))?
        };
        std::fs::write(file_path, content)?;
        let report = serde_json::json!({
            "status": "written",
            "file": file_path.display().to_string(),
            "format": match params.format {
                ContextFormat::Graph => "graph",
                ContextFormat::Jsonld => "jsonld",
                ContextFormat::Owl => "owl",
                ContextFormat::Rdf => "rdf",
                ContextFormat::Full => "full",
            },
            "nodes": final_graph.summary.total_nodes,
            "edges": final_graph.summary.total_edges,
            "workspaces": final_graph.summary.workspaces_scanned,
        });
        output::render_object(cli, &report, "status");
    } else if let Some(raw) = raw_content {
        // RDF/XML format: write raw text to stdout
        print!("{raw}");
    } else {
        output::render_object(cli, &json_value, "summary");
    }
    Ok(())
}

async fn resolve_workspaces(
    client: &FabricClient,
    workspaces: &[String],
) -> Result<Vec<WorkspaceInfo>> {
    verbose::trace_category(
        "context",
        &format!("resolving {} workspace(s)", workspaces.len()),
    );

    // Resolve all workspaces concurrently
    let mut join_set = JoinSet::new();
    for (i, ws) in workspaces.iter().enumerate() {
        let client = client.clone();
        let ws = ws.clone();
        join_set.spawn(async move { (i, resolve_workspace(&client, &ws).await) });
    }

    let mut infos: Vec<Option<WorkspaceInfo>> = vec![None; workspaces.len()];
    while let Some(Ok((i, result))) = join_set.join_next().await {
        infos[i] = Some(result?);
    }

    Ok(infos.into_iter().flatten().collect())
}

async fn list_workspace_items(
    client: &FabricClient,
    workspace_infos: &[WorkspaceInfo],
    type_filter: Option<&HashSet<String>>,
) -> Result<Vec<(Value, String, String)>> {
    verbose::trace_category(
        "context",
        &format!("listing items in {} workspace(s)", workspace_infos.len()),
    );

    // List all workspaces concurrently
    let mut join_set = JoinSet::new();
    for (i, ws) in workspace_infos.iter().enumerate() {
        let client = client.clone();
        let ws_id = ws.id.clone();
        join_set.spawn(async move {
            let resp = client
                .get_list(&format!("/workspaces/{ws_id}/items"), "value", true, None)
                .await;
            (i, resp)
        });
    }

    let mut per_workspace_items: Vec<Vec<Value>> = vec![Vec::new(); workspace_infos.len()];
    while let Some(Ok((i, result))) = join_set.join_next().await {
        per_workspace_items[i] = result?.items;
    }

    // Flatten and filter
    let mut all_items: Vec<(Value, String, String)> = Vec::new();
    for (i, items) in per_workspace_items.into_iter().enumerate() {
        let ws_id = &workspace_infos[i].id;
        let ws_name = &workspace_infos[i].name;
        for item in items {
            if let Some(filter) = type_filter {
                let item_type = item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_lowercase();
                if !filter.contains(&item_type) {
                    continue;
                }
            }
            all_items.push((item, ws_id.clone(), ws_name.clone()));
        }
    }

    verbose::trace_category("context", &format!("found {} items total", all_items.len()));
    Ok(all_items)
}

#[allow(clippy::too_many_lines)]
async fn build_graph(
    client: &FabricClient,
    workspace_infos: &[WorkspaceInfo],
    all_items: &[(Value, String, String)],
    deep: bool,
    include_connections: bool,
    no_properties: bool,
    semaphore: &Arc<Semaphore>,
) -> Result<ContextGraph> {
    // Build known-item ID set for cross-referencing
    let known_ids: HashSet<String> = all_items
        .iter()
        .filter_map(|(item, _, _)| item.get("id").and_then(Value::as_str).map(String::from))
        .collect();

    let known_workspace_ids: HashSet<String> =
        workspace_infos.iter().map(|ws| ws.id.clone()).collect();

    // Fetch item details (properties) concurrently — skip if --no-properties
    let item_details = if no_properties {
        verbose::trace_category("context", "skipping property fetching (--no-properties)");
        all_items.iter().map(|(item, _, _)| item.clone()).collect()
    } else {
        verbose::trace_category(
            "context",
            "fetching item details for property-based relationships",
        );
        fetch_item_details(client, all_items, semaphore).await
    };

    // Build nodes
    let nodes: Vec<GraphNode> = build_nodes(all_items, &item_details);

    // Discover edges from properties
    verbose::trace_category("context", "discovering relationships from item properties");
    let mut edges: HashSet<GraphEdge> = HashSet::new();

    for (i, detail) in item_details.iter().enumerate() {
        extract_property_edges(
            detail,
            &nodes[i].id,
            &nodes[i].item_type,
            &known_ids,
            &known_workspace_ids,
            &mut edges,
        );
    }

    // Deep mode: fetch definitions & GUID-scan
    if deep {
        verbose::trace_category(
            "context",
            "deep mode: fetching definitions for GUID scanning",
        );
        let definition_edges =
            fetch_definitions_and_scan(client, &nodes, &known_ids, &known_workspace_ids, semaphore)
                .await;
        edges.extend(definition_edges);
    }

    // Fetch connections
    if include_connections {
        verbose::trace_category("context", "fetching item connections");
        let connection_edges = fetch_connections(client, &nodes, &known_ids, semaphore).await;
        edges.extend(connection_edges);
    }

    // Build summary
    Ok(assemble_graph(nodes, edges, workspace_infos))
}

fn build_nodes(all_items: &[(Value, String, String)], item_details: &[Value]) -> Vec<GraphNode> {
    all_items
        .iter()
        .enumerate()
        .map(|(i, (item, ws_id, ws_name))| {
            let properties = item_details
                .get(i)
                .and_then(|detail| detail.get("properties").cloned());

            GraphNode {
                id: item
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                item_type: item
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                name: item
                    .get("displayName")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                workspace_id: ws_id.clone(),
                workspace_name: ws_name.clone(),
                description: item
                    .get("description")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(String::from),
                properties,
            }
        })
        .collect()
}

fn assemble_graph(
    nodes: Vec<GraphNode>,
    edges: HashSet<GraphEdge>,
    workspace_infos: &[WorkspaceInfo],
) -> ContextGraph {
    let mut item_type_counts: BTreeMap<String, usize> = BTreeMap::new();
    for node in &nodes {
        *item_type_counts.entry(node.item_type.clone()).or_insert(0) += 1;
    }

    let mut rel_type_counts: BTreeMap<String, usize> = BTreeMap::new();
    for edge in &edges {
        *rel_type_counts
            .entry(edge.relationship.clone())
            .or_insert(0) += 1;
    }

    let edges_vec: Vec<GraphEdge> = edges.into_iter().collect();

    ContextGraph {
        summary: GraphSummary {
            total_nodes: nodes.len(),
            total_edges: edges_vec.len(),
            workspaces_scanned: workspace_infos.len(),
            item_types: item_type_counts,
            relationship_types: if rel_type_counts.is_empty() {
                None
            } else {
                Some(rel_type_counts)
            },
        },
        nodes,
        edges: edges_vec,
        workspaces: workspace_infos.to_vec(),
        meta: None,
    }
}

// ── Workspace resolution ────────────────────────────────────────────────────

async fn resolve_workspace(client: &FabricClient, ws: &str) -> Result<WorkspaceInfo> {
    // GUID detection: 36 chars, hex + dashes, exactly 4 dashes
    if is_guid(ws) {
        let data = client.get(&format!("/workspaces/{ws}")).await?;
        let name = data
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let capacity_id = data
            .get("capacityId")
            .and_then(Value::as_str)
            .map(String::from);
        return Ok(WorkspaceInfo {
            id: ws.to_string(),
            name,
            capacity_id,
        });
    }

    // Name resolution: list workspaces and find by name
    let resp = client.get_list("/workspaces", "value", true, None).await?;
    for item in &resp.items {
        let name = item
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if name.eq_ignore_ascii_case(ws) {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let capacity_id = item
                .get("capacityId")
                .and_then(Value::as_str)
                .map(String::from);
            return Ok(WorkspaceInfo {
                id,
                name: name.to_string(),
                capacity_id,
            });
        }
    }

    bail!("Workspace not found: {ws}. Use `fabio workspace list` to see available workspaces.")
}

fn is_guid(s: &str) -> bool {
    s.len() == 36
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && s.chars().filter(|&c| c == '-').count() == 4
}

// ── Fetch item details concurrently ─────────────────────────────────────────

async fn fetch_item_details(
    client: &FabricClient,
    items: &[(Value, String, String)],
    semaphore: &Arc<Semaphore>,
) -> Vec<Value> {
    let mut results: Vec<Value> = vec![Value::Null; items.len()];
    let mut join_set = JoinSet::new();

    // Collect items that have type-specific endpoints for richer detail
    let detail_requests: Vec<(usize, String, String, String)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, (item, ws_id, _))| {
            let id = item.get("id").and_then(Value::as_str)?;
            let item_type = item.get("type").and_then(Value::as_str)?;
            let endpoint = type_specific_endpoint(item_type)?;
            Some((i, ws_id.clone(), id.to_string(), endpoint.to_string()))
        })
        .collect();

    for (index, ws_id, item_id, endpoint) in detail_requests {
        let client = client.clone();
        let sem = semaphore.clone();
        let path = format!("/workspaces/{ws_id}/{endpoint}/{item_id}");
        join_set.spawn(async move {
            let _permit = sem.acquire().await.unwrap_or_else(|_| unreachable!());
            let result = client.get(&path).await.unwrap_or(Value::Null);
            (index, result)
        });
    }

    while let Some(Ok((index, value))) = join_set.join_next().await {
        results[index] = value;
    }

    // Fill in items without type-specific endpoints using the basic item data
    for (i, (item, _, _)) in items.iter().enumerate() {
        if results[i].is_null() {
            results[i] = item.clone();
        }
    }

    results
}

/// Returns the type-specific REST endpoint segment for item types that expose
/// properties with relationship data in their GET response.
fn type_specific_endpoint(item_type: &str) -> Option<&'static str> {
    match item_type {
        "KQLDatabase" => Some("kqlDatabases"),
        "Lakehouse" => Some("lakehouses"),
        "Warehouse" => Some("warehouses"),
        "SQLDatabase" => Some("sqlDatabases"),
        "SQLEndpoint" => Some("sqlEndpoints"),
        "Eventhouse" => Some("eventhouses"),
        "KQLDashboard" => Some("kqlDashboards"),
        "KQLQueryset" => Some("kqlQuerysets"),
        "SemanticModel" => Some("semanticModels"),
        "Report" => Some("reports"),
        "Notebook" => Some("notebooks"),
        "Eventstream" => Some("eventstreams"),
        "DataPipeline" => Some("dataPipelines"),
        "Dataflow" => Some("dataflows"),
        "GraphQLApi" => Some("graphqlApis"),
        "MirroredDatabase" => Some("mirroredDatabases"),
        "CopyJob" => Some("copyJobs"),
        "SparkJobDefinition" => Some("sparkJobDefinitions"),
        "Environment" => Some("environments"),
        "MirroredAzureDatabricksCatalog" => Some("mirroredAzureDatabricksCatalogs"),
        "GraphModel" => Some("graphModels"),
        _ => None,
    }
}

// ── Extract edges from item properties ──────────────────────────────────────

fn extract_property_edges(
    detail: &Value,
    source_id: &str,
    source_type: &str,
    known_ids: &HashSet<String>,
    _known_workspace_ids: &HashSet<String>,
    edges: &mut HashSet<GraphEdge>,
) {
    let properties = match detail.get("properties") {
        Some(p) if p.is_object() => p,
        _ => return,
    };

    // KQL Database → Eventhouse (parent)
    if source_type == "KQLDatabase"
        && let Some(parent_id) = properties
            .get("parentEventhouseItemId")
            .and_then(Value::as_str)
        && known_ids.contains(parent_id)
    {
        edges.insert(GraphEdge {
            source: source_id.to_string(),
            target: parent_id.to_string(),
            relationship: "child_of".to_string(),
            metadata: Some(serde_json::json!({"parentType": "Eventhouse"})),
            confidence: Some("high".to_string()),
            discovery_method: Some("structured_property".to_string()),
        });
    }

    // Lakehouse → SQL Endpoint (companion relationship via sqlEndpointProperties)
    if source_type == "Lakehouse"
        && let Some(sql_props) = properties.get("sqlEndpointProperties")
        && let Some(ep_id) = sql_props.get("id").and_then(Value::as_str)
        && known_ids.contains(ep_id)
    {
        edges.insert(GraphEdge {
            source: source_id.to_string(),
            target: ep_id.to_string(),
            relationship: "has_endpoint".to_string(),
            metadata: Some(serde_json::json!({"endpointType": "SQLEndpoint"})),
            confidence: Some("high".to_string()),
            discovery_method: Some("structured_property".to_string()),
        });
    }

    // Graph Model → properties.queryReadiness, lastDataLoadingStatus
    // (informational — not a cross-item edge)

    // Generic: scan all property string values for known item IDs
    scan_value_for_ids(properties, source_id, "references", known_ids, edges);
}

/// Recursively scan a JSON value for strings matching known item IDs.
fn scan_value_for_ids(
    value: &Value,
    source_id: &str,
    relationship: &str,
    known_ids: &HashSet<String>,
    edges: &mut HashSet<GraphEdge>,
) {
    match value {
        Value::String(s) => {
            if s.len() == 36 && is_guid(s) && known_ids.contains(s.as_str()) && s != source_id {
                edges.insert(GraphEdge {
                    source: source_id.to_string(),
                    target: s.clone(),
                    relationship: relationship.to_string(),
                    metadata: None,
                    confidence: Some("low".to_string()),
                    discovery_method: Some("guid_scan_property".to_string()),
                });
            }
        }
        Value::Object(map) => {
            for v in map.values() {
                scan_value_for_ids(v, source_id, relationship, known_ids, edges);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                scan_value_for_ids(v, source_id, relationship, known_ids, edges);
            }
        }
        _ => {}
    }
}

// ── Deep mode: definition scanning ──────────────────────────────────────────

/// Item types known to NOT support `getDefinition` — skip to avoid wasted LRO calls.
///
/// NOTE: `PaginatedReport` DOES support `getDefinition` (verified live against the
/// generic `/items/{id}/getDefinition` endpoint) and its `.rdl` embeds a
/// `semanticmodelid=<uuid>` reference, so scanning it recovers report→semantic-model
/// lineage — it is intentionally NOT excluded here.
fn supports_definition(item_type: &str) -> bool {
    !matches!(
        item_type,
        "SQLEndpoint" | "Dashboard" | "Datamart" | "MLModel" | "MLExperiment"
    )
}

async fn fetch_definitions_and_scan(
    client: &FabricClient,
    nodes: &[GraphNode],
    known_ids: &HashSet<String>,
    known_workspace_ids: &HashSet<String>,
    semaphore: &Arc<Semaphore>,
) -> HashSet<GraphEdge> {
    let uuid_re =
        Regex::new(r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")
            .expect("valid regex");

    let mut edges: HashSet<GraphEdge> = HashSet::new();
    let mut join_set = JoinSet::new();

    // All IDs we consider "interesting" references (items + workspaces)
    let all_known: HashSet<String> = known_ids.union(known_workspace_ids).cloned().collect();

    let scannable_count = nodes
        .iter()
        .filter(|n| supports_definition(&n.item_type))
        .count();
    verbose::trace_category(
        "context",
        &format!(
            "scanning {scannable_count}/{} items (skipping types without definitions)",
            nodes.len()
        ),
    );

    for (i, node) in nodes.iter().enumerate() {
        // Skip items that don't support getDefinition
        if !supports_definition(&node.item_type) {
            continue;
        }

        let client = client.clone();
        let sem = semaphore.clone();
        let ws_id = node.workspace_id.clone();
        let item_id = node.id.clone();
        let uuid_re = uuid_re.clone();
        let all_known = all_known.clone();
        let known_items = known_ids.clone();
        let source_id = node.id.clone();

        join_set.spawn(async move {
            let _permit = sem.acquire().await.unwrap_or_else(|_| unreachable!());
            let discovered = scan_item_definition(
                &client,
                &ws_id,
                &item_id,
                &source_id,
                &uuid_re,
                &all_known,
                &known_items,
            )
            .await;
            (i, discovered)
        });
    }

    while let Some(Ok((_index, discovered))) = join_set.join_next().await {
        edges.extend(discovered);
    }

    edges
}

/// Scan a single item's definition for GUID references to known items.
async fn scan_item_definition(
    client: &FabricClient,
    ws_id: &str,
    item_id: &str,
    source_id: &str,
    uuid_re: &Regex,
    all_known: &HashSet<String>,
    known_items: &HashSet<String>,
) -> HashSet<GraphEdge> {
    let mut discovered: HashSet<GraphEdge> = HashSet::new();

    let Ok(def_data) = client
        .post(
            &format!("/workspaces/{ws_id}/items/{item_id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
    else {
        return discovered;
    };

    let parts = def_data
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(Value::as_array)
        .or_else(|| def_data.get("parts").and_then(Value::as_array));

    let Some(parts) = parts else {
        return discovered;
    };

    for part in parts {
        let path = part.get("path").and_then(Value::as_str).unwrap_or_default();
        if path == ".platform" {
            continue;
        }

        let payload = part
            .get("payload")
            .and_then(Value::as_str)
            .unwrap_or_default();

        let Ok(decoded) = BASE64_STANDARD.decode(payload) else {
            continue;
        };
        let Ok(content) = String::from_utf8(decoded) else {
            continue;
        };

        scan_content_for_refs(
            &content,
            path,
            source_id,
            uuid_re,
            all_known,
            known_items,
            &mut discovered,
        );
    }

    discovered
}

/// Scan decoded content for UUID references to known items.
fn scan_content_for_refs(
    content: &str,
    path: &str,
    source_id: &str,
    uuid_re: &Regex,
    all_known: &HashSet<String>,
    known_items: &HashSet<String>,
    discovered: &mut HashSet<GraphEdge>,
) {
    for mat in uuid_re.find_iter(content) {
        let found_id = mat.as_str().to_lowercase();
        if found_id.eq_ignore_ascii_case(source_id) {
            continue;
        }
        if is_well_known_guid(&found_id) {
            continue;
        }
        if !all_known.contains(&found_id) {
            continue;
        }

        let (rel, confidence) = if known_items
            .iter()
            .any(|k| k.eq_ignore_ascii_case(&found_id))
        {
            let r = classify_definition_reference(path, content, &found_id);
            let conf = if r == "definition_ref" {
                "low" // Generic fallback — could be spurious
            } else {
                "medium" // Classified by path/content context
            };
            (r, conf)
        } else {
            ("workspace_ref".to_string(), "medium")
        };

        let target = known_items
            .iter()
            .find(|k| k.eq_ignore_ascii_case(&found_id))
            .cloned()
            .unwrap_or(found_id);

        discovered.insert(GraphEdge {
            source: source_id.to_string(),
            target,
            relationship: rel,
            metadata: Some(serde_json::json!({"discoveredIn": path})),
            confidence: Some(confidence.to_string()),
            discovery_method: Some("guid_scan_definition".to_string()),
        });
    }
}

/// Classify a definition reference based on the file path and content context.
fn classify_definition_reference(path: &str, content: &str, _found_id: &str) -> String {
    let path_lower = path.to_lowercase();

    // Report → Semantic Model
    if path_lower.contains("definition.pbir") || path_lower.contains("pbir") {
        return "bound_to_model".to_string();
    }

    // Notebook → Lakehouse (trident metadata)
    if content.contains("default_lakehouse") || content.contains("trident") {
        return "default_lakehouse".to_string();
    }

    // Eventstream topology references
    if path_lower.contains("eventstream") && content.contains("destinations") {
        return "streams_to".to_string();
    }

    // Semantic model → data source
    if path_lower.contains("model.tmdl") || path_lower.contains(".bim") {
        return "reads_from".to_string();
    }

    // Data pipeline → referenced items
    if path_lower.contains("pipeline") && content.contains("ExecutePipeline") {
        return "executes".to_string();
    }

    // Data agent → data source
    if path_lower.contains("datasource") {
        return "queries".to_string();
    }

    // Ontology → data binding (lakehouse reference)
    if path_lower.contains("databinding") || path_lower.contains("data_binding") {
        return "bound_to_data".to_string();
    }

    // Generic reference
    "definition_ref".to_string()
}

/// Well-known GUIDs that should not be treated as item references.
fn is_well_known_guid(id: &str) -> bool {
    let lower = id.to_lowercase();
    // All zeros
    if lower == "00000000-0000-0000-0000-000000000000" {
        return true;
    }
    // All f's
    if lower == "ffffffff-ffff-ffff-ffff-ffffffffffff" {
        return true;
    }
    // Near-zero (placeholder GUIDs)
    if lower.starts_with("00000000-0000-0000-0000-00000000000") {
        return true;
    }
    false
}

// ── Connection fetching ─────────────────────────────────────────────────────

async fn fetch_connections(
    client: &FabricClient,
    nodes: &[GraphNode],
    _known_ids: &HashSet<String>,
    semaphore: &Arc<Semaphore>,
) -> HashSet<GraphEdge> {
    let mut edges: HashSet<GraphEdge> = HashSet::new();
    let mut join_set = JoinSet::new();

    for node in nodes {
        let client = client.clone();
        let sem = semaphore.clone();
        let ws_id = node.workspace_id.clone();
        let item_id = node.id.clone();
        let source_id = node.id.clone();

        join_set.spawn(async move {
            let _permit = sem.acquire().await.unwrap_or_else(|_| unreachable!());
            let result = client
                .get_list(
                    &format!("/workspaces/{ws_id}/items/{item_id}/connections"),
                    "value",
                    true,
                    None,
                )
                .await;

            let mut discovered: HashSet<GraphEdge> = HashSet::new();
            if let Ok(resp) = result {
                for conn in &resp.items {
                    if let Some(conn_id) = conn.get("id").and_then(Value::as_str) {
                        let conn_name = conn
                            .get("displayName")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        let conn_type = conn
                            .get("connectivityType")
                            .and_then(Value::as_str)
                            .unwrap_or_default();

                        discovered.insert(GraphEdge {
                            source: source_id.clone(),
                            target: conn_id.to_string(),
                            relationship: "connected_via".to_string(),
                            metadata: Some(serde_json::json!({
                                "connectionName": conn_name,
                                "connectivityType": conn_type,
                            })),
                            confidence: Some("high".to_string()),
                            discovery_method: Some("connection_api".to_string()),
                        });
                    }
                }
            }
            discovered
        });
    }

    while let Some(Ok(discovered)) = join_set.join_next().await {
        edges.extend(discovered);
    }

    edges
}

// ── JSON-LD formatting ──────────────────────────────────────────────────────

/// Format the context graph as a JSON-LD document with `@context` and `@graph`.
fn format_as_jsonld(graph: &ContextGraph) -> Value {
    // Build @context vocabulary
    let context = build_jsonld_context();

    // Build @graph: items as nodes with edges inlined as properties
    let mut resources: Vec<Value> = Vec::new();

    // Add workspace resources
    for ws in &graph.workspaces {
        let mut resource = serde_json::json!({
            "@id": format!("urn:fabric:workspace:{}", ws.id),
            "@type": "fabric:Workspace",
            "name": ws.name,
        });
        if let Some(ref cap) = ws.capacity_id {
            resource["fabric:capacityId"] = Value::from(cap.as_str());
        }
        resources.push(resource);
    }

    // Group edges by source for inlining
    let mut edges_by_source: std::collections::HashMap<&str, Vec<&GraphEdge>> =
        std::collections::HashMap::new();
    for edge in &graph.edges {
        edges_by_source
            .entry(edge.source.as_str())
            .or_default()
            .push(edge);
    }

    // Add item resources with inlined edges
    for node in &graph.nodes {
        let mut resource = serde_json::json!({
            "@id": format!("urn:fabric:item:{}", node.id),
            "@type": format!("fabric:{}", node.item_type),
            "name": node.name,
            "fabric:workspace": {"@id": format!("urn:fabric:workspace:{}", node.workspace_id)},
        });
        if let Some(ref desc) = node.description {
            resource["description"] = Value::from(desc.as_str());
        }

        // Inline edges as typed properties
        if let Some(edges) = edges_by_source.get(node.id.as_str()) {
            let mut rel_targets: std::collections::HashMap<&str, Vec<Value>> =
                std::collections::HashMap::new();
            for edge in edges {
                let target_iri = if edge.relationship == "connected_via" {
                    format!("urn:fabric:connection:{}", edge.target)
                } else {
                    format!("urn:fabric:item:{}", edge.target)
                };
                rel_targets
                    .entry(edge.relationship.as_str())
                    .or_default()
                    .push(serde_json::json!({"@id": target_iri}));
            }
            for (rel, targets) in rel_targets {
                let predicate = format!("fabric:{}", relationship_to_camel(rel));
                if targets.len() == 1 {
                    resource[&predicate] = targets.into_iter().next().unwrap_or(Value::Null);
                } else {
                    resource[&predicate] = Value::Array(targets);
                }
            }
        }

        resources.push(resource);
    }

    serde_json::json!({
        "@context": context,
        "@graph": resources
    })
}

/// Format the context graph as OWL JSON-LD — directly importable by `fabio ontology import`.
///
/// Produces `owl:Class` for each unique item type, `owl:DatatypeProperty` for common
/// fields (id, name, workspaceId), and `owl:ObjectProperty` for each unique relationship type.
fn format_as_owl_jsonld(graph: &ContextGraph) -> Value {
    let base = "http://fabric.microsoft.com/ontology/";
    let mut owl_graph: Vec<Value> = Vec::new();

    // Collect unique item types as OWL classes
    let mut types: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for node in &graph.nodes {
        types.insert(&node.item_type);
    }

    for item_type in &types {
        let uri = format!("{base}{item_type}");
        owl_graph.push(serde_json::json!({
            "@id": uri,
            "@type": "owl:Class",
            "rdfs:label": *item_type,
        }));

        // Add standard properties for each class
        owl_graph.push(serde_json::json!({
            "@id": format!("{base}{}_itemId", item_type.to_lowercase()),
            "@type": "owl:DatatypeProperty",
            "rdfs:label": "itemId",
            "rdfs:domain": {"@id": &uri},
            "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
            "ont:isIdentifier": true,
            "ont:propertyType": "string",
        }));
        owl_graph.push(serde_json::json!({
            "@id": format!("{base}{}_name", item_type.to_lowercase()),
            "@type": "owl:DatatypeProperty",
            "rdfs:label": "name",
            "rdfs:domain": {"@id": &uri},
            "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
            "ont:propertyType": "string",
        }));
        owl_graph.push(serde_json::json!({
            "@id": format!("{base}{}_workspaceId", item_type.to_lowercase()),
            "@type": "owl:DatatypeProperty",
            "rdfs:label": "workspaceId",
            "rdfs:domain": {"@id": &uri},
            "rdfs:range": {"@id": "http://www.w3.org/2001/XMLSchema#string"},
            "ont:propertyType": "string",
        }));
    }

    // Collect unique relationship types and their source/target class pairs
    let mut rel_pairs: std::collections::BTreeMap<&str, (&str, &str)> =
        std::collections::BTreeMap::new();
    for edge in &graph.edges {
        if rel_pairs.contains_key(edge.relationship.as_str()) {
            continue;
        }
        // Find source and target node types
        let source_type = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.source)
            .map_or("Unknown", |n| n.item_type.as_str());
        let target_type = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.target)
            .map_or("Unknown", |n| n.item_type.as_str());
        rel_pairs.insert(&edge.relationship, (source_type, target_type));
    }

    for (rel_name, (source_type, target_type)) in &rel_pairs {
        if *rel_name == "workspace" || *rel_name == "workspace_ref" {
            continue; // Skip generic workspace edges
        }
        owl_graph.push(serde_json::json!({
            "@id": format!("{base}{}", relationship_to_camel(rel_name)),
            "@type": "owl:ObjectProperty",
            "rdfs:label": *rel_name,
            "rdfs:domain": {"@id": format!("{base}{source_type}")},
            "rdfs:range": {"@id": format!("{base}{target_type}")},
        }));
    }

    serde_json::json!({
        "@context": {
            "owl": "http://www.w3.org/2002/07/owl#",
            "rdfs": "http://www.w3.org/2000/01/rdf-schema#",
            "xsd": "http://www.w3.org/2001/XMLSchema#",
            "ont": base,
        },
        "@graph": owl_graph
    })
}

/// Build an `OwlModelBuilder` from the context graph for RDF/XML serialization.
fn build_owl_model_from_graph(
    graph: &ContextGraph,
) -> crate::commands::ontology::import::OwlModelBuilder {
    let base = "http://fabric.microsoft.com/ontology/";

    // Unique item types → classes
    let mut types: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for node in &graph.nodes {
        types.insert(&node.item_type);
    }

    let classes: Vec<(String, String)> = types
        .iter()
        .map(|t| (format!("{base}{t}"), (*t).to_string()))
        .collect();

    // Standard properties for each class
    let mut properties: Vec<(String, String, String, bool)> = Vec::new();
    for t in &types {
        let uri = format!("{base}{t}");
        properties.push((
            "itemId".to_string(),
            uri.clone(),
            "String".to_string(),
            true,
        ));
        properties.push(("name".to_string(), uri.clone(), "String".to_string(), false));
        properties.push(("workspaceId".to_string(), uri, "String".to_string(), false));
    }

    // Unique relationships
    let mut rel_pairs: std::collections::BTreeMap<&str, (&str, &str)> =
        std::collections::BTreeMap::new();
    for edge in &graph.edges {
        if rel_pairs.contains_key(edge.relationship.as_str())
            || edge.relationship == "workspace"
            || edge.relationship == "workspace_ref"
        {
            continue;
        }
        let source_type = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.source)
            .map_or("Unknown", |n| n.item_type.as_str());
        let target_type = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.target)
            .map_or("Unknown", |n| n.item_type.as_str());
        rel_pairs.insert(&edge.relationship, (source_type, target_type));
    }

    let relationships: Vec<(String, String, String)> = rel_pairs
        .iter()
        .map(|(rel, (src, tgt))| {
            (
                (*rel).to_string(),
                format!("{base}{src}"),
                format!("{base}{tgt}"),
            )
        })
        .collect();

    crate::commands::ontology::import::OwlModelBuilder {
        classes,
        properties,
        relationships,
    }
}

/// Format as full RDF/XML: OWL schema (classes, properties, relationships) + instance data
/// (individual items as rdf:Description with typed properties and relationship triples).
#[allow(clippy::too_many_lines, clippy::write_with_newline)]
fn format_as_full_rdf(graph: &ContextGraph) -> String {
    use std::fmt::Write;
    let base = "http://fabric.microsoft.com/ontology/";
    // Estimate: ~500 bytes header + ~300 per node + ~200 per edge
    let estimated = 500 + graph.nodes.len() * 300 + graph.edges.len() * 200;
    let mut s = String::with_capacity(estimated);

    s.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<rdf:RDF\n");
    let _ = write!(s, "    xml:base=\"{base}\"\n");
    s.push_str("    xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"\n");
    s.push_str("    xmlns:rdfs=\"http://www.w3.org/2000/01/rdf-schema#\"\n");
    s.push_str("    xmlns:owl=\"http://www.w3.org/2002/07/owl#\"\n");
    s.push_str("    xmlns:xsd=\"http://www.w3.org/2001/XMLSchema#\"\n");
    let _ = write!(s, "    xmlns:ont=\"{base}\"\n");
    let _ = write!(s, "    xmlns:fabric=\"{base}\">\n\n");

    // ── Schema: OWL Classes ──
    s.push_str("    <!-- ====== Schema (OWL Classes) ====== -->\n\n");
    let mut types: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    for node in &graph.nodes {
        types.insert(&node.item_type);
    }
    for t in &types {
        let _ = write!(
            s,
            "    <owl:Class rdf:about=\"{base}{t}\">\n        <rdfs:label>{t}</rdfs:label>\n    </owl:Class>\n\n"
        );
    }

    // ── Schema: Properties ──
    s.push_str("    <!-- ====== Schema (Properties) ====== -->\n\n");
    for t in &types {
        let _ = write!(
            s,
            "    <owl:DatatypeProperty rdf:about=\"{base}{}_itemId\">\n        <rdfs:label>itemId</rdfs:label>\n        <rdfs:domain rdf:resource=\"{base}{t}\"/>\n        <rdfs:range rdf:resource=\"http://www.w3.org/2001/XMLSchema#string\"/>\n        <ont:isIdentifier rdf:datatype=\"http://www.w3.org/2001/XMLSchema#boolean\">true</ont:isIdentifier>\n        <ont:propertyType>string</ont:propertyType>\n    </owl:DatatypeProperty>\n\n",
            t.to_lowercase()
        );
        let _ = write!(
            s,
            "    <owl:DatatypeProperty rdf:about=\"{base}{}_name\">\n        <rdfs:label>name</rdfs:label>\n        <rdfs:domain rdf:resource=\"{base}{t}\"/>\n        <rdfs:range rdf:resource=\"http://www.w3.org/2001/XMLSchema#string\"/>\n        <ont:propertyType>string</ont:propertyType>\n    </owl:DatatypeProperty>\n\n",
            t.to_lowercase()
        );
    }

    // ── Schema: Relationships ──
    s.push_str("    <!-- ====== Schema (Relationships) ====== -->\n\n");
    let mut rel_pairs: std::collections::BTreeMap<&str, (&str, &str)> =
        std::collections::BTreeMap::new();
    for edge in &graph.edges {
        if rel_pairs.contains_key(edge.relationship.as_str())
            || edge.relationship == "workspace"
            || edge.relationship == "workspace_ref"
        {
            continue;
        }
        let src = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.source)
            .map_or("Unknown", |n| n.item_type.as_str());
        let tgt = graph
            .nodes
            .iter()
            .find(|n| n.id == edge.target)
            .map_or("Unknown", |n| n.item_type.as_str());
        rel_pairs.insert(&edge.relationship, (src, tgt));
    }
    for (rel, (src, tgt)) in &rel_pairs {
        let camel = relationship_to_camel(rel);
        let _ = write!(
            s,
            "    <owl:ObjectProperty rdf:about=\"{base}{camel}\">\n        <rdfs:label>{rel}</rdfs:label>\n        <rdfs:domain rdf:resource=\"{base}{src}\"/>\n        <rdfs:range rdf:resource=\"{base}{tgt}\"/>\n    </owl:ObjectProperty>\n\n"
        );
    }

    // ── Instances: Items ──
    s.push_str("    <!-- ====== Instances ====== -->\n\n");
    for node in &graph.nodes {
        let _ = write!(
            s,
            "    <rdf:Description rdf:about=\"{base}item/{}\">",
            node.id
        );
        let _ = write!(
            s,
            "\n        <rdf:type rdf:resource=\"{base}{}\"/>",
            node.item_type
        );
        let _ = write!(
            s,
            "\n        <rdfs:label>{}</rdfs:label>",
            xml_escape(&node.name)
        );
        let _ = write!(s, "\n        <fabric:itemId>{}</fabric:itemId>", node.id);
        let _ = write!(
            s,
            "\n        <fabric:workspaceId>{}</fabric:workspaceId>",
            node.workspace_id
        );
        s.push_str("\n    </rdf:Description>\n\n");
    }

    // ── Instances: Relationship triples ──
    s.push_str("    <!-- ====== Instance Relationships ====== -->\n\n");
    for edge in &graph.edges {
        if edge.relationship == "workspace" || edge.relationship == "workspace_ref" {
            continue;
        }
        let camel = relationship_to_camel(&edge.relationship);
        let _ = write!(
            s,
            "    <rdf:Description rdf:about=\"{base}item/{}\">",
            edge.source
        );
        let _ = write!(
            s,
            "\n        <fabric:{camel} rdf:resource=\"{base}item/{}\"/>",
            edge.target
        );
        s.push_str("\n    </rdf:Description>\n\n");
    }

    s.push_str("</rdf:RDF>\n");
    s
}

/// Escape XML special characters.
fn xml_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build the JSON-LD `@context` mapping.
fn build_jsonld_context() -> Value {
    serde_json::json!({
        "fabric": "https://api.fabric.microsoft.com/ontology/",
        "xsd": "http://www.w3.org/2001/XMLSchema#",
        "name": "fabric:name",
        "description": "fabric:description",
        "fabric:workspace": {"@type": "@id"},
        "fabric:capacityId": "fabric:capacityId",
        "fabric:defaultLakehouse": {"@type": "@id"},
        "fabric:boundToModel": {"@type": "@id"},
        "fabric:childOf": {"@type": "@id"},
        "fabric:hasEndpoint": {"@type": "@id"},
        "fabric:readsFrom": {"@type": "@id"},
        "fabric:streamsTo": {"@type": "@id"},
        "fabric:queries": {"@type": "@id"},
        "fabric:executes": {"@type": "@id"},
        "fabric:connectedVia": {"@type": "@id"},
        "fabric:references": {"@type": "@id"},
        "fabric:definitionRef": {"@type": "@id"},
        "fabric:workspaceRef": {"@type": "@id"},
        "fabric:boundToData": {"@type": "@id"}
    })
}

/// Convert `snake_case` relationship names to `camelCase` for JSON-LD predicates.
fn relationship_to_camel(rel: &str) -> String {
    let mut result = String::with_capacity(rel.len());
    let mut capitalize_next = false;
    for ch in rel.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

// ── Graph persistence and merging ───────────────────────────────────────────

/// Load an existing graph from a JSON file (expects `{"data": {...}}` envelope).
fn load_graph(path: &std::path::Path) -> Result<ContextGraph> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read graph file {}: {e}", path.display()))?;
    let envelope: Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Failed to parse graph file as JSON: {e}"))?;

    // Support both {"data": {...}} envelope and bare graph object
    let graph_value = envelope.get("data").unwrap_or(&envelope);

    let graph: ContextGraph = serde_json::from_value(graph_value.clone())
        .map_err(|e| anyhow::anyhow!("Failed to parse graph structure: {e}"))?;

    verbose::trace_category(
        "context",
        &format!(
            "loaded existing graph: {} nodes, {} edges, {} workspaces",
            graph.summary.total_nodes, graph.summary.total_edges, graph.summary.workspaces_scanned
        ),
    );

    Ok(graph)
}

/// Merge two graphs: union of nodes (by ID), union of edges, union of workspaces.
fn merge_graphs(mut existing: ContextGraph, new: ContextGraph) -> ContextGraph {
    // Merge nodes (deduplicate by ID, new nodes overwrite existing for same ID)
    let mut node_map: std::collections::HashMap<String, GraphNode> = existing
        .nodes
        .drain(..)
        .map(|n| (n.id.clone(), n))
        .collect();
    for node in new.nodes {
        node_map.insert(node.id.clone(), node);
    }
    let nodes: Vec<GraphNode> = node_map.into_values().collect();

    // Merge edges (deduplicate by full equality)
    let mut edge_set: HashSet<GraphEdge> = existing.edges.into_iter().collect();
    edge_set.extend(new.edges);

    // Merge workspaces (deduplicate by ID)
    let mut ws_map: std::collections::HashMap<String, WorkspaceInfo> = existing
        .workspaces
        .into_iter()
        .map(|ws| (ws.id.clone(), ws))
        .collect();
    for ws in new.workspaces {
        ws_map.insert(ws.id.clone(), ws);
    }
    let workspaces: Vec<WorkspaceInfo> = ws_map.into_values().collect();

    // Rebuild summary
    assemble_graph(nodes, edge_set, &workspaces)
}

// ── Fast-path: --resolve (name → ID lookup) ─────────────────────────────────

/// Resolve `Type:Name` pairs to item IDs from the already-listed items.
fn execute_resolve(all_items: &[(Value, String, String)], resolve_spec: &str) -> Value {
    let mut results: Vec<Value> = Vec::new();

    for pair in resolve_spec.split(',') {
        let pair = pair.trim();
        let Some((type_filter, name_filter)) = pair.split_once(':') else {
            results.push(serde_json::json!({
                "query": pair,
                "error": "Expected format Type:Name"
            }));
            continue;
        };
        let (type_filter, name_filter) = (type_filter.trim(), name_filter.trim());

        let mut found = false;
        for (item, ws_id, ws_name) in all_items {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
            let item_name = item
                .get("displayName")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let item_id = item.get("id").and_then(Value::as_str).unwrap_or_default();

            if item_type.eq_ignore_ascii_case(type_filter)
                && item_name.eq_ignore_ascii_case(name_filter)
            {
                results.push(serde_json::json!({
                    "type": item_type,
                    "name": item_name,
                    "id": item_id,
                    "workspaceId": ws_id,
                    "workspaceName": ws_name,
                }));
                found = true;
                break;
            }
        }

        if !found {
            results.push(serde_json::json!({
                "query": pair,
                "error": "Not found"
            }));
        }
    }

    serde_json::json!({ "resolved": results, "count": results.len() })
}

// ── Fast-path: --summary-only (inventory probe) ─────────────────────────────

/// Return a lightweight inventory without building the full graph.
fn execute_summary_only(
    workspace_infos: &[WorkspaceInfo],
    all_items: &[(Value, String, String)],
    start: Instant,
) -> Value {
    let mut item_types: BTreeMap<String, usize> = BTreeMap::new();
    for (item, _, _) in all_items {
        let t = item
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("Unknown")
            .to_string();
        *item_types.entry(t).or_insert(0) += 1;
    }

    let ws_list: Vec<Value> = workspace_infos
        .iter()
        .map(|ws| {
            serde_json::json!({
                "id": ws.id,
                "name": ws.name,
                "capacityId": ws.capacity_id,
            })
        })
        .collect();

    let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    serde_json::json!({
        "workspaces": ws_list,
        "totalItems": all_items.len(),
        "itemTypes": item_types,
        "scanDurationMs": elapsed_ms,
        "hint": "Use --deep and --include-connections for relationship discovery. Use --focus <id> to get a targeted subgraph."
    })
}

// ── Focus mode: ego-centric subgraph extraction ─────────────────────────────

/// Extract the subgraph within `max_depth` hops of `focus_id`.
fn focus_subgraph(graph: &ContextGraph, focus_id: &str, max_depth: usize) -> ContextGraph {
    // BFS from focus_id over edges (treated as undirected for reachability)
    let mut visited: HashSet<&str> = HashSet::new();
    let mut frontier: Vec<&str> = Vec::new();

    // Check if focus_id exists in graph
    if !graph.nodes.iter().any(|n| n.id == focus_id) {
        // Return empty graph with a note — the focus item was not found
        return ContextGraph {
            nodes: Vec::new(),
            edges: Vec::new(),
            workspaces: graph.workspaces.clone(),
            summary: GraphSummary {
                total_nodes: 0,
                total_edges: 0,
                workspaces_scanned: graph.workspaces.len(),
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };
    }

    visited.insert(focus_id);
    frontier.push(focus_id);

    for _depth in 0..max_depth {
        let mut next_frontier: Vec<&str> = Vec::new();
        for &node_id in &frontier {
            // Outgoing edges
            for edge in &graph.edges {
                if edge.source == node_id && !visited.contains(edge.target.as_str()) {
                    visited.insert(&edge.target);
                    next_frontier.push(&edge.target);
                }
                if edge.target == node_id && !visited.contains(edge.source.as_str()) {
                    visited.insert(&edge.source);
                    next_frontier.push(&edge.source);
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    // Filter nodes to only visited set
    let nodes: Vec<GraphNode> = graph
        .nodes
        .iter()
        .filter(|n| visited.contains(n.id.as_str()))
        .cloned()
        .collect();

    // Filter edges to only those between visited nodes
    let edges: HashSet<GraphEdge> = graph
        .edges
        .iter()
        .filter(|e| visited.contains(e.source.as_str()) && visited.contains(e.target.as_str()))
        .cloned()
        .collect();

    assemble_graph(nodes, edges, &graph.workspaces)
}

// ── Metadata builder ────────────────────────────────────────────────────────

/// Compute scan metadata (timestamp, duration, content hash, scope).
fn build_scan_metadata(
    start: Instant,
    graph: &ContextGraph,
    partial: bool,
    params: &ExtractParams<'_>,
) -> ScanMetadata {
    let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

    // Content fingerprint: SHA-256 of sorted (id, name) pairs
    let mut id_name_pairs: Vec<(&str, &str)> = graph
        .nodes
        .iter()
        .map(|n| (n.id.as_str(), n.name.as_str()))
        .collect();
    id_name_pairs.sort_unstable();

    let mut hasher = Sha256::new();
    for (id, name) in &id_name_pairs {
        hasher.update(id.as_bytes());
        hasher.update(b":");
        hasher.update(name.as_bytes());
        hasher.update(b"\n");
    }
    let hash = hasher.finalize();
    let mut hex_str = String::with_capacity(64);
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(hex_str, "{byte:02x}");
    }
    let etag = format!("sha256:{hex_str}");

    let scanned_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    ScanMetadata {
        scanned_at,
        scan_duration_ms: elapsed_ms,
        etag,
        partial,
        scope: ScanScope {
            workspaces: params.workspaces.to_vec(),
            item_types_filter: params.item_types_filter.map(|s| {
                s.split(',')
                    .map(|t| t.trim().to_string())
                    .collect::<Vec<_>>()
            }),
            focus_item: params.focus.map(String::from),
            focus_depth: params.focus.map(|_| params.depth),
            deep: params.deep,
            include_connections: params.include_connections,
        },
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn test_is_guid() {
        assert!(is_guid("12345678-1234-1234-1234-123456789abc"));
        assert!(is_guid("ABCDEF01-2345-6789-abcd-ef0123456789"));
        assert!(!is_guid("not-a-guid"));
        assert!(!is_guid("12345678-1234-1234-1234-123456789ab")); // too short
        assert!(!is_guid("12345678-1234-1234-1234-123456789abcd")); // too long
        assert!(!is_guid("1234567g-1234-1234-1234-123456789abc")); // invalid char
    }

    #[test]
    fn test_supports_definition() {
        // PaginatedReport DOES support getDefinition (verified live) and its RDL
        // embeds a semantic-model UUID, so it must be scanned for lineage.
        assert!(supports_definition("PaginatedReport"));
        // Definition-bearing types are scanned.
        assert!(supports_definition("Notebook"));
        assert!(supports_definition("Report"));
        assert!(supports_definition("SemanticModel"));
        // Types that genuinely lack getDefinition stay excluded.
        assert!(!supports_definition("SQLEndpoint"));
        assert!(!supports_definition("Dashboard"));
        assert!(!supports_definition("Datamart"));
        assert!(!supports_definition("MLModel"));
        assert!(!supports_definition("MLExperiment"));
    }

    #[test]
    fn test_is_well_known_guid() {
        assert!(is_well_known_guid("00000000-0000-0000-0000-000000000000"));
        assert!(is_well_known_guid("00000000-0000-0000-0000-000000000001"));
        assert!(is_well_known_guid("ffffffff-ffff-ffff-ffff-ffffffffffff"));
        assert!(!is_well_known_guid("12345678-1234-1234-1234-123456789abc"));
    }

    #[test]
    fn test_scan_value_for_ids() {
        let known_ids: HashSet<String> = [
            "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee".to_string(),
            "11111111-2222-3333-4444-555555555555".to_string(),
        ]
        .into();

        let value = serde_json::json!({
            "parentId": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "nested": {
                "ref": "11111111-2222-3333-4444-555555555555"
            },
            "unrelated": "not-a-guid"
        });

        let source_id = "source00-0000-0000-0000-000000000000";
        let mut edges: HashSet<GraphEdge> = HashSet::new();
        scan_value_for_ids(&value, source_id, "references", &known_ids, &mut edges);

        assert_eq!(edges.len(), 2);
        let targets: BTreeSet<String> = edges.iter().map(|e| e.target.clone()).collect();
        assert!(targets.contains("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"));
        assert!(targets.contains("11111111-2222-3333-4444-555555555555"));
    }

    #[test]
    fn test_scan_value_excludes_self() {
        let source_id = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let known_ids: HashSet<String> = [source_id.to_string()].into();

        let value = serde_json::json!({"id": source_id});
        let mut edges: HashSet<GraphEdge> = HashSet::new();
        scan_value_for_ids(&value, source_id, "references", &known_ids, &mut edges);

        assert_eq!(edges.len(), 0);
    }

    #[test]
    fn test_classify_definition_reference() {
        assert_eq!(
            classify_definition_reference("definition.pbir", "{}", "id"),
            "bound_to_model"
        );
        assert_eq!(
            classify_definition_reference(
                "notebook-content.py",
                r#"{"trident":{"lakehouse":{"default_lakehouse":"x"}}}"#,
                "id"
            ),
            "default_lakehouse"
        );
        assert_eq!(
            classify_definition_reference("pipeline.json", r#"{"ExecutePipeline": {}}"#, "id"),
            "executes"
        );
        assert_eq!(
            classify_definition_reference("model.tmdl", "expression stuff", "id"),
            "reads_from"
        );
        assert_eq!(
            classify_definition_reference("some-file.json", "random content", "id"),
            "definition_ref"
        );
    }

    #[test]
    fn test_type_specific_endpoint() {
        assert_eq!(type_specific_endpoint("KQLDatabase"), Some("kqlDatabases"));
        assert_eq!(type_specific_endpoint("Lakehouse"), Some("lakehouses"));
        assert_eq!(type_specific_endpoint("UnknownType"), None);
    }

    #[test]
    fn test_graph_serialization() {
        let graph = ContextGraph {
            nodes: vec![GraphNode {
                id: "aaa".to_string(),
                item_type: "Notebook".to_string(),
                name: "MyNB".to_string(),
                workspace_id: "ws1".to_string(),
                workspace_name: "TestWS".to_string(),
                description: None,
                properties: None,
            }],
            edges: vec![GraphEdge {
                source: "aaa".to_string(),
                target: "bbb".to_string(),
                relationship: "default_lakehouse".to_string(),
                metadata: None,
                confidence: None,
                discovery_method: None,
            }],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "TestWS".to_string(),
                capacity_id: Some("cap1".to_string()),
            }],
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 1,
                workspaces_scanned: 1,
                item_types: BTreeMap::from([("Notebook".to_string(), 1)]),
                relationship_types: Some(BTreeMap::from([("default_lakehouse".to_string(), 1)])),
            },
            meta: None,
        };

        let json = serde_json::to_value(&graph).unwrap();
        assert_eq!(json["summary"]["totalNodes"], 1);
        assert_eq!(json["summary"]["totalEdges"], 1);
        assert_eq!(json["nodes"][0]["name"], "MyNB");
        assert_eq!(json["edges"][0]["relationship"], "default_lakehouse");
        assert_eq!(json["workspaces"][0]["capacityId"], "cap1");
    }

    #[test]
    fn test_format_as_jsonld() {
        let graph = ContextGraph {
            nodes: vec![GraphNode {
                id: "aaa-bbb-ccc".to_string(),
                item_type: "Notebook".to_string(),
                name: "MyNB".to_string(),
                workspace_id: "ws1".to_string(),
                workspace_name: "TestWS".to_string(),
                description: Some("A notebook".to_string()),
                properties: None,
            }],
            edges: vec![GraphEdge {
                source: "aaa-bbb-ccc".to_string(),
                target: "ddd-eee-fff".to_string(),
                relationship: "default_lakehouse".to_string(),
                metadata: None,
                confidence: None,
                discovery_method: None,
            }],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "TestWS".to_string(),
                capacity_id: Some("cap1".to_string()),
            }],
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 1,
                workspaces_scanned: 1,
                item_types: BTreeMap::from([("Notebook".to_string(), 1)]),
                relationship_types: Some(BTreeMap::from([("default_lakehouse".to_string(), 1)])),
            },
            meta: None,
        };

        let jsonld = format_as_jsonld(&graph);

        // Has @context and @graph
        assert!(jsonld.get("@context").is_some());
        assert!(jsonld.get("@graph").is_some());

        let graph_arr = jsonld["@graph"].as_array().unwrap();
        // Workspace + 1 item = 2 resources
        assert_eq!(graph_arr.len(), 2);

        // Find the notebook node
        let nb = graph_arr
            .iter()
            .find(|r| r["@id"] == "urn:fabric:item:aaa-bbb-ccc")
            .expect("notebook not found");
        assert_eq!(nb["@type"], "fabric:Notebook");
        assert_eq!(nb["name"], "MyNB");
        assert_eq!(nb["description"], "A notebook");
        // Edge is inlined as property
        assert_eq!(
            nb["fabric:defaultLakehouse"]["@id"],
            "urn:fabric:item:ddd-eee-fff"
        );

        // Find workspace
        let ws = graph_arr
            .iter()
            .find(|r| r["@id"] == "urn:fabric:workspace:ws1")
            .expect("workspace not found");
        assert_eq!(ws["@type"], "fabric:Workspace");
        assert_eq!(ws["name"], "TestWS");
    }

    #[test]
    fn test_relationship_to_camel() {
        assert_eq!(
            relationship_to_camel("default_lakehouse"),
            "defaultLakehouse"
        );
        assert_eq!(relationship_to_camel("bound_to_model"), "boundToModel");
        assert_eq!(relationship_to_camel("has_endpoint"), "hasEndpoint");
        assert_eq!(relationship_to_camel("references"), "references");
        assert_eq!(relationship_to_camel("connected_via"), "connectedVia");
    }

    #[test]
    fn test_merge_graphs_nodes_dedup_by_id() {
        let existing = ContextGraph {
            nodes: vec![
                GraphNode {
                    id: "aaa".to_string(),
                    item_type: "Notebook".to_string(),
                    name: "OldName".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS1".to_string(),
                    description: None,
                    properties: None,
                },
                GraphNode {
                    id: "bbb".to_string(),
                    item_type: "Lakehouse".to_string(),
                    name: "LH".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS1".to_string(),
                    description: None,
                    properties: None,
                },
            ],
            edges: vec![],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "WS1".to_string(),
                capacity_id: None,
            }],
            summary: GraphSummary {
                total_nodes: 2,
                total_edges: 0,
                workspaces_scanned: 1,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        let new = ContextGraph {
            nodes: vec![
                GraphNode {
                    id: "aaa".to_string(),
                    item_type: "Notebook".to_string(),
                    name: "NewName".to_string(), // Updated name
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS1".to_string(),
                    description: Some("updated".to_string()),
                    properties: None,
                },
                GraphNode {
                    id: "ccc".to_string(),
                    item_type: "Report".to_string(),
                    name: "NewReport".to_string(),
                    workspace_id: "ws2".to_string(),
                    workspace_name: "WS2".to_string(),
                    description: None,
                    properties: None,
                },
            ],
            edges: vec![],
            workspaces: vec![WorkspaceInfo {
                id: "ws2".to_string(),
                name: "WS2".to_string(),
                capacity_id: None,
            }],
            summary: GraphSummary {
                total_nodes: 2,
                total_edges: 0,
                workspaces_scanned: 1,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        let merged = merge_graphs(existing, new);

        // Should have 3 unique nodes (aaa overwritten with new name, bbb kept, ccc added)
        assert_eq!(merged.summary.total_nodes, 3);
        let aaa = merged.nodes.iter().find(|n| n.id == "aaa").unwrap();
        assert_eq!(aaa.name, "NewName");
        assert_eq!(aaa.description, Some("updated".to_string()));
        assert!(merged.nodes.iter().any(|n| n.id == "bbb"));
        assert!(merged.nodes.iter().any(|n| n.id == "ccc"));

        // Should have 2 workspaces
        assert_eq!(merged.summary.workspaces_scanned, 2);
    }

    #[test]
    fn test_merge_graphs_edges_union() {
        let existing = ContextGraph {
            nodes: vec![],
            edges: vec![
                GraphEdge {
                    source: "a".to_string(),
                    target: "b".to_string(),
                    relationship: "ref".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
                GraphEdge {
                    source: "a".to_string(),
                    target: "c".to_string(),
                    relationship: "ref".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
            ],
            workspaces: vec![],
            summary: GraphSummary {
                total_nodes: 0,
                total_edges: 2,
                workspaces_scanned: 0,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        let new = ContextGraph {
            nodes: vec![],
            edges: vec![
                GraphEdge {
                    source: "a".to_string(),
                    target: "b".to_string(),
                    relationship: "ref".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                }, // duplicate
                GraphEdge {
                    source: "x".to_string(),
                    target: "y".to_string(),
                    relationship: "new_rel".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
            ],
            workspaces: vec![],
            summary: GraphSummary {
                total_nodes: 0,
                total_edges: 2,
                workspaces_scanned: 0,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        let merged = merge_graphs(existing, new);
        // 2 from existing + 1 new (1 duplicate removed)
        assert_eq!(merged.summary.total_edges, 3);
    }

    #[test]
    fn test_merge_graphs_idempotent() {
        let graph = ContextGraph {
            nodes: vec![GraphNode {
                id: "aaa".to_string(),
                item_type: "Notebook".to_string(),
                name: "NB".to_string(),
                workspace_id: "ws1".to_string(),
                workspace_name: "WS1".to_string(),
                description: None,
                properties: None,
            }],
            edges: vec![GraphEdge {
                source: "aaa".to_string(),
                target: "bbb".to_string(),
                relationship: "ref".to_string(),
                metadata: None,
                confidence: None,
                discovery_method: None,
            }],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "WS1".to_string(),
                capacity_id: None,
            }],
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 1,
                workspaces_scanned: 1,
                item_types: BTreeMap::from([("Notebook".to_string(), 1)]),
                relationship_types: Some(BTreeMap::from([("ref".to_string(), 1)])),
            },
            meta: None,
        };

        let clone = ContextGraph {
            nodes: graph.nodes.clone(),
            edges: graph.edges.clone(),
            workspaces: graph.workspaces.clone(),
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 1,
                workspaces_scanned: 1,
                item_types: BTreeMap::from([("Notebook".to_string(), 1)]),
                relationship_types: Some(BTreeMap::from([("ref".to_string(), 1)])),
            },
            meta: None,
        };

        let merged = merge_graphs(graph, clone);
        // Merging with itself should produce same counts
        assert_eq!(merged.summary.total_nodes, 1);
        assert_eq!(merged.summary.total_edges, 1);
        assert_eq!(merged.summary.workspaces_scanned, 1);
    }

    #[test]
    fn test_jsonld_multiple_edges_same_type() {
        let graph = ContextGraph {
            nodes: vec![GraphNode {
                id: "nb1".to_string(),
                item_type: "Notebook".to_string(),
                name: "Multi".to_string(),
                workspace_id: "ws1".to_string(),
                workspace_name: "WS".to_string(),
                description: None,
                properties: None,
            }],
            edges: vec![
                GraphEdge {
                    source: "nb1".to_string(),
                    target: "lh1".to_string(),
                    relationship: "references".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
                GraphEdge {
                    source: "nb1".to_string(),
                    target: "lh2".to_string(),
                    relationship: "references".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
            ],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "WS".to_string(),
                capacity_id: None,
            }],
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 2,
                workspaces_scanned: 1,
                item_types: BTreeMap::from([("Notebook".to_string(), 1)]),
                relationship_types: Some(BTreeMap::from([("references".to_string(), 2)])),
            },
            meta: None,
        };

        let jsonld = format_as_jsonld(&graph);
        let graph_arr = jsonld["@graph"].as_array().unwrap();
        let nb = graph_arr
            .iter()
            .find(|r| r["@id"] == "urn:fabric:item:nb1")
            .unwrap();

        // Multiple edges of same type should be an array
        let refs = &nb["fabric:references"];
        assert!(refs.is_array(), "expected array for multiple edges");
        assert_eq!(refs.as_array().unwrap().len(), 2);
    }

    #[test]
    fn test_execute_resolve_found() {
        let items: Vec<(Value, String, String)> = vec![
            (
                serde_json::json!({"id": "id-1", "type": "Notebook", "displayName": "my-nb"}),
                "ws-1".to_string(),
                "TestWS".to_string(),
            ),
            (
                serde_json::json!({"id": "id-2", "type": "Lakehouse", "displayName": "bronze"}),
                "ws-1".to_string(),
                "TestWS".to_string(),
            ),
        ];

        let result = execute_resolve(&items, "Notebook:my-nb,Lakehouse:bronze");
        let resolved = result["resolved"].as_array().unwrap();
        assert_eq!(resolved.len(), 2);
        assert_eq!(resolved[0]["id"], "id-1");
        assert_eq!(resolved[0]["type"], "Notebook");
        assert_eq!(resolved[1]["id"], "id-2");
        assert_eq!(resolved[1]["name"], "bronze");
    }

    #[test]
    fn test_execute_resolve_not_found() {
        let items: Vec<(Value, String, String)> = vec![(
            serde_json::json!({"id": "id-1", "type": "Notebook", "displayName": "my-nb"}),
            "ws-1".to_string(),
            "TestWS".to_string(),
        )];

        let result = execute_resolve(&items, "Lakehouse:missing");
        let resolved = result["resolved"].as_array().unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0]["error"], "Not found");
    }

    #[test]
    fn test_execute_resolve_invalid_format() {
        let items: Vec<(Value, String, String)> = vec![];
        let result = execute_resolve(&items, "no-colon-here");
        let resolved = result["resolved"].as_array().unwrap();
        assert_eq!(resolved[0]["error"], "Expected format Type:Name");
    }

    #[test]
    fn test_focus_subgraph_basic() {
        let graph = ContextGraph {
            nodes: vec![
                GraphNode {
                    id: "a".to_string(),
                    item_type: "Notebook".to_string(),
                    name: "NB-A".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS".to_string(),
                    description: None,
                    properties: None,
                },
                GraphNode {
                    id: "b".to_string(),
                    item_type: "Lakehouse".to_string(),
                    name: "LH-B".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS".to_string(),
                    description: None,
                    properties: None,
                },
                GraphNode {
                    id: "c".to_string(),
                    item_type: "Report".to_string(),
                    name: "Report-C".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS".to_string(),
                    description: None,
                    properties: None,
                },
                GraphNode {
                    id: "d".to_string(),
                    item_type: "SemanticModel".to_string(),
                    name: "SM-D".to_string(),
                    workspace_id: "ws1".to_string(),
                    workspace_name: "WS".to_string(),
                    description: None,
                    properties: None,
                },
            ],
            edges: vec![
                GraphEdge {
                    source: "a".to_string(),
                    target: "b".to_string(),
                    relationship: "default_lakehouse".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
                GraphEdge {
                    source: "c".to_string(),
                    target: "d".to_string(),
                    relationship: "bound_to_model".to_string(),
                    metadata: None,
                    confidence: None,
                    discovery_method: None,
                },
            ],
            workspaces: vec![WorkspaceInfo {
                id: "ws1".to_string(),
                name: "WS".to_string(),
                capacity_id: None,
            }],
            summary: GraphSummary {
                total_nodes: 4,
                total_edges: 2,
                workspaces_scanned: 1,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        // Focus on "a" with depth 1 — should include a and b (connected), not c or d
        let focused = focus_subgraph(&graph, "a", 1);
        assert_eq!(focused.summary.total_nodes, 2);
        assert!(focused.nodes.iter().any(|n| n.id == "a"));
        assert!(focused.nodes.iter().any(|n| n.id == "b"));
        assert!(!focused.nodes.iter().any(|n| n.id == "c"));
        assert_eq!(focused.summary.total_edges, 1);
    }

    #[test]
    fn test_focus_subgraph_not_found() {
        let graph = ContextGraph {
            nodes: vec![GraphNode {
                id: "a".to_string(),
                item_type: "Notebook".to_string(),
                name: "NB".to_string(),
                workspace_id: "ws1".to_string(),
                workspace_name: "WS".to_string(),
                description: None,
                properties: None,
            }],
            edges: vec![],
            workspaces: vec![],
            summary: GraphSummary {
                total_nodes: 1,
                total_edges: 0,
                workspaces_scanned: 0,
                item_types: BTreeMap::new(),
                relationship_types: None,
            },
            meta: None,
        };

        let focused = focus_subgraph(&graph, "nonexistent", 1);
        assert_eq!(focused.summary.total_nodes, 0);
    }

    #[test]
    fn test_confidence_on_edges() {
        // Verify that confidence serializes correctly in JSON output
        let edge = GraphEdge {
            source: "a".to_string(),
            target: "b".to_string(),
            relationship: "child_of".to_string(),
            metadata: None,
            confidence: Some("high".to_string()),
            discovery_method: Some("structured_property".to_string()),
        };
        let json = serde_json::to_value(&edge).unwrap();
        assert_eq!(json["confidence"], "high");
        assert_eq!(json["discoveryMethod"], "structured_property");

        // Without confidence (None) — fields should be absent
        let edge_no_conf = GraphEdge {
            source: "a".to_string(),
            target: "b".to_string(),
            relationship: "ref".to_string(),
            metadata: None,
            confidence: None,
            discovery_method: None,
        };
        let json2 = serde_json::to_value(&edge_no_conf).unwrap();
        assert!(json2.get("confidence").is_none());
        assert!(json2.get("discoveryMethod").is_none());
    }
}
