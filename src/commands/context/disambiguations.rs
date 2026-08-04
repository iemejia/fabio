//! Terminology disambiguation tables for overloaded Fabric terms.
//!
//! Many Fabric terms mean different things in different workloads (e.g.
//! "materialized view" in Spark vs KQL vs Warehouse). These tables resolve a
//! term to the concrete artifact + the fabio command group that handles it, so
//! agents route to the right place. Authored as JSON data, auto-registered by
//! `build.rs`.

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::output;

use super::find_entry;

pub(super) fn execute(cli: &Cli, term: &str) {
    let normalized = term.to_lowercase().replace(['-', '_', ' '], "");
    // Match the entry key (filename) first, then fall back to any table's
    // declared `term`/`aliases` so an aliased term (e.g. "data-activator" or
    // "mirror") resolves to its table.
    let hit = find_entry(DISAMBIGUATIONS, &normalized).or_else(|| find_by_alias(&normalized));
    if let Some(content) = hit {
        let val: Value =
            serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
        output::render_object(cli, &val, "term");
    } else {
        let available: Vec<&str> = DISAMBIGUATIONS.iter().map(|(name, _)| *name).collect();
        let result = json!({
            "error": format!("No disambiguation table found for '{term}'"),
            "available_terms": available,
            "hint": "Use 'fabio context list' to see all disambiguation terms"
        });
        output::render_object(cli, &result, "error");
    }
}

/// Resolve a normalized query against each table's `term` + `aliases` fields.
fn find_by_alias(normalized: &str) -> Option<&'static str> {
    let matches = |s: &str| s.to_lowercase().replace(['-', '_', ' '], "") == normalized;
    for (_, content) in DISAMBIGUATIONS {
        let Ok(v) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if v.get("term").and_then(Value::as_str).is_some_and(matches) {
            return Some(content);
        }
        if v.get("aliases")
            .and_then(Value::as_array)
            .is_some_and(|arr| arr.iter().filter_map(Value::as_str).any(matches))
        {
            return Some(content);
        }
    }
    None
}

pub(super) fn list_names() -> Vec<&'static str> {
    DISAMBIGUATIONS.iter().map(|(name, _)| *name).collect()
}

pub(super) const fn entries() -> &'static [(&'static str, &'static str)] {
    DISAMBIGUATIONS
}

include!(concat!(env!("OUT_DIR"), "/disambiguations.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_disambiguation_entries_are_valid_json() {
        for (name, content) in DISAMBIGUATIONS {
            let val: Result<serde_json::Value, _> = serde_json::from_str(content);
            assert!(
                val.is_ok(),
                "Disambiguation '{name}' contains invalid JSON: {}",
                val.unwrap_err()
            );
        }
    }

    #[test]
    fn all_disambiguation_entries_have_required_fields() {
        for (name, content) in DISAMBIGUATIONS {
            let val: serde_json::Value = serde_json::from_str(content).unwrap();
            assert!(
                val.get("term").is_some(),
                "Disambiguation '{name}' must have a 'term' field"
            );
            assert!(
                val.get("summary").is_some(),
                "Disambiguation '{name}' must have a 'summary' field for discoverability"
            );
            assert!(
                val.get("meanings").is_some(),
                "Disambiguation '{name}' must have a 'meanings' field"
            );
        }
    }

    #[test]
    fn disambiguations_is_non_empty() {
        assert!(
            !DISAMBIGUATIONS.is_empty(),
            "DISAMBIGUATIONS should have at least one entry"
        );
    }

    #[test]
    fn find_by_alias_resolves_declared_aliases() {
        // A declared alias (normalized) must resolve to its table, not just the
        // filename term. "mirror" is an alias of the "mirroring" table.
        assert!(
            find_by_alias("mirror").is_some(),
            "alias 'mirror' should resolve"
        );
        // Space/hyphen/underscore-insensitive: "data activator" -> normalized.
        assert!(
            find_by_alias("dataactivator").is_some(),
            "alias 'data activator' should resolve"
        );
        assert!(find_by_alias("nonexistentterm").is_none());
    }

    #[test]
    fn activator_table_maps_data_activator_to_reflex() {
        // Regression: "Data Activator" == "Reflex" must be discoverable, and the
        // table must route to the `reflex` command group.
        let content = find_entry(DISAMBIGUATIONS, "activator")
            .expect("activator disambiguation table must exist");
        let v: serde_json::Value = serde_json::from_str(content).unwrap();
        let aliases: Vec<&str> = v["aliases"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect();
        assert!(aliases.contains(&"data activator"));
        assert!(aliases.contains(&"reflex"));
        assert_eq!(v["meanings"][0]["command_group"], "reflex");
    }
}
