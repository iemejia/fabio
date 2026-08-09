//! Intent-scoped sub-skill judgment for AI agents using fabio.
//!
//! Each entry is the AUTHORED judgment JSON for a workload family
//! (`data/skills/<family>.json`: when-to-use, `must`/`prefer`/`avoid`,
//! `key_gotchas`, `troubleshooting`, `safety`, `shared_references`). The
//! generated `.agents/skills/fabio-<family>/SKILL.md` files pair this judgment
//! with a command index derived from `commands.json`; this module exposes the
//! raw judgment so an agent can retrieve it with `fabio context skill <family>`
//! and so `fabio context find` can search the gotchas/troubleshooting by
//! keyword (they are otherwise only visible by loading the whole sub-skill).
//!
//! Auto-registered by `build.rs` (like personas/workflows/best-practices).

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::output;

use super::find_entry;

pub(super) fn execute(cli: &Cli, family: &str) {
    let normalized = family.to_lowercase().replace(['-', '_'], "");
    if let Some(content) = find_entry(SKILLS, &normalized) {
        let val: Value =
            serde_json::from_str(content).unwrap_or_else(|_| json!({"content": content}));
        output::render_object(cli, &val, "family");
    } else {
        let available: Vec<&str> = SKILLS.iter().map(|(name, _)| *name).collect();
        let result = json!({
            "error": format!("No skill family found for '{family}'"),
            "available_skills": available,
            "hint": "Use 'fabio context list' to see all available skill families"
        });
        output::render_object(cli, &result, "error");
    }
}

pub(super) fn list_names() -> Vec<&'static str> {
    SKILLS.iter().map(|(name, _)| *name).collect()
}

pub(super) const fn entries() -> &'static [(&'static str, &'static str)] {
    SKILLS
}

include!(concat!(env!("OUT_DIR"), "/skills.rs"));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_skill_entries_are_valid_json() {
        for (name, content) in SKILLS {
            let val: Result<serde_json::Value, _> = serde_json::from_str(content);
            assert!(
                val.is_ok(),
                "Skill family '{name}' contains invalid JSON: {}",
                val.unwrap_err()
            );
        }
    }

    #[test]
    fn all_skill_entries_have_required_fields() {
        for (name, content) in SKILLS {
            let val: serde_json::Value = serde_json::from_str(content).unwrap();
            for field in ["family", "title", "description", "command_groups"] {
                assert!(
                    val.get(field).is_some(),
                    "Skill family '{name}' must have a '{field}' field"
                );
            }
        }
    }

    #[test]
    fn skills_is_non_empty() {
        assert!(!SKILLS.is_empty(), "SKILLS should have at least one entry");
    }
}
