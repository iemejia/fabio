//! Deterministic guardrail audit for DESTRUCTIVE commands.
//!
//! Every `destructive: true` subcommand in `commands.json` (the source of truth
//! for agent-facing metadata) MUST ship the standard safety stack:
//!   1. It is annotated `mutates: true` (a destructive op mutates state).
//!   2. It honours `--dry-run` — running it with `--dry-run` never *completes* a
//!      mutation: the command either returns a dry-run preview (before any
//!      network call, or after a read-only expansion) or fails fast on input
//!      validation / auth — it must NEVER succeed (exit 0) without a dry-run
//!      marker.
//!
//! `destructive_commands_are_annotated_mutates` is a fast, hermetic metadata
//! check that runs in CI. `destructive_commands_dry_run_never_mutates` spawns
//! the binary once per destructive command with dummy required args and
//! `--dry-run`; it is `#[ignore]`d (it starts ~164 processes and read-modify-
//! write commands attempt a network read) — run it on demand with
//! `cargo test --test e2e_destructive_guardrails -- --ignored`.

use serde_json::Value;
use std::process::Command;

mod common;

/// Load `commands.json` from the source tree.
fn commands_json() -> Value {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/commands/context/data/agent/commands.json"
    );
    let raw = std::fs::read_to_string(path).expect("read commands.json");
    serde_json::from_str(&raw).expect("parse commands.json")
}

/// `(group, subcommand, subcommand-metadata)` for every `destructive: true` sub.
fn destructive_subcommands(root: &Value) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    for (group, gmeta) in root.as_object().expect("root object") {
        let Some(subs) = gmeta.get("subcommands").and_then(Value::as_object) else {
            continue;
        };
        for (sub, smeta) in subs {
            if smeta.get("destructive").and_then(Value::as_bool) == Some(true) {
                out.push((group.clone(), sub.clone(), smeta.clone()));
            }
        }
    }
    out
}

/// Build `--flag value` args for every REQUIRED flag with a type-appropriate
/// dummy value. Values that fail deeper validation (e.g. a UUID where an int is
/// expected) are fine — the command then fails fast (non-zero exit), which the
/// runtime assertion treats as "did not mutate".
fn dummy_required_args(smeta: &Value) -> Vec<String> {
    const DUMMY_UUID: &str = "00000000-0000-0000-0000-000000000000";
    let mut args = Vec::new();
    let Some(flags) = smeta.get("flags").and_then(Value::as_object) else {
        return args;
    };
    for (flag, fmeta) in flags {
        if fmeta.get("required").and_then(Value::as_bool) != Some(true) {
            continue;
        }
        let ty = fmeta
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("string");
        match ty {
            "bool" => args.push(flag.clone()),
            "integer" | "number" | "int" => {
                args.push(flag.clone());
                args.push("1".to_string());
            }
            _ => {
                args.push(flag.clone());
                let is_id = flag.contains("id") || flag.contains("workspace");
                args.push(if is_id { DUMMY_UUID } else { "x" }.to_string());
            }
        }
    }
    args
}

#[test]
fn destructive_commands_are_annotated_mutates() {
    let root = commands_json();
    let dest = destructive_subcommands(&root);
    assert!(
        dest.len() > 100,
        "sanity: expected many destructive commands, found {}",
        dest.len()
    );
    let gaps: Vec<String> = dest
        .iter()
        .filter(|(_, _, m)| m.get("mutates").and_then(Value::as_bool) != Some(true))
        .map(|(g, s, _)| format!("{g} {s}"))
        .collect();
    assert!(
        gaps.is_empty(),
        "{} destructive command(s) are NOT annotated mutates:true (a destructive op must \
         mutate state):\n  {}",
        gaps.len(),
        gaps.join("\n  ")
    );
}

#[test]
#[ignore = "spawns one process per destructive command; run on demand"]
fn destructive_commands_dry_run_never_mutates() {
    let root = commands_json();
    let dest = destructive_subcommands(&root);
    let bin = assert_cmd::cargo::cargo_bin("fabio");

    let mut failures = Vec::new();
    for (group, sub, smeta) in &dest {
        let mut args = vec![group.clone(), sub.clone()];
        args.extend(dummy_required_args(smeta));
        args.push("--dry-run".to_string());

        let output = Command::new(&bin)
            .args(&args)
            // A dummy static token avoids the interactive credential chain (no
            // hang in a non-TTY). Guarded commands return before any HTTP call;
            // read-modify-write commands get a fast 401 (still non-zero exit).
            .env("FABIO_ACCESS_TOKEN", "dummy-token-for-dry-run-audit")
            .env_remove("FABIO_SQL_ACCESS_TOKEN")
            .output()
            .expect("run fabio");

        // The invariant: if the command SUCCEEDED (exit 0), it MUST be a dry-run
        // preview — never a completed mutation.
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let compact: String = stdout.split_whitespace().collect();
            let is_dry_run =
                compact.contains("\"dry_run\":true") || compact.contains("\"status\":\"dry_run\"");
            if !is_dry_run {
                failures.push(format!(
                    "`{group} {sub}` exited 0 under --dry-run WITHOUT a dry-run marker \
                     (missing dry_run_guard?):\n    {}",
                    stdout.trim().chars().take(200).collect::<String>()
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "{} destructive command(s) may be missing a --dry-run guard:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
