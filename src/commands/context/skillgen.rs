//! Generator for intent-scoped sub-skills (Layer 2 of the information architecture).
//!
//! Each sub-skill combines **authored judgment** (a JSON file in `data/skills/`,
//! auto-registered by `build.rs`) with a **generated command index** pulled from
//! `commands.json` (the source of truth). This realizes the division of labor:
//! prose carries judgment (when to use, gotchas, safety, routing); the command
//! table is mechanically derived and therefore drift-free.
//!
//! The generated Markdown lives at `.agents/skills/fabio-<family>/SKILL.md`.
//! Regenerate with `cargo test generate_subskills -- --ignored`; a drift test
//! (`subskills_match_generated`) fails in CI if the committed files are stale.

use serde_json::Value;
use std::fmt::Write as _;

include!(concat!(env!("OUT_DIR"), "/skills.rs"));

/// Directory name for a sub-skill family (e.g. `lakehouse` -> `fabio-lakehouse`).
fn subskill_dir_name(family: &str) -> String {
    format!("fabio-{family}")
}

/// Render a bullet list from a JSON array field, or an empty string if absent.
fn render_bullets(value: Option<&Value>) -> String {
    let Some(arr) = value.and_then(Value::as_array) else {
        return String::new();
    };
    arr.iter()
        .filter_map(Value::as_str)
        .fold(String::new(), |mut out, s| {
            let _ = writeln!(out, "- {s}");
            out
        })
}

/// Escape a Markdown table cell (pipes would break the column layout).
fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Build the generated command-index section for a sub-skill from `commands.json`.
fn render_command_index(command_groups: &[&str], commands: &Value) -> String {
    let mut out = String::from(
        "## Command index\n\nGenerated from fabio's command schema. For full flag details use `fabio context agent --group <group>` or `fabio context describe <group> <command>`.\n\n",
    );
    for group in command_groups {
        let Some(group_val) = commands.get(*group) else {
            continue;
        };
        let group_desc = group_val
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("");
        let _ = writeln!(out, "### fabio {group}");
        if !group_desc.is_empty() {
            let _ = writeln!(out, "{group_desc}\n");
        }
        let Some(subcommands) = group_val.get("subcommands").and_then(Value::as_object) else {
            out.push('\n');
            continue;
        };
        out.push_str("| Command | Mutates | Description |\n|---|---|---|\n");
        let mut names: Vec<&String> = subcommands.keys().collect();
        names.sort();
        for name in names {
            let sub = &subcommands[name];
            let desc = sub.get("description").and_then(Value::as_str).unwrap_or("");
            let mutates = sub.get("mutates").and_then(Value::as_bool).unwrap_or(false);
            let _ = writeln!(
                out,
                "| `fabio {group} {name}` | {} | {} |",
                if mutates { "yes" } else { "no" },
                escape_cell(desc)
            );
        }
        out.push('\n');
    }
    out
}

/// Render one subsection (MUST / PREFER / AVOID) into `md` if its array is present.
fn render_triad_subsection(md: &mut String, family_value: &Value, key: &str, heading: &str) {
    let bullets = render_bullets(family_value.get(key));
    if !bullets.is_empty() {
        let _ = writeln!(md, "### {heading}");
        md.push_str(&bullets);
        md.push('\n');
    }
}

/// Render the "## Must / Prefer / Avoid" behavioral-guidance section, if present.
fn render_must_prefer_avoid(family_value: &Value) -> String {
    let has_any = ["must", "prefer", "avoid"].iter().any(|k| {
        family_value
            .get(*k)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    });
    if !has_any {
        return String::new();
    }
    let mut out = String::from("## Must / Prefer / Avoid\n");
    render_triad_subsection(&mut out, family_value, "must", "MUST");
    render_triad_subsection(&mut out, family_value, "prefer", "PREFER");
    render_triad_subsection(&mut out, family_value, "avoid", "AVOID");
    out
}

/// Render the "## Troubleshooting" symptom -> fix table from an array of
/// `{symptom, fix}` objects.
fn render_troubleshooting(family_value: &Value) -> String {
    let Some(rows) = family_value
        .get("troubleshooting")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    else {
        return String::new();
    };
    let mut out = String::from("## Troubleshooting\n| Symptom | Fix |\n|---|---|\n");
    for row in rows {
        let symptom = row.get("symptom").and_then(Value::as_str).unwrap_or("");
        let fix = row.get("fix").and_then(Value::as_str).unwrap_or("");
        let _ = writeln!(out, "| {} | {} |", escape_cell(symptom), escape_cell(fix));
    }
    out.push('\n');
    out
}

/// Look up a best-practice topic's summary from the embedded best-practices data.
/// Returns `None` if the topic does not exist (used by drift/validation tests).
fn best_practice_summary(topic: &str) -> Option<String> {
    let normalized = topic.to_lowercase().replace(['-', '_'], "");
    super::best_practices::entries()
        .iter()
        .find(|(name, _)| name.to_lowercase().replace(['-', '_'], "") == normalized)
        .and_then(|(_, content)| serde_json::from_str::<Value>(content).ok())
        .and_then(|v| {
            v.get("summary")
                .or_else(|| v.get("title"))
                .and_then(Value::as_str)
                .map(String::from)
        })
}

/// Render the "## Shared references" section — the cross-cutting "common" layer.
/// Each entry links to a `context best-practices` topic; the "Covers" column is
/// pulled from that topic's own summary, so it stays drift-free.
fn render_shared_references(family_value: &Value) -> String {
    let Some(topics) = family_value
        .get("shared_references")
        .and_then(Value::as_array)
        .filter(|a| !a.is_empty())
    else {
        return String::new();
    };
    let mut out = String::from(
        "## Shared references\nCross-cutting operational guidance (the \"common\" layer) — consult the relevant topic before non-trivial work:\n\n| Reference | Covers |\n|---|---|\n",
    );
    for topic in topics.iter().filter_map(Value::as_str) {
        let summary = best_practice_summary(topic).unwrap_or_default();
        let _ = writeln!(
            out,
            "| `fabio context best-practices {topic}` | {} |",
            escape_cell(&summary)
        );
    }
    out.push('\n');
    out
}

/// Generate the full SKILL.md Markdown for one sub-skill family.
pub(super) fn generate_markdown(family_value: &Value, commands: &Value) -> String {
    let family = family_value
        .get("family")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let title = family_value
        .get("title")
        .and_then(Value::as_str)
        .unwrap_or(family);
    let description = family_value
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let command_groups: Vec<&str> = family_value
        .get("command_groups")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let name = subskill_dir_name(family);

    let mut md = String::new();
    // Frontmatter — folded block scalar avoids quote-escaping issues.
    md.push_str("---\n");
    let _ = writeln!(md, "name: {name}");
    md.push_str("description: >-\n");
    let _ = writeln!(md, "  {description}");
    md.push_str("license: MIT\n");
    md.push_str("---\n\n");

    let _ = writeln!(md, "# {name} — {title}\n");
    md.push_str(
        "> **Generated file — do not edit by hand.** This intent-scoped sub-skill of the `fabio` \
         skill is generated from fabio's command schema plus authored judgment. Regenerate with \
         `cargo test generate_subskills -- --ignored`. For install, auth, output envelope, global \
         flags, and agent-safety rules, see the root `fabio` skill.\n\n",
    );
    md.push_str(
        "> **Prefer runtime introspection.** This index is a snapshot; the installed binary is \
         always authoritative. Use `fabio context agent --group <group>` and \
         `fabio context describe <group> <command>` for exact flags and output shapes.\n\n",
    );

    let when = render_bullets(family_value.get("when_to_use"));
    if !when.is_empty() {
        md.push_str("## When to use\n");
        md.push_str(&when);
        md.push('\n');
    }

    let when_not = render_bullets(family_value.get("when_not_to_use"));
    if !when_not.is_empty() {
        md.push_str("## When NOT to use (route elsewhere)\n");
        md.push_str(&when_not);
        md.push('\n');
    }

    md.push_str(&render_command_index(&command_groups, commands));

    md.push_str(&render_must_prefer_avoid(family_value));

    let gotchas = render_bullets(family_value.get("key_gotchas"));
    if !gotchas.is_empty() {
        md.push_str("## Key gotchas\n");
        md.push_str(&gotchas);
        md.push('\n');
    }

    md.push_str(&render_troubleshooting(family_value));

    let safety = render_bullets(family_value.get("safety"));
    if !safety.is_empty() {
        md.push_str("## Safety\n");
        md.push_str(&safety);
        md.push('\n');
    }

    md.push_str(&render_shared_references(family_value));

    let see_also = render_bullets(family_value.get("see_also"));
    if !see_also.is_empty() {
        md.push_str("## See also\n");
        md.push_str(&see_also);
    }

    // Normalize to a single trailing newline.
    format!("{}\n", md.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commands() -> Value {
        super::super::agent_commands_schema()
    }

    #[test]
    fn all_skill_families_are_valid_json_with_required_fields() {
        for (name, content) in SKILLS {
            let val: Value = serde_json::from_str(content)
                .unwrap_or_else(|e| panic!("skill family '{name}' invalid JSON: {e}"));
            assert!(
                val.get("family").is_some(),
                "skill family '{name}' must have a 'family' field"
            );
            assert!(
                val.get("description").is_some(),
                "skill family '{name}' must have a 'description' field"
            );
            assert!(
                val.get("command_groups")
                    .and_then(Value::as_array)
                    .is_some(),
                "skill family '{name}' must have a 'command_groups' array"
            );
        }
    }

    #[test]
    fn skill_family_command_groups_exist_in_schema() {
        let cmds = commands();
        for (name, content) in SKILLS {
            let val: Value = serde_json::from_str(content).unwrap();
            for group in val.get("command_groups").and_then(Value::as_array).unwrap() {
                let group = group.as_str().unwrap();
                assert!(
                    cmds.get(group).is_some(),
                    "skill family '{name}' references unknown command group '{group}'"
                );
            }
        }
    }

    /// RELEASE GATE — every command group's SUBCOMMANDS must be discoverable in a
    /// generated sub-skill command table (so skills + context stay consistent with
    /// the CLI down to the subcommand level).
    ///
    /// A sub-skill command index is generated for each SKILL FAMILY from its
    /// `command_groups`, so every subcommand of a family-covered group appears in a
    /// table (name + description + mutates), kept current by `subskills_match_generated`.
    /// Personas do NOT generate tables — they are additive routing — so a group
    /// covered only by a persona would leave its subcommands out of every table.
    /// Therefore every command group MUST be in a skill family
    /// (`data/skills/<f>.json` `command_groups`) OR the cross-cutting/meta allowlist
    /// below (those groups are documented in the root skill, not a workload family).
    #[test]
    fn every_command_group_has_a_knowledge_home() {
        use std::collections::BTreeSet;

        // Cross-cutting / meta / core-infra groups documented in the ROOT skill,
        // not a workload family. Adding a workload group here is NOT a valid fix —
        // give it a real skill family so its subcommands get a generated table.
        const CROSS_CUTTING: &[&str] = &[
            "auth",
            "catalog",
            "completions",
            "context",
            "feedback",
            "item",
            "jobs",
            "mcp",
            "operation",
            "profile",
            "rest",
            "upgrade",
        ];

        let cmds = commands();
        let all_groups: BTreeSet<&str> = cmds
            .as_object()
            .expect("commands schema is an object")
            .iter()
            .filter(|(_, v)| v.get("subcommands").is_some())
            .map(|(k, _)| k.as_str())
            .collect();

        // Keep the allowlist honest: every entry must be a real command group.
        for g in CROSS_CUTTING {
            assert!(
                all_groups.contains(g),
                "CROSS_CUTTING allowlist entry '{g}' is not a real command group — remove the stale entry"
            );
        }

        // Groups whose subcommands appear in a generated sub-skill command table
        // (= groups claimed by a skill family).
        let mut covered: BTreeSet<String> = BTreeSet::new();
        for (_, content) in SKILLS {
            let val: Value = serde_json::from_str(content).unwrap();
            for g in val["command_groups"].as_array().into_iter().flatten() {
                if let Some(g) = g.as_str() {
                    covered.insert(g.to_owned());
                }
            }
        }
        for g in CROSS_CUTTING {
            covered.insert((*g).to_owned());
        }

        let uncovered: Vec<&str> = all_groups
            .iter()
            .copied()
            .filter(|g| !covered.contains(*g))
            .collect();
        assert!(
            uncovered.is_empty(),
            "Command group(s) have no generated sub-skill command table: {uncovered:?}.\n\
             Every workload group must be listed in a skill family's `command_groups` \
             (src/commands/context/data/skills/<family>.json) so all its subcommands \
             appear in a generated sub-skill index (a persona routes but does NOT generate \
             a table). A genuinely cross-cutting/meta/core-infra group goes in the root skill \
             AND the CROSS_CUTTING allowlist in this test. After editing a family, regenerate: \
             cargo test generate_subskills -- --ignored"
        );
    }

    #[test]
    fn skill_family_shared_references_exist() {
        for (name, content) in SKILLS {
            let val: Value = serde_json::from_str(content).unwrap();
            if let Some(topics) = val.get("shared_references").and_then(Value::as_array) {
                for topic in topics.iter().filter_map(Value::as_str) {
                    assert!(
                        best_practice_summary(topic).is_some(),
                        "skill family '{name}' references unknown best-practice topic '{topic}'"
                    );
                }
            }
        }
    }

    #[test]
    fn generated_markdown_has_frontmatter_and_index() {
        let cmds = commands();
        let (_, content) = SKILLS
            .iter()
            .find(|(n, _)| *n == "lakehouse")
            .expect("lakehouse family exists");
        let val: Value = serde_json::from_str(content).unwrap();
        let md = generate_markdown(&val, &cmds);
        assert!(md.starts_with("---\nname: fabio-lakehouse\n"));
        assert!(md.contains("## Command index"));
        assert!(md.contains("`fabio lakehouse create`"));
        assert!(md.contains("## When NOT to use"));
        assert!(md.contains("## Must / Prefer / Avoid"));
        assert!(md.contains("### MUST"));
        assert!(md.contains("## Troubleshooting"));
        assert!(md.contains("| Symptom | Fix |"));
    }

    /// Drift detection: committed sub-skill files must match generator output.
    #[test]
    fn subskills_match_generated() {
        let cmds = commands();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".agents/skills");
        let mut stale = Vec::new();
        for (name, content) in SKILLS {
            let val: Value = serde_json::from_str(content).unwrap();
            let family = val.get("family").and_then(Value::as_str).unwrap_or(name);
            let expected = generate_markdown(&val, &cmds);
            let path = base.join(subskill_dir_name(family)).join("SKILL.md");
            match std::fs::read_to_string(&path) {
                Ok(actual) if actual == expected => {}
                Ok(_) => stale.push(format!("{} (out of date)", path.display())),
                Err(_) => stale.push(format!("{} (missing)", path.display())),
            }
        }
        assert!(
            stale.is_empty(),
            "Generated sub-skills are stale or missing:\n  {}\n\
             Run `cargo test generate_subskills -- --ignored` to regenerate.",
            stale.join("\n  ")
        );
    }

    /// Regenerate the sub-skill Markdown files. Run with:
    /// `cargo test generate_subskills -- --ignored`
    #[test]
    #[ignore = "writes sub-skill SKILL.md files to disk — run manually after changing commands or skill families"]
    fn generate_subskills() {
        let cmds = commands();
        let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".agents/skills");
        for (name, content) in SKILLS {
            let val: Value = serde_json::from_str(content).unwrap();
            let family = val.get("family").and_then(Value::as_str).unwrap_or(name);
            let md = generate_markdown(&val, &cmds);
            let dir = base.join(subskill_dir_name(family));
            std::fs::create_dir_all(&dir).expect("create sub-skill dir");
            let path = dir.join("SKILL.md");
            std::fs::write(&path, md).expect("write sub-skill SKILL.md");
            println!("Wrote {}", path.display());
        }
    }
}
