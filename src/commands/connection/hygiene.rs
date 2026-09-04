//! Connection governance/hygiene helpers for `fabio connection`.
//!
//! These commands turn the connection-recency signals returned by the List
//! Connections API (`connectionRecency.{createdDateTime, lastBoundDateTime,
//! lastCredentialUsedDateTime}`) plus role-assignment ownership into actionable,
//! composable reports. They are all READ-ONLY — they report candidates for
//! review, never mutate. Pipe their output (`-o json --query "[].id"`) into
//! `connection delete` / `connection add-role-assignment` to act on the results
//! after human review.

use anyhow::Result;
use chrono::{DateTime, NaiveDate, TimeDelta, Utc};
use serde_json::{Value, json};
use std::collections::BTreeMap;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Parse an RFC 3339 timestamp (e.g. `2026-06-01T00:00:00Z`) into a UTC instant.
/// Returns `None` for a missing/null field or an unparseable value.
fn parse_ts(v: Option<&Value>) -> Option<DateTime<Utc>> {
    let s = v?.as_str()?;
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

/// Parse a `YYYY-MM-DD` date (midnight UTC). Teaching error on a bad format.
fn parse_date(s: &str) -> Result<DateTime<Utc>> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d")
        .map(|d| {
            d.and_hms_opt(0, 0, 0)
                .expect("midnight is always a valid time")
                .and_utc()
        })
        .map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --created-after date '{s}': {e}"),
                "Expected an ISO date, e.g. --created-after 2026-05-01",
            )
            .into()
        })
}

/// Read a connection's `connectionRecency.lastCredentialUsedDateTime`. A missing
/// value sorts as the oldest (so it "loses" when picking the connection to keep).
fn last_credential_used(conn: &Value) -> Option<DateTime<Utc>> {
    parse_ts(
        conn.get("connectionRecency")
            .and_then(|r| r.get("lastCredentialUsedDateTime")),
    )
}

/// Classify a connection as a stale candidate, returning a stable machine-readable
/// reason, or `None` if it is not stale (or cannot be reliably assessed).
///
/// Reasons (precedence order):
/// - `never-bound` — never linked to any Fabric item.
/// - `never-used` — bound, but its credentials were never used.
/// - `credentials-unused` — credentials not used since `unused_threshold`.
///
/// Connections created before `created_after` are NOT assessed: connections that
/// predate connection-recency GA report NULL recency even when actively used, so
/// treating a NULL as "stale" would produce false positives.
fn stale_reason(
    conn: &Value,
    created_after: DateTime<Utc>,
    unused_threshold: DateTime<Utc>,
) -> Option<&'static str> {
    let recency = conn.get("connectionRecency")?;
    let created = parse_ts(recency.get("createdDateTime"))?;
    if created < created_after {
        return None;
    }
    if parse_ts(recency.get("lastBoundDateTime")).is_none() {
        return Some("never-bound");
    }
    match parse_ts(recency.get("lastCredentialUsedDateTime")) {
        None => Some("never-used"),
        Some(ts) if ts < unused_threshold => Some("credentials-unused"),
        Some(_) => None,
    }
}

/// Build the equivalence key that identifies two connections as reaching the same
/// target: connection type + path + connectivity type + gateway. When
/// `match_credential_type` is set, the credential type is also part of the key
/// (so connections that differ only in credential type are NOT merged).
/// Returns `None` when the connection lacks the details needed to compare.
fn duplicate_key(conn: &Value, match_credential_type: bool) -> Option<String> {
    let details = conn.get("connectionDetails")?;
    let ctype = details.get("type").and_then(Value::as_str)?;
    let path = details.get("path").and_then(Value::as_str)?;
    let connectivity = conn
        .get("connectivityType")
        .and_then(Value::as_str)
        .unwrap_or("");
    let gateway = conn.get("gatewayId").and_then(Value::as_str).unwrap_or("");
    let mut key = format!("{ctype}\u{1f}{path}\u{1f}{connectivity}\u{1f}{gateway}");
    if match_credential_type {
        let cred = conn
            .get("credentialDetails")
            .and_then(|c| c.get("credentialType"))
            .and_then(Value::as_str)
            .unwrap_or("");
        key.push('\u{1f}');
        key.push_str(cred);
    }
    Some(key)
}

/// Given the full connection list, return the REDUNDANT connections — for every
/// group of connections reaching the same target, the most-recently-used one is
/// kept and the rest are emitted as consolidation candidates (each carrying a
/// `keepId`/`keepDisplayName` pointing at the connection to retain).
fn duplicate_candidates(conns: &[Value], match_credential_type: bool) -> Vec<Value> {
    // BTreeMap for deterministic group ordering (stable output for tests + agents).
    let mut groups: BTreeMap<String, Vec<&Value>> = BTreeMap::new();
    for c in conns {
        if let Some(k) = duplicate_key(c, match_credential_type) {
            groups.entry(k).or_default().push(c);
        }
    }

    let mut out = Vec::new();
    for group in groups.values() {
        if group.len() < 2 {
            continue;
        }
        // Keep the most-recently-used connection (NULL last-used sorts oldest).
        let winner = group
            .iter()
            .max_by_key(|c| last_credential_used(c))
            .expect("group is non-empty");
        let winner_id = winner.get("id").and_then(Value::as_str);
        let details = |c: &Value, k: &str| {
            c.get("connectionDetails")
                .and_then(|d| d.get(k))
                .cloned()
                .unwrap_or(Value::Null)
        };
        for c in group {
            let cid = c.get("id").and_then(Value::as_str);
            if cid == winner_id {
                continue;
            }
            out.push(json!({
                "id": cid,
                "displayName": c.get("displayName").cloned().unwrap_or(Value::Null),
                "connectionType": details(c, "type"),
                "path": details(c, "path"),
                "connectivityType": c.get("connectivityType").cloned().unwrap_or(Value::Null),
                "gatewayId": c.get("gatewayId").cloned().unwrap_or(Value::Null),
                "lastCredentialUsedDateTime": c
                    .get("connectionRecency")
                    .and_then(|r| r.get("lastCredentialUsedDateTime"))
                    .cloned()
                    .unwrap_or(Value::Null),
                "keepId": winner_id,
                "keepDisplayName": winner.get("displayName").cloned().unwrap_or(Value::Null),
            }));
        }
    }
    out
}

/// If a connection's ONLY `Owner` role assignment is a single individual `User`,
/// return that user's principal id (the orphan-risk case). Returns `None` when
/// there is no single owner, or the sole owner is a Group/ServicePrincipal.
fn single_user_owner(role_assignments: &[Value]) -> Option<String> {
    let owners: Vec<&Value> = role_assignments
        .iter()
        .filter(|ra| ra.get("role").and_then(Value::as_str) == Some("Owner"))
        .collect();
    if owners.len() != 1 {
        return None;
    }
    let principal = owners[0].get("principal")?;
    if principal.get("type").and_then(Value::as_str) != Some("User") {
        return None;
    }
    principal
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) async fn find_stale(
    cli: &Cli,
    client: &FabricClient,
    unused_days: u32,
    created_after: &str,
) -> Result<()> {
    let created_after_dt = parse_date(created_after)?;
    let resp = client.get_list("/connections", "value", true, None).await?;

    let threshold = Utc::now() - TimeDelta::days(i64::from(unused_days));
    let flagged: Vec<Value> = resp
        .items
        .iter()
        .filter_map(|conn| {
            let reason = stale_reason(conn, created_after_dt, threshold)?;
            Some(json!({
                "id": conn.get("id").cloned().unwrap_or(Value::Null),
                "displayName": conn.get("displayName").cloned().unwrap_or(Value::Null),
                "connectivityType": conn.get("connectivityType").cloned().unwrap_or(Value::Null),
                "reason": reason,
                "connectionRecency": conn.get("connectionRecency").cloned().unwrap_or(Value::Null),
            }))
        })
        .collect();

    output::render_list(
        cli,
        &flagged,
        &[
            "displayName",
            "id",
            "connectivityType",
            "reason",
            "connectionRecency.lastBoundDateTime",
            "connectionRecency.lastCredentialUsedDateTime",
        ],
        &[
            "NAME",
            "ID",
            "CONNECTIVITY TYPE",
            "REASON",
            "LAST BOUND",
            "LAST USED",
        ],
        "id",
    );
    Ok(())
}

pub(super) async fn find_duplicates(
    cli: &Cli,
    client: &FabricClient,
    match_credential_type: bool,
) -> Result<()> {
    let resp = client.get_list("/connections", "value", true, None).await?;
    let candidates = duplicate_candidates(&resp.items, match_credential_type);

    output::render_list(
        cli,
        &candidates,
        &[
            "displayName",
            "id",
            "connectionType",
            "path",
            "keepDisplayName",
            "keepId",
        ],
        &[
            "NAME",
            "ID",
            "CONNECTION TYPE",
            "PATH",
            "KEEP NAME",
            "KEEP ID",
        ],
        "id",
    );
    Ok(())
}

pub(super) async fn find_single_owner(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client.get_list("/connections", "value", true, None).await?;

    let mut flagged: Vec<Value> = Vec::new();
    for conn in &resp.items {
        let Some(id) = conn.get("id").and_then(Value::as_str) else {
            continue;
        };
        // Skip (rather than abort the whole sweep) connections whose role
        // assignments we cannot read — a bulk governance scan should be resilient.
        let Ok(roles) = client
            .get_list(
                &format!("/connections/{id}/roleAssignments"),
                "value",
                true,
                None,
            )
            .await
        else {
            continue;
        };
        if let Some(owner_id) = single_user_owner(&roles.items) {
            flagged.push(json!({
                "id": id,
                "displayName": conn.get("displayName").cloned().unwrap_or(Value::Null),
                "connectivityType": conn.get("connectivityType").cloned().unwrap_or(Value::Null),
                "ownerPrincipalId": owner_id,
            }));
        }
    }

    output::render_list(
        cli,
        &flagged,
        &["displayName", "id", "connectivityType", "ownerPrincipalId"],
        &["NAME", "ID", "CONNECTIVITY TYPE", "OWNER PRINCIPAL ID"],
        "id",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn_with_recency(created: &str, bound: Option<&str>, used: Option<&str>) -> Value {
        let mut recency = json!({ "createdDateTime": created });
        if let Some(b) = bound {
            recency["lastBoundDateTime"] = json!(b);
        }
        if let Some(u) = used {
            recency["lastCredentialUsedDateTime"] = json!(u);
        }
        json!({
            "id": "c1",
            "displayName": "C1",
            "connectivityType": "ShareableCloud",
            "connectionRecency": recency
        })
    }

    fn cutoff() -> DateTime<Utc> {
        parse_date("2026-05-01").unwrap()
    }

    fn threshold() -> DateTime<Utc> {
        // 90 days before a fixed "now" of 2026-09-01.
        parse_date("2026-06-03").unwrap()
    }

    #[test]
    fn stale_never_bound() {
        let c = conn_with_recency("2026-06-01T00:00:00Z", None, None);
        assert_eq!(stale_reason(&c, cutoff(), threshold()), Some("never-bound"));
    }

    #[test]
    fn stale_never_used_but_bound() {
        let c = conn_with_recency("2026-06-01T00:00:00Z", Some("2026-06-02T00:00:00Z"), None);
        assert_eq!(stale_reason(&c, cutoff(), threshold()), Some("never-used"));
    }

    #[test]
    fn stale_credentials_unused() {
        // Used on 2026-05-10, well before the 2026-06-03 threshold.
        let c = conn_with_recency(
            "2026-05-05T00:00:00Z",
            Some("2026-05-06T00:00:00Z"),
            Some("2026-05-10T00:00:00Z"),
        );
        assert_eq!(
            stale_reason(&c, cutoff(), threshold()),
            Some("credentials-unused")
        );
    }

    #[test]
    fn stale_active_connection_not_flagged() {
        // Used on 2026-08-20, after the threshold — active, not stale.
        let c = conn_with_recency(
            "2026-06-01T00:00:00Z",
            Some("2026-06-02T00:00:00Z"),
            Some("2026-08-20T00:00:00Z"),
        );
        assert_eq!(stale_reason(&c, cutoff(), threshold()), None);
    }

    #[test]
    fn stale_pre_cutoff_connection_not_assessed() {
        // Created before the reliable cutoff — NULL recency would be a false
        // positive, so it is skipped even though it is never-bound.
        let c = conn_with_recency("2026-03-01T00:00:00Z", None, None);
        assert_eq!(stale_reason(&c, cutoff(), threshold()), None);
    }

    #[test]
    fn stale_missing_recency_not_assessed() {
        let c = json!({ "id": "c1", "displayName": "C1" });
        assert_eq!(stale_reason(&c, cutoff(), threshold()), None);
    }

    fn dup_conn(id: &str, path: &str, used: Option<&str>, cred: &str) -> Value {
        let mut recency = json!({ "createdDateTime": "2026-06-01T00:00:00Z" });
        if let Some(u) = used {
            recency["lastCredentialUsedDateTime"] = json!(u);
        }
        json!({
            "id": id,
            "displayName": format!("conn-{id}"),
            "connectivityType": "ShareableCloud",
            "connectionDetails": { "type": "SQL", "path": path },
            "credentialDetails": { "credentialType": cred },
            "connectionRecency": recency
        })
    }

    #[test]
    fn duplicates_keeps_most_recently_used() {
        let conns = vec![
            dup_conn("a", "server;db", Some("2026-07-01T00:00:00Z"), "Basic"),
            dup_conn("b", "server;db", Some("2026-08-01T00:00:00Z"), "Basic"),
            dup_conn("c", "other;db", Some("2026-08-01T00:00:00Z"), "Basic"),
        ];
        let out = duplicate_candidates(&conns, false);
        // Only a/b are duplicates; b (newer) is kept, a is the candidate.
        assert_eq!(out.len(), 1);
        assert_eq!(out[0]["id"], "a");
        assert_eq!(out[0]["keepId"], "b");
        assert_eq!(out[0]["connectionType"], "SQL");
        assert_eq!(out[0]["path"], "server;db");
    }

    #[test]
    fn duplicates_none_when_all_distinct() {
        let conns = vec![
            dup_conn("a", "s1;db", Some("2026-07-01T00:00:00Z"), "Basic"),
            dup_conn("b", "s2;db", Some("2026-08-01T00:00:00Z"), "Basic"),
        ];
        assert!(duplicate_candidates(&conns, false).is_empty());
    }

    #[test]
    fn duplicates_respect_credential_type_flag() {
        // Same target, different credential type. Without the flag they are
        // duplicates; with the flag they are distinct.
        let conns = vec![
            dup_conn("a", "server;db", Some("2026-07-01T00:00:00Z"), "Basic"),
            dup_conn("b", "server;db", Some("2026-08-01T00:00:00Z"), "OAuth2"),
        ];
        assert_eq!(duplicate_candidates(&conns, false).len(), 1);
        assert!(duplicate_candidates(&conns, true).is_empty());
    }

    #[test]
    fn duplicate_key_missing_details_is_none() {
        let c = json!({ "id": "x", "connectivityType": "ShareableCloud" });
        assert!(duplicate_key(&c, false).is_none());
    }

    #[test]
    fn single_user_owner_detected() {
        let ras = vec![
            json!({ "role": "Owner", "principal": { "id": "u1", "type": "User" } }),
            json!({ "role": "User", "principal": { "id": "u2", "type": "User" } }),
        ];
        assert_eq!(single_user_owner(&ras), Some("u1".to_string()));
    }

    #[test]
    fn single_group_owner_not_flagged() {
        let ras = vec![json!({ "role": "Owner", "principal": { "id": "g1", "type": "Group" } })];
        assert_eq!(single_user_owner(&ras), None);
    }

    #[test]
    fn two_owners_not_flagged() {
        let ras = vec![
            json!({ "role": "Owner", "principal": { "id": "u1", "type": "User" } }),
            json!({ "role": "Owner", "principal": { "id": "g1", "type": "Group" } }),
        ];
        assert_eq!(single_user_owner(&ras), None);
    }

    #[test]
    fn no_owner_not_flagged() {
        let ras = vec![json!({ "role": "User", "principal": { "id": "u1", "type": "User" } })];
        assert_eq!(single_user_owner(&ras), None);
    }

    #[test]
    fn parse_date_rejects_bad_format() {
        assert!(parse_date("not-a-date").is_err());
        assert!(parse_date("2026-13-01").is_err());
        assert!(parse_date("2026-05-01").is_ok());
    }
}
