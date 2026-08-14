//! Architecture-shape blueprints for AI agents.
//!
//! A blueprint is a higher-level abstraction than a workflow recipe: it describes
//! a whole solution SHAPE for a problem (which storage, which ingestion, which
//! item set with deployment phase, and the key decisions) rather than a linear
//! sequence of commands. Blueprints are the fabio-native equivalent of Microsoft
//! Fabric "task flows" — an agent routes a business problem to a blueprint (via
//! the `data-solution-architect` persona or `context find`), reads the item set +
//! decisions, then delegates the mechanics to workflows and command groups.
//!
//! Authored once as JSON data and auto-registered by `build.rs` (like workflows,
//! personas, and best-practices).

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::output;

use super::find_entry;

pub(super) fn execute(cli: &Cli, name: &str) {
    let normalized = name.to_lowercase().replace(['-', '_'], "");
    if let Some(content) = find_entry(BLUEPRINTS, &normalized) {
        let val: Value =
            serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
        output::render_object(cli, &val, "name");
    } else {
        let available: Vec<&str> = BLUEPRINTS.iter().map(|(name, _)| *name).collect();
        let result = json!({
            "error": format!("No blueprint found for '{name}'"),
            "available_blueprints": available,
            "hint": "Use 'fabio context list' to see all blueprints, or 'fabio context persona data-solution-architect' to route a problem to one"
        });
        output::render_object(cli, &result, "error");
    }
}

pub(super) fn list_names() -> Vec<&'static str> {
    BLUEPRINTS.iter().map(|(name, _)| *name).collect()
}

pub(super) const fn entries() -> &'static [(&'static str, &'static str)] {
    BLUEPRINTS
}

include!(concat!(env!("OUT_DIR"), "/blueprints.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_blueprint_entries_are_valid_json() {
        for (name, content) in BLUEPRINTS {
            let val: Result<serde_json::Value, _> = serde_json::from_str(content);
            assert!(
                val.is_ok(),
                "Blueprint '{name}' contains invalid JSON: {}",
                val.unwrap_err()
            );
        }
    }

    #[test]
    fn all_blueprint_entries_have_required_fields() {
        for (name, content) in BLUEPRINTS {
            let val: serde_json::Value = serde_json::from_str(content).unwrap();
            assert!(
                val.get("name").is_some(),
                "Blueprint '{name}' must have a 'name' field"
            );
            assert!(
                val.get("description").is_some(),
                "Blueprint '{name}' must have a 'description' field for discoverability"
            );
            assert!(
                val.get("item_set")
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|a| !a.is_empty()),
                "Blueprint '{name}' must have a non-empty 'item_set' array"
            );
        }
    }

    #[test]
    fn blueprints_is_non_empty() {
        assert!(
            !BLUEPRINTS.is_empty(),
            "BLUEPRINTS should have at least one entry"
        );
    }
}
