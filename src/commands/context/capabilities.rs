//! Item-capability matrix for AI agents.
//!
//! Answers, per Fabric item type: can fabio create it, what CI/CD deploy
//! strategy applies, can `fabio deploy` round-trip it from an exported
//! definition, and its definition format.
//!
//! Everything here is **derived at runtime** from fabio's own sources of truth —
//! there is NO hand-maintained registry to drift:
//!   - the item-type universe: `item::known_item_types()` ∪ `deploy::ordering::DEPLOY_ORDER`
//!   - the "content" signal (has a versionable definition): union of
//!     `definition_spec::spec_for()` and a schema's `definition_format`
//!   - deployability + order: membership/position in `DEPLOY_ORDER`
//!   - creatability: whether the type's command group exposes a `create` subcommand
//!     in the (drift-checked) `commands.json`
//!
//! Because the matrix is a pure function of those sources, it cannot disagree
//! with fabio's actual behavior. To change a capability, change the source
//! (e.g. add a definition spec) — the matrix follows automatically. Consistency
//! is enforced by the unit tests below.

use serde_json::{Value, json};

use crate::cli::Cli;
use crate::commands::deploy::ordering::{DEPLOY_ORDER, deploy_priority};
use crate::output;

use super::schemas;

/// One item type's derived capabilities.
struct Capability {
    item_type: &'static str,
    creatable: bool,
    supports_definition: bool,
    deploy_strategy: &'static str,
    deployable_from_definition: bool,
    definition_format: Option<String>,
    deploy_order: Option<usize>,
}

impl Capability {
    fn to_json(&self) -> Value {
        json!({
            "type": self.item_type,
            "creatable": self.creatable,
            "supports_definition": self.supports_definition,
            "deploy_strategy": self.deploy_strategy,
            "deployable_from_definition": self.deployable_from_definition,
            "definition_format": self.definition_format,
            "deploy_order": self.deploy_order,
        })
    }
}

/// Item types fabio cannot map to its command group by pure normalization
/// (irregular casing that drops a word). Keyed by item type → command group.
const GROUP_ALIASES: &[(&str, &str)] = &[(
    "MirroredAzureDatabricksCatalog",
    "mirrored-databricks-catalog",
)];

/// Normalize a name for case/-/_-insensitive matching.
fn normalize(s: &str) -> String {
    s.to_lowercase().replace(['-', '_'], "")
}

/// Resolve an item type to its `commands.json` command-group key, if any.
fn group_for<'a>(item_type: &str, groups: &'a serde_json::Map<String, Value>) -> Option<&'a str> {
    if let Some((_, g)) = GROUP_ALIASES.iter().find(|(t, _)| *t == item_type) {
        return groups.keys().find(|k| k.as_str() == *g).map(String::as_str);
    }
    let target = normalize(item_type);
    groups
        .keys()
        .find(|k| normalize(k) == target)
        .map(String::as_str)
}

/// True when the type's command group exposes a `create`/`create-*` subcommand.
fn is_creatable(item_type: &str, commands: &Value) -> bool {
    group_subcommands(item_type, commands).is_some_and(|subs| {
        subs.keys()
            .any(|s| s == "create" || s.starts_with("create"))
    })
}

/// True when fabio exposes definition GET/update commands for the type — the
/// authoritative `api.definition` signal, read straight from the (drift-checked)
/// command surface. Distinct from `deploy_strategy`: a type can support the
/// definition API yet deploy as a shell (e.g. Lakehouse has an empty definition).
fn supports_definition_api(item_type: &str, commands: &Value) -> bool {
    group_subcommands(item_type, commands).is_some_and(|subs| {
        subs.keys()
            .any(|s| s == "update-definition" || s == "get-definition")
    })
}

/// The `subcommands` object for the item type's command group, if resolvable.
fn group_subcommands<'a>(
    item_type: &str,
    commands: &'a Value,
) -> Option<&'a serde_json::Map<String, Value>> {
    let groups = commands.as_object()?;
    let group = group_for(item_type, groups)?;
    groups
        .get(group)
        .and_then(|g| g.get("subcommands"))
        .and_then(Value::as_object)
}

/// True when the type has a versionable definition (union of the two authoritative
/// content signals: a `definition_spec` OR a schema `definition_format`).
fn has_definition(item_type: &str) -> bool {
    crate::definition_spec::spec_for(item_type).is_some()
        || schemas::definition_format_for(item_type).is_some()
}

/// The definition format for a content-strategy type (spec format preferred,
/// else the schema's declared format).
fn definition_format(item_type: &str) -> Option<String> {
    crate::definition_spec::spec_for(item_type)
        .and_then(|s| s.format.clone())
        .or_else(|| schemas::definition_format_for(item_type))
}

#[inline]
fn in_deploy_order(item_type: &str) -> bool {
    deploy_priority(item_type) < DEPLOY_ORDER.len()
}

/// Compute the full capability matrix, sorted by item type.
fn derive() -> Vec<Capability> {
    let commands = super::agent_commands_schema();

    // Universe = every known item type ∪ every deployable type.
    let mut types: Vec<&'static str> = crate::commands::item::known_item_types().to_vec();
    for t in DEPLOY_ORDER {
        if !types.contains(t) {
            types.push(t);
        }
    }
    types.sort_unstable();

    types
        .into_iter()
        .map(|t| {
            let content = has_definition(t);
            let deployable = in_deploy_order(t);
            let deploy_strategy = if content {
                "content"
            } else if deployable {
                "platform_only"
            } else {
                "unsupported"
            };
            Capability {
                item_type: t,
                creatable: is_creatable(t, &commands),
                supports_definition: supports_definition_api(t, &commands),
                deploy_strategy,
                deployable_from_definition: content,
                definition_format: if content { definition_format(t) } else { None },
                deploy_order: deployable.then(|| deploy_priority(t)),
            }
        })
        .collect()
}

fn legend() -> Value {
    json!({
        "deploy_strategy": {
            "content": "fabio deploy exports and pushes a versionable definition; fully round-trippable.",
            "platform_only": "fabio deploy creates a shell (.platform + creationPayload) only — no versionable definition content.",
            "unsupported": "Not in fabio's deploy order; cannot be created/updated by fabio deploy (portal-only or auto-provisioned)."
        },
        "creatable": "fabio exposes a create/create-* command for this type.",
        "supports_definition": "fabio exposes get-definition/update-definition for this type (the API definition axis). A type may support the definition API yet still deploy as a shell (platform_only) when the definition carries no versionable content, e.g. Lakehouse.",
        "deployable_from_definition": "fabio deploy can recreate this item from an exported definition (== content strategy). The CI/CD 'verified' axis.",
        "deploy_order": "0-based position in fabio's dependency-ordered deploy sequence (null when unsupported).",
        "derivation": "Derived at runtime from known item types + definition specs + schema definition_format + DEPLOY_ORDER + commands.json. No hand-maintained registry — change the source and the matrix follows."
    })
}

pub(super) fn execute(cli: &Cli, item_type: Option<&str>) {
    let matrix = derive();

    if let Some(requested) = item_type {
        let target = normalize(requested);
        if let Some(cap) = matrix.iter().find(|c| normalize(c.item_type) == target) {
            let mut out = cap.to_json();
            if let Value::Object(ref mut map) = out {
                map.insert("legend".to_string(), legend());
            }
            output::render_object(cli, &out, "type");
        } else {
            let available: Vec<&str> = matrix.iter().map(|c| c.item_type).collect();
            let result = json!({
                "error": format!("Unknown item type '{requested}'"),
                "available_types": available,
                "hint": "Run 'fabio context item-capabilities' (no type) for the full matrix."
            });
            output::render_object(cli, &result, "error");
        }
        return;
    }

    let rows: Vec<Value> = matrix.iter().map(Capability::to_json).collect();
    let result = json!({
        "item_capabilities": rows,
        "count": matrix.len(),
        "legend": legend(),
    });
    output::render_object(cli, &result, "item_capabilities");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_is_non_empty_and_well_formed() {
        let m = derive();
        assert!(m.len() >= 30, "expected the full item-type universe");
        for c in &m {
            assert!(
                matches!(
                    c.deploy_strategy,
                    "content" | "platform_only" | "unsupported"
                ),
                "{} has invalid strategy {}",
                c.item_type,
                c.deploy_strategy
            );
            // deployable_from_definition is exactly the content strategy.
            assert_eq!(
                c.deployable_from_definition,
                c.deploy_strategy == "content",
                "{} deployable_from_definition must equal (strategy==content)",
                c.item_type
            );
            // A definition_format is only ever reported for content types
            // (it's optional even then — some specs carry no format string).
            if c.deploy_strategy != "content" {
                assert!(
                    c.definition_format.is_none(),
                    "non-content type {} must not report a definition_format",
                    c.item_type
                );
            }
        }
    }

    #[test]
    fn every_known_item_type_maps_to_a_command_group() {
        // Guards the `creatable` derivation: a new item type without a resolvable
        // command group (via normalization or an alias) would silently report
        // creatable=false. Fail loudly instead so the alias table stays complete.
        let commands = super::super::agent_commands_schema();
        let groups = commands.as_object().unwrap();
        for t in crate::commands::item::known_item_types() {
            assert!(
                group_for(t, groups).is_some(),
                "item type '{t}' does not resolve to a command group — add a GROUP_ALIASES entry"
            );
        }
    }

    #[test]
    fn every_content_type_is_deployable() {
        // A type with a versionable definition must be in the deploy order
        // (content ⊆ deployable) — otherwise the strategy derivation is incoherent.
        for c in derive() {
            if c.deploy_strategy == "content" {
                assert!(
                    in_deploy_order(c.item_type),
                    "content type {} must be in DEPLOY_ORDER",
                    c.item_type
                );
            }
        }
    }

    #[test]
    fn content_types_support_the_definition_api() {
        // content ⊆ supports_definition: if fabio deploys a versionable
        // definition, it must also expose the get/update-definition commands.
        for c in derive() {
            if c.deploy_strategy == "content" {
                assert!(
                    c.supports_definition,
                    "content type {} must support the definition API (get/update-definition)",
                    c.item_type
                );
            }
        }
    }

    #[test]
    fn known_content_and_platform_only_types_are_classified() {
        let m = derive();
        let strat = |t: &str| {
            m.iter()
                .find(|c| c.item_type == t)
                .map(|c| c.deploy_strategy)
        };
        // Content: has a definition fabio deploys.
        assert_eq!(strat("Notebook"), Some("content"));
        assert_eq!(strat("SemanticModel"), Some("content"));
        assert_eq!(strat("SQLDatabase"), Some("content")); // via schema dacpac/sqlproj
        // Platform-only: data stores deployed as shells.
        assert_eq!(strat("Lakehouse"), Some("platform_only"));
        assert_eq!(strat("Warehouse"), Some("platform_only"));
    }
}
