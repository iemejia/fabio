//! Item endorsement (certification) — READ-ONLY.
//!
//! Fabric/Power BI endorsement (`Promoted`/`Certified`) can only be SET in the
//! portal — there is NO public REST API to set it (verified: the entire Fabric
//! REST API spec contains zero endorsement operations, Fabric `Update Item`
//! rejects an `endorsementDetails` body with 400, and the Power BI `Datasets`
//! operation list has no "Set Endorsement"). It IS readable via the Power BI
//! REST API, which returns `endorsementDetails` on its content types. These
//! commands surface that for governance / CI checks (e.g. "is this model
//! Certified?").
//!
//! Scope: the Power BI-lineage item types the API exposes endorsement for —
//! `SemanticModel`, `Report`, `PaginatedReport`, `Dashboard`, `Dataflow`,
//! `Datamart`. Fabric-native items' endorsement is only available through the
//! admin metadata scanner, which is out of scope here.

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

const SUPPORTED_TYPES: &str =
    "SemanticModel, Report, PaginatedReport, Dashboard, Dataflow, Datamart";

/// The Power BI collections queried by `list-endorsements` (all
/// endorsement-readable types), paired with the fabio item-type label.
const ALL_COLLECTIONS: &[(&str, &str)] = &[
    ("datasets", "SemanticModel"),
    ("reports", "Report"),
    ("dataflows", "Dataflow"),
    ("dashboards", "Dashboard"),
    ("datamarts", "Datamart"),
];

// ─── Pure helpers (unit-tested) ──────────────────────────────────────────────

/// Map a Fabric item type to its Power BI collection endpoint, if endorsement
/// is readable for it.
fn pbi_collection_for_type(item_type: &str) -> Option<&'static str> {
    match item_type.to_ascii_lowercase().as_str() {
        "semanticmodel" | "dataset" => Some("datasets"),
        "report" | "paginatedreport" => Some("reports"),
        "dashboard" => Some("dashboards"),
        "dataflow" => Some("dataflows"),
        "datamart" => Some("datamarts"),
        _ => None,
    }
}

/// Parse a Power BI item's `endorsementDetails` into a flat, stable shape. A
/// null/absent `endorsementDetails` means the item is not endorsed → `"None"`.
fn parse_endorsement(item: &Value) -> (String, Value, Value) {
    let details = item.get("endorsementDetails");
    let endorsement = details
        .and_then(|d| d.get("endorsement"))
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or("None")
        .to_string();
    let certified_by = details
        .and_then(|d| d.get("certifiedBy"))
        .cloned()
        .unwrap_or(Value::Null);
    let certification_details = details
        .and_then(|d| d.get("certificationDetails"))
        .cloned()
        .unwrap_or(Value::Null);
    (endorsement, certified_by, certification_details)
}

/// Normalize an `--endorsement` filter value to canonical
/// `Certified`/`Promoted`/`None`.
fn normalize_endorsement_filter(input: &str) -> Result<String> {
    let v = match input.trim().to_ascii_lowercase().as_str() {
        "certified" | "certify" => "Certified",
        "promoted" | "promote" => "Promoted",
        "none" | "unendorsed" | "not-endorsed" | "notendorsed" => "None",
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown endorsement '{input}'"),
                "Valid values: Certified, Promoted, None",
            )
            .into());
        }
    };
    Ok(v.to_string())
}

/// The fabio item-type label for a row, refining `reports` into `Report` vs
/// `PaginatedReport` from the `reportType` field.
fn row_type_label(collection_label: &str, item: &Value) -> String {
    if collection_label == "Report"
        && item.get("reportType").and_then(Value::as_str) == Some("PaginatedReport")
    {
        return "PaginatedReport".to_string();
    }
    collection_label.to_string()
}

/// Read an item's id (Power BI uses `id`; dataflows use `objectId`).
fn item_id(item: &Value) -> Option<&str> {
    item.get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("objectId").and_then(Value::as_str))
}

/// Read an item's display name (`name` for most PBI items, `displayName` fallback).
fn item_name(item: &Value) -> Value {
    item.get("name")
        .or_else(|| item.get("displayName"))
        .cloned()
        .unwrap_or(Value::Null)
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// Show a single item's endorsement (certification) status.
pub(super) async fn show_endorsement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    item_type: Option<&str>,
) -> Result<()> {
    // Resolve the type: use --type if given, else fetch it from the Fabric API.
    let resolved_type = match item_type {
        Some(t) => t.to_string(),
        None => client
            .get(&format!("/workspaces/{workspace}/items/{id}"))
            .await?
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    };

    let collection = pbi_collection_for_type(&resolved_type).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Endorsement is not readable via the API for item type '{resolved_type}'"),
            format!(
                "Endorsement read is available for Power BI-lineage types: {SUPPORTED_TYPES}. \
                 There is no public API to SET endorsement (portal-only)."
            ),
        )
    })?;

    // The collection list returns endorsementDetails uniformly; find our item.
    let data = client
        .get_powerbi(&format!("/groups/{workspace}/{collection}"))
        .await?;
    let items = data.get("value").and_then(Value::as_array);
    let item = items
        .and_then(|v| v.iter().find(|x| item_id(x) == Some(id)))
        .ok_or_else(|| {
            FabioError::not_found(format!(
                "Item '{id}' not found among {collection} in workspace '{workspace}'"
            ))
        })?;

    let (endorsement, certified_by, certification_details) = parse_endorsement(item);
    let result = json!({
        "id": id,
        "type": resolved_type,
        "displayName": item_name(item),
        "endorsement": endorsement,
        "certifiedBy": certified_by,
        "certificationDetails": certification_details,
    });
    output::render_object(cli, &result, "endorsement");
    Ok(())
}

/// List items with their endorsement status, optionally filtered by endorsement
/// level and/or item type.
pub(super) async fn list_endorsements(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    endorsement: Option<&str>,
    item_type: Option<&str>,
) -> Result<()> {
    let filter = endorsement.map(normalize_endorsement_filter).transpose()?;

    // Which collections to query.
    let collections: Vec<(&str, &str)> = match item_type {
        Some(t) => {
            let col = pbi_collection_for_type(t).ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Endorsement is not readable for item type '{t}'"),
                    format!("Endorsement-readable types: {SUPPORTED_TYPES}"),
                )
            })?;
            ALL_COLLECTIONS
                .iter()
                .copied()
                .filter(|(c, _)| *c == col)
                .collect()
        }
        None => ALL_COLLECTIONS.to_vec(),
    };

    let mut rows = Vec::new();
    for (collection, label) in collections {
        // A collection can 404 (feature off) or be empty; tolerate per-collection.
        let Ok(data) = client
            .get_powerbi(&format!("/groups/{workspace}/{collection}"))
            .await
        else {
            continue;
        };
        for item in data
            .get("value")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (endorsement, certified_by, _) = parse_endorsement(item);
            if filter.as_deref().is_some_and(|f| f != endorsement) {
                continue;
            }
            rows.push(json!({
                "id": item_id(item).unwrap_or_default(),
                "displayName": item_name(item),
                "type": row_type_label(label, item),
                "endorsement": endorsement,
                "certifiedBy": certified_by,
            }));
        }
    }

    output::render_list_with_token(
        cli,
        &rows,
        &["id", "displayName", "type", "endorsement", "certifiedBy"],
        &["ID", "NAME", "TYPE", "ENDORSEMENT", "CERTIFIED BY"],
        "displayName",
        None,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbi_collection_maps_endorsable_types() {
        assert_eq!(pbi_collection_for_type("SemanticModel"), Some("datasets"));
        assert_eq!(pbi_collection_for_type("dataset"), Some("datasets"));
        assert_eq!(pbi_collection_for_type("Report"), Some("reports"));
        assert_eq!(pbi_collection_for_type("PaginatedReport"), Some("reports"));
        assert_eq!(pbi_collection_for_type("Dashboard"), Some("dashboards"));
        assert_eq!(pbi_collection_for_type("Dataflow"), Some("dataflows"));
        assert_eq!(pbi_collection_for_type("Datamart"), Some("datamarts"));
        // Fabric-native / non-endorsable-via-API types.
        assert_eq!(pbi_collection_for_type("Lakehouse"), None);
        assert_eq!(pbi_collection_for_type("Notebook"), None);
    }

    #[test]
    fn parse_endorsement_null_is_none() {
        let (e, by, _) = parse_endorsement(&json!({"id": "x", "endorsementDetails": null}));
        assert_eq!(e, "None");
        assert!(by.is_null());
        // Absent field also → None.
        let (e2, _, _) = parse_endorsement(&json!({"id": "x"}));
        assert_eq!(e2, "None");
    }

    #[test]
    fn parse_endorsement_certified() {
        let item = json!({
            "id": "x",
            "endorsementDetails": {"endorsement": "Certified", "certifiedBy": "alice@contoso.com"}
        });
        let (e, by, _) = parse_endorsement(&item);
        assert_eq!(e, "Certified");
        assert_eq!(by, "alice@contoso.com");
    }

    #[test]
    fn normalize_endorsement_filter_aliases_and_rejects() {
        assert_eq!(
            normalize_endorsement_filter("certified").unwrap(),
            "Certified"
        );
        assert_eq!(normalize_endorsement_filter("Promote").unwrap(), "Promoted");
        assert_eq!(normalize_endorsement_filter("unendorsed").unwrap(), "None");
        assert!(normalize_endorsement_filter("gold").is_err());
    }

    #[test]
    fn row_type_label_refines_paginated_reports() {
        assert_eq!(
            row_type_label("Report", &json!({"reportType": "PaginatedReport"})),
            "PaginatedReport"
        );
        assert_eq!(
            row_type_label("Report", &json!({"reportType": "PowerBIReport"})),
            "Report"
        );
        assert_eq!(row_type_label("SemanticModel", &json!({})), "SemanticModel");
    }

    #[test]
    fn item_id_prefers_id_then_objectid() {
        assert_eq!(item_id(&json!({"id": "a"})), Some("a"));
        assert_eq!(item_id(&json!({"objectId": "b"})), Some("b"));
        assert_eq!(item_id(&json!({})), None);
    }
}
