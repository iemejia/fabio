//! Generator safety judgment and a separate CLI-to-published-metadata gate.
//! The expected command set comes from clap, never destructive:true annotations.

use std::collections::{BTreeMap, HashSet};
use std::sync::LazyLock;

use clap::CommandFactory as _;
use serde::Deserialize;

#[derive(Deserialize)]
struct Policy {
    destructive_verbs: HashSet<String>,
    destructive_commands: BTreeMap<String, String>,
    destructive_flags: HashSet<String>,
    action_scoped_commands: HashSet<String>,
    non_destructive_commands: BTreeMap<String, String>,
}

static POLICY: LazyLock<Policy> = LazyLock::new(|| {
    serde_json::from_str(include_str!("data/agent/safety_policy.json"))
        .expect("valid safety policy")
});

#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Classification {
    pub destructive: bool,
    pub destructive_flags: Vec<String>,
    pub action_scoped: bool,
}

pub(super) fn classify(path: &str, command: &clap::Command) -> Classification {
    if POLICY.non_destructive_commands.contains_key(path) {
        return Classification::default();
    }
    let name = command.get_name();
    let destructive = name == "update-definition"
        || name
            .split('-')
            .any(|word| POLICY.destructive_verbs.contains(word))
        || POLICY.destructive_commands.contains_key(path);
    let mut flags: Vec<_> = command
        .get_arguments()
        .filter(|arg| !arg.is_global_set())
        .filter_map(|arg| arg.get_long())
        .map(|flag| format!("--{flag}"))
        .filter(|flag| POLICY.destructive_flags.contains(flag))
        .collect();
    flags.sort();
    Classification {
        destructive,
        destructive_flags: flags,
        action_scoped: POLICY.action_scoped_commands.contains(path),
    }
}

fn with_cli(check: impl FnOnce(clap::Command) + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || check(crate::cli::Cli::command()))
        .unwrap()
        .join()
        .unwrap();
}

fn visit_cli(command: &clap::Command, path: &str, visit: &mut impl FnMut(&str, &clap::Command)) {
    for child in command
        .get_subcommands()
        .filter(|child| child.get_name() != "help")
    {
        let path = if path.is_empty() {
            child.get_name().to_string()
        } else {
            format!("{path} {}", child.get_name())
        };
        visit(&path, child);
        visit_cli(child, &path, visit);
    }
}

fn metadata<'a>(root: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut segments = path.split_whitespace();
    let mut node = root.get(segments.next()?)?;
    for segment in segments {
        node = node.get("subcommands")?.get(segment)?;
    }
    Some(node)
}

fn action_scope_is_reviewed(path: &str, metadata: &serde_json::Value) -> bool {
    metadata
        .get("destructive_scope")
        .is_none_or(|scope| scope == "actions" && POLICY.action_scoped_commands.contains(path))
}

#[test]
fn published_safety_metadata_covers_the_actual_cli_not_just_already_annotated_commands() {
    with_cli(|cli| {
        let schema: serde_json::Value =
            serde_json::from_str(include_str!("data/agent/commands.json")).unwrap();
        let mut gaps = Vec::new();
        visit_cli(&cli, "", &mut |path, command| {
            let required = classify(path, command);
            let published = metadata(&schema, path);
            if published.is_some_and(|metadata| !action_scope_is_reviewed(path, metadata)) {
                gaps.push(format!(
                    "{path}: unreviewed action-scoped preview exception"
                ));
            }
            if !required.destructive && required.destructive_flags.is_empty() {
                return;
            }
            let Some(published) = published else {
                gaps.push(format!("{path}: missing metadata"));
                return;
            };
            if published["mutates"] != true {
                gaps.push(format!("{path}: must be mutates:true"));
            }
            if required.destructive && published["destructive"] != true {
                gaps.push(format!("{path}: must be destructive:true"));
            }
            if required.action_scoped && published["destructive_scope"] != "actions" {
                gaps.push(format!("{path}: missing action-scoped preview contract"));
            }
            if !required.destructive && published["destructive"] != true {
                let expected = serde_json::json!(required.destructive_flags);
                if published["destructive_flags"] != expected {
                    gaps.push(format!(
                        "{path}: missing flag-conditional destructive metadata"
                    ));
                }
            }
        });
        assert_eq!(
            gaps,
            Vec::<String>::new(),
            "Regenerate commands.json from the safety policy; omissions must not escape by lacking destructive:true."
        );
    });
}

#[test]
fn policy_judgment_references_real_commands_and_has_review_rationale() {
    with_cli(|cli| {
        let mut paths = HashSet::new();
        visit_cli(&cli, "", &mut |path, _| {
            paths.insert(path.to_string());
        });
        for (path, reason) in POLICY
            .destructive_commands
            .iter()
            .chain(POLICY.non_destructive_commands.iter())
        {
            assert!(paths.contains(path), "Stale safety policy path: {path}");
            assert_ne!(
                reason.trim().len(),
                0,
                "Explain the safety judgment for {path}"
            );
        }
        for path in &POLICY.action_scoped_commands {
            assert!(
                paths.contains(path) && POLICY.destructive_commands.contains_key(path),
                "Action-scoped exception needs a real reviewed destructive command: {path}"
            );
        }
    });
}

#[test]
fn future_commands_cannot_hide_behind_missing_annotations_or_global_flags() {
    for name in [
        "update-definition",
        "statistics-delete",
        "cancel-instance",
        "queries-kill",
        "stop",
        "checkout",
        "reset",
        "vacuum-table",
        "prune",
    ] {
        assert!(classify(&format!("future {name}"), &clap::Command::new(name)).destructive);
    }
    let command = clap::Command::new("import")
        .arg(
            clap::Arg::new("drop")
                .long("drop-if-exists")
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("global")
                .long("overwrite")
                .global(true)
                .action(clap::ArgAction::SetTrue),
        );
    let class = classify("future import", &command);
    assert!(!class.destructive);
    assert_eq!(class.destructive_flags, ["--drop-if-exists"]);
    let apply = classify("deploy apply", &clap::Command::new("apply"));
    assert!(apply.destructive && apply.action_scoped);
    assert!(action_scope_is_reviewed(
        "deploy apply",
        &serde_json::json!({"destructive_scope":"actions"})
    ));
    assert!(!action_scope_is_reviewed(
        "future delete",
        &serde_json::json!({"destructive_scope":"actions"})
    ));
    assert!(!action_scope_is_reviewed(
        "deploy apply",
        &serde_json::json!({"destructive_scope":"unknown"})
    ));
    assert!(classify("dataflow upgrade-gen1", &clap::Command::new("upgrade-gen1")).destructive);
    assert_eq!(classify("deploy plan", &command), Classification::default());
    assert_eq!(
        classify("operations-agent stop", &clap::Command::new("stop")),
        Classification::default()
    );
}
