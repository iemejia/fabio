//! Item endorsement (certification) — READ-ONLY.
//!
//! Fabric/Power BI endorsement (`Promoted`/`Certified`) can only be SET in the
//! portal — there is NO public REST API to set it (verified: the entire Fabric
//! REST API spec contains zero endorsement operations, Fabric `Update Item`
//! rejects an `endorsementDetails` body with 400, and the Power BI `Datasets`
//! operation list has no "Set Endorsement"). These commands READ endorsement
//! for governance / CI checks (e.g. "is this model Certified?").
//!
//! **Source = the admin metadata scanner** (`POST /admin/workspaces/getInfo` →
//! poll `scanStatus` → `scanResult`). This is deliberate: the simpler Power BI
//! list endpoints (`GET /groups/{ws}/datasets`) and even `getGroupsAsAdmin`
//! return a STALE `endorsementDetails` (they lag portal changes, observed by
//! hours), while the scanner reflects endorsement immediately. The scanner
//! requires Fabric-admin / metadata-scanner access.
//!
//! Endorsement is a Power BI-lineage concept: the scan carries
//! `endorsementDetails` only on `datasets` (`SemanticModel`), `reports`
//! (Report/PaginatedReport), `dataflows`, `dashboards`, and `datamarts`.
//! Fabric-native items (Lakehouse, Notebook, …) have no endorsement.

use std::time::Duration;

use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

const SUPPORTED_TYPES: &str =
    "SemanticModel, Report, PaginatedReport, Dashboard, Dataflow, Datamart";

/// Metadata-scan collections that carry `endorsementDetails`, paired with the
/// fabio item-type label.
const PBI_COLLECTIONS: &[(&str, &str)] = &[
    ("datasets", "SemanticModel"),
    ("reports", "Report"),
    ("dataflows", "Dataflow"),
    ("dashboards", "Dashboard"),
    ("datamarts", "Datamart"),
];

// ─── Pure helpers (unit-tested) ──────────────────────────────────────────────

/// Parse a scanned item's `endorsementDetails` into `(endorsement, certifiedBy,
/// certificationDetails)`. A null/absent value means the item is not endorsed →
/// `"None"`.
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

/// Normalize an `--endorsement` filter to canonical
/// `Certified`/`Promoted`/`Master`/`None`.
fn normalize_endorsement_filter(input: &str) -> Result<String> {
    let v = match input.trim().to_ascii_lowercase().as_str() {
        "certified" | "certify" => "Certified",
        "promoted" | "promote" => "Promoted",
        "master" | "masterdata" | "master data" | "master-data" => "Master",
        "none" | "unendorsed" | "not-endorsed" | "notendorsed" => "None",
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Unknown endorsement '{input}'"),
                "Valid values: Certified, Promoted, Master, None",
            )
            .into());
        }
    };
    Ok(v.to_string())
}

/// The item-type label for a row, refining `reports` into `Report` vs
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

/// Read an item's display name (`name` for scanned PBI items, `displayName` fallback).
fn item_name(item: &Value) -> Value {
    item.get("name")
        .or_else(|| item.get("displayName"))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Collect endorsement rows for every endorsement-readable item in a scanned
/// workspace. Pure — testable against a fixture scan result.
fn collect_endorsements(workspace: &Value, type_filter: Option<&str>) -> Vec<Value> {
    let mut rows = Vec::new();
    for (collection, label) in PBI_COLLECTIONS {
        for item in workspace
            .get(*collection)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let typ = row_type_label(label, item);
            if type_filter.is_some_and(|t| !t.eq_ignore_ascii_case(&typ)) {
                continue;
            }
            let (endorsement, certified_by, _) = parse_endorsement(item);
            rows.push(json!({
                "id": item_id(item).unwrap_or_default(),
                "displayName": item_name(item),
                "type": typ,
                "endorsement": endorsement,
                "certifiedBy": certified_by,
            }));
        }
    }
    rows
}

/// Find the raw scanned item + its type label by id, across the
/// endorsement-readable collections.
fn find_endorsable_item<'a>(workspace: &'a Value, id: &str) -> Option<(&'a Value, String)> {
    for (collection, label) in PBI_COLLECTIONS {
        if let Some(item) = workspace
            .get(*collection)
            .and_then(Value::as_array)
            .and_then(|v| v.iter().find(|x| item_id(x) == Some(id)))
        {
            return Some((item, row_type_label(label, item)));
        }
    }
    None
}

/// Find an item's type anywhere in the scan (including Fabric-native
/// collections keyed by type name), for a helpful "not endorsable" message.
fn find_any_item_type(workspace: &Value, id: &str) -> Option<String> {
    let obj = workspace.as_object()?;
    for (key, val) in obj {
        if let Some(arr) = val.as_array()
            && arr.iter().any(|x| item_id(x) == Some(id))
        {
            // Fabric-native collections are keyed by the type name (e.g.
            // "Lakehouse"); PBI collections use plural lowercase.
            return Some(key.clone());
        }
    }
    None
}

// ─── Metadata scanner ────────────────────────────────────────────────────────

/// Run the admin metadata scanner for a single workspace and return the scanned
/// workspace object (the authoritative, fresh source of `endorsementDetails`).
async fn scan_workspace(client: &FabricClient, workspace: &str) -> Result<Value> {
    let scan = client
        .post_powerbi(
            "/admin/workspaces/getInfo?datasetSchema=false&datasetExpressions=false&lineage=false&getArtifactUsers=false",
            &json!({ "workspaces": [workspace] }),
        )
        .await
        .map_err(enrich_scanner_auth)?;
    let scan_id = scan
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| FabioError::new(ErrorCode::ApiError, "Scanner did not return a scan id"))?
        .to_string();

    // Poll until the scan succeeds (it completes in a few seconds).
    for _ in 0..30 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let status = client
            .get_powerbi(&format!("/admin/workspaces/scanStatus/{scan_id}"))
            .await?;
        match status.get("status").and_then(Value::as_str) {
            Some("Succeeded") => {
                let result = client
                    .get_powerbi(&format!("/admin/workspaces/scanResult/{scan_id}"))
                    .await?;
                return result
                    .get("workspaces")
                    .and_then(Value::as_array)
                    .and_then(|w| w.first())
                    .cloned()
                    .ok_or_else(|| {
                        FabioError::not_found(format!(
                            "Workspace '{workspace}' not found in scan result"
                        ))
                        .into()
                    });
            }
            Some("Failed") => {
                return Err(FabioError::new(
                    ErrorCode::ApiError,
                    "Metadata scan failed".to_string(),
                )
                .into());
            }
            _ => {}
        }
    }
    Err(FabioError::new(ErrorCode::Timeout, "Metadata scan did not complete in time").into())
}

/// Add an actionable hint when the scanner rejects the caller for lack of admin.
fn enrich_scanner_auth(e: anyhow::Error) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("401") || msg.contains("403") || msg.to_lowercase().contains("unauthorized") {
        return FabioError::with_hint(
            ErrorCode::Forbidden,
            "The metadata scanner requires Fabric administrator access".to_string(),
            "Endorsement is only readable via the admin metadata scanner (the non-admin Power BI \
             list endpoints return stale endorsement). Sign in as a Fabric administrator, or have \
             an admin run this.",
        )
        .into();
    }
    e
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// Show a single item's endorsement (certification) status.
pub(super) async fn show_endorsement(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let ws = scan_workspace(client, workspace).await?;

    let Some((item, typ)) = find_endorsable_item(&ws, id) else {
        // Give a precise message: is it a non-endorsable type, or absent?
        return Err(find_any_item_type(&ws, id)
            .map_or_else(
                || FabioError::not_found(format!("Item '{id}' not found in workspace '{workspace}'")),
                |actual| {
                    FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        format!("Endorsement is not readable via the API for item type '{actual}'"),
                        format!(
                            "Endorsement applies to Power BI-lineage types only: {SUPPORTED_TYPES}. \
                             Fabric-native items are not endorsable via the API."
                        ),
                    )
                },
            )
            .into());
    };

    let (endorsement, certified_by, certification_details) = parse_endorsement(item);
    let result = json!({
        "id": id,
        "type": typ,
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
    if let Some(t) = item_type
        && !PBI_COLLECTIONS.iter().any(|(_, label)| {
            label.eq_ignore_ascii_case(t) || t.eq_ignore_ascii_case("PaginatedReport")
        })
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Endorsement is not readable for item type '{t}'"),
            format!("Endorsement-readable types: {SUPPORTED_TYPES}"),
        )
        .into());
    }

    let ws = scan_workspace(client, workspace).await?;
    let mut rows = collect_endorsements(&ws, item_type);
    if let Some(f) = &filter {
        rows.retain(|r| r.get("endorsement").and_then(Value::as_str) == Some(f.as_str()));
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

    fn fixture_workspace() -> Value {
        json!({
            "id": "ws1",
            "name": "TestWorkspace",
            "datasets": [
                {"id": "d1", "name": "CertModel", "endorsementDetails": {"endorsement": "Certified", "certifiedBy": "alice@contoso.com"}},
                {"id": "d2", "name": "PromoModel", "endorsementDetails": {"endorsement": "Promoted"}},
                {"id": "d3", "name": "PlainModel", "endorsementDetails": null}
            ],
            "reports": [
                {"id": "r1", "name": "Rpt", "reportType": "PowerBIReport", "endorsementDetails": null},
                {"id": "r2", "name": "Paginated", "reportType": "PaginatedReport", "endorsementDetails": {"endorsement": "Promoted"}}
            ],
            "Lakehouse": [ {"id": "lh1", "name": "MyLake"} ]
        })
    }

    #[test]
    fn parse_endorsement_null_and_certified() {
        let (e, by, _) = parse_endorsement(&json!({"endorsementDetails": null}));
        assert_eq!(e, "None");
        assert!(by.is_null());
        let (e2, by2, _) = parse_endorsement(
            &json!({"endorsementDetails": {"endorsement": "Certified", "certifiedBy": "a@b.c"}}),
        );
        assert_eq!(e2, "Certified");
        assert_eq!(by2, "a@b.c");
    }

    #[test]
    fn normalize_endorsement_filter_aliases_and_rejects() {
        assert_eq!(
            normalize_endorsement_filter("certified").unwrap(),
            "Certified"
        );
        assert_eq!(normalize_endorsement_filter("Promote").unwrap(), "Promoted");
        assert_eq!(
            normalize_endorsement_filter("master data").unwrap(),
            "Master"
        );
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
    fn collect_endorsements_covers_all_pbi_types_with_labels() {
        let ws = fixture_workspace();
        let rows = collect_endorsements(&ws, None);
        // 3 datasets + 2 reports = 5 rows (Lakehouse is not endorsement-readable).
        assert_eq!(rows.len(), 5);
        let cert = rows.iter().find(|r| r["id"] == "d1").unwrap();
        assert_eq!(cert["type"], "SemanticModel");
        assert_eq!(cert["endorsement"], "Certified");
        assert_eq!(cert["certifiedBy"], "alice@contoso.com");
        let plain = rows.iter().find(|r| r["id"] == "d3").unwrap();
        assert_eq!(plain["endorsement"], "None");
        // Paginated report is refined.
        let pag = rows.iter().find(|r| r["id"] == "r2").unwrap();
        assert_eq!(pag["type"], "PaginatedReport");
        assert_eq!(pag["endorsement"], "Promoted");
    }

    #[test]
    fn collect_endorsements_type_filter() {
        let ws = fixture_workspace();
        let rows = collect_endorsements(&ws, Some("SemanticModel"));
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r["type"] == "SemanticModel"));
    }

    #[test]
    fn find_endorsable_item_and_any_type() {
        let ws = fixture_workspace();
        let (item, typ) = find_endorsable_item(&ws, "d2").unwrap();
        assert_eq!(typ, "SemanticModel");
        assert_eq!(item["name"], "PromoModel");
        assert!(find_endorsable_item(&ws, "lh1").is_none());
        // A Fabric-native item resolves its type (collection key) for the error.
        assert_eq!(find_any_item_type(&ws, "lh1").as_deref(), Some("Lakehouse"));
        assert_eq!(find_any_item_type(&ws, "nope"), None);
    }
}
