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
//! `--dry-run`; it is `#[ignore]`d (it starts many processes and read-modify-
//! write commands attempt mock reads) — run it on demand with
//! `cargo test --test e2e_destructive_guardrails -- --ignored`.

use serde_json::Value;
use std::path::Path;

mod common;

/// Load `commands.json` from the source tree.
fn commands_json() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("commands")
        .join("context")
        .join("data")
        .join("agent")
        .join("commands.json");
    let raw = std::fs::read_to_string(path).expect("read commands.json");
    serde_json::from_str(&raw).expect("parse commands.json")
}

/// `(group, subcommand, subcommand-metadata)` for every `destructive: true` sub.
fn destructive_subcommands(root: &Value) -> Vec<(String, String, Value)> {
    let mut out = Vec::new();
    collect_destructive(root, "", &mut out);
    out
}

fn collect_destructive(nodes: &Value, parent: &str, out: &mut Vec<(String, String, Value)>) {
    for (name, metadata) in nodes.as_object().expect("command map") {
        let path = if parent.is_empty() {
            name.clone()
        } else {
            format!("{parent} {name}")
        };
        if metadata["destructive"] == true {
            let (group, rest) = path.split_once(' ').unwrap_or((&path, ""));
            out.push((group.to_string(), rest.to_string(), metadata.clone()));
        }
        if let Some(children) = metadata.get("subcommands") {
            collect_destructive(children, &path, out);
        }
    }
}

fn isolated_command(
    server: &wiremock::MockServer,
    home: &tempfile::TempDir,
) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .current_dir(home.path())
        .timeout(std::time::Duration::from_secs(10))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_READONLY", "false")
        .env("FABIO_WORKSPACE", "00000000-0000-0000-0000-000000000000")
        .env("FABIO_CAPACITY_ID", "00000000-0000-0000-0000-000000000000")
        .env(
            "AZURE_SUBSCRIPTION_ID",
            "00000000-0000-0000-0000-000000000000",
        )
        .env_remove("FABIO_PROFILE")
        .env("FABIO_NO_VERSION_CHECK", "1")
        .env("FABIO_POWERBI_ENDPOINT", server.uri())
        .env("FABIO_ARM_ENDPOINT", server.uri())
        .env("FABIO_ONELAKE_DFS_ENDPOINT", server.uri())
        .env("FABIO_ONELAKE_BLOB_ENDPOINT", server.uri())
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .env("HTTPS_PROXY", server.uri())
        .env("HTTP_PROXY", server.uri())
        .env("ALL_PROXY", server.uri())
        .env("NO_PROXY", "localhost,127.0.0.1,::1")
        .env("FABIO_SQL_ACCESS_TOKEN", "dummy-test-token")
        .env("FABIO_STORAGE_ACCESS_TOKEN", "dummy-test-token")
        .env("FABIO_ARM_ACCESS_TOKEN", "dummy-test-token")
        .env("FABIO_GRAPH_ACCESS_TOKEN", "dummy-test-token")
        .env("FABIO_COSMOS_ACCESS_TOKEN", "dummy-test-token");
    command
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns one process per destructive command; run on demand"]
async fn destructive_commands_dry_run_never_mutates() {
    let server = wiremock::MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let root = commands_json();
    let dest = destructive_subcommands(&root);

    let mut failures = Vec::new();
    let mut unexpected_requests = Vec::new();
    for (group, sub, smeta) in &dest {
        let mut args = vec![group.clone()];
        args.extend(sub.split_whitespace().map(str::to_string));
        args.extend(dummy_required_args(smeta));
        args.push("--dry-run".to_string());

        let before = server.received_requests().await.unwrap().len();
        let output = isolated_command(&server, &home)
            .args(&args)
            // A dummy static token avoids the interactive credential chain (no
            // hang in a non-TTY). Guarded commands return before any HTTP call;
            // read-modify-write commands get a fast mock rejection (non-zero).
            .env("FABIO_ACCESS_TOKEN", "dummy-token-for-dry-run-audit")
            .output()
            .expect("run fabio");
        for request in server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .skip(before)
        {
            // Definition retrieval and DAX executeQueries are explicitly
            // read-only POSTs used while expanding a read-modify-write scope.
            if !matches!(request.method.as_str(), "GET" | "HEAD" | "CONNECT")
                && !request.url.path().ends_with("/getDefinition")
                && !request.url.path().ends_with("/executeQueries")
            {
                let rpc = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|value| value["method"].as_str().map(str::to_string));
                unexpected_requests.push(format!(
                    "{group} {sub}: {} {} rpc={rpc:?}",
                    request.method,
                    request.url.path()
                ));
            }
        }

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
    assert_eq!(
        unexpected_requests,
        Vec::<String>::new(),
        "A dry-run attempted a mutating mock request"
    );
}

/// Whenever a destructive command's `--dry-run` guard actually fires offline
/// (exit 0 with a dry-run marker), its preview MUST carry `"destructive": true`
/// AND — because we run with an AI-agent env var set — an `agentNotice` telling
/// the agent to confirm the irreversible action with the user.
///
/// This validates the destructive-signal end-to-end for every destructive
/// command whose guard is reachable without a network call: it proves the
/// operation string each command passes to `dry_run_guard` matches its
/// `commands.json` key (a mismatch would yield a dry-run preview WITHOUT the
/// destructive marker, which fails here). Read-modify-write commands whose guard
/// fires only after a network read get a fast mock rejection (no dry-run marker),
/// so they are counted as "not observable offline" rather than asserted — their
/// recognition is covered deterministically by the `agent.rs` unit test
/// `every_destructive_command_is_recognized`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "spawns one process per destructive command; run on demand"]
async fn destructive_dry_run_previews_carry_confirm_signal() {
    let server = wiremock::MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let root = commands_json();
    let dest = destructive_subcommands(&root);

    let mut observed = 0usize;
    let mut failures = Vec::new();
    for (group, sub, smeta) in &dest {
        let mut args = vec![group.clone()];
        args.extend(sub.split_whitespace().map(str::to_string));
        args.extend(dummy_required_args(smeta));
        args.push("--dry-run".to_string());

        let output = isolated_command(&server, &home)
            .args(&args)
            .env("FABIO_ACCESS_TOKEN", "dummy-token-for-dry-run-audit")
            // Present an AI agent so the confirm-with-user notice fires.
            .env("CLAUDE_CODE", "1")
            .output()
            .expect("run fabio");

        if !output.status.success() {
            continue; // fast auth/validation failure — guard not reachable offline
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let compact: String = stdout.split_whitespace().collect();
        let is_dry_run =
            compact.contains("\"dry_run\":true") || compact.contains("\"status\":\"dry_run\"");
        if !is_dry_run {
            continue; // handled by destructive_commands_dry_run_never_mutates
        }
        observed += 1;
        // A registered action-scoped preview may prove that this selected plan
        // contains no destructive actions. This exception is not available to
        // arbitrary commands that forgot their unconditional marker.
        if smeta["destructive_scope"] == "actions" {
            let value: Value = serde_json::from_slice(&output.stdout).expect("JSON preview");
            if value["data"]["destructive"] == false {
                continue;
            }
        }
        if !compact.contains("\"destructive\":true") {
            failures.push(format!(
                "`{group} {sub}` dry-run preview is MISSING \"destructive\":true \
                 (operation string likely does not match its commands.json key)"
            ));
        }
        if !compact.contains("\"agentNotice\":") {
            failures.push(format!(
                "`{group} {sub}` dry-run preview is MISSING agentNotice with an agent present"
            ));
        }
    }

    assert!(
        observed > 20,
        "sanity: expected to observe many offline destructive dry-run previews, saw {observed}"
    );
    assert!(
        failures.is_empty(),
        "{} destructive dry-run preview(s) missing the confirm signal:\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}
