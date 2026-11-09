//! Local-only receipt CLI tests. Never read or prune the developer's ledger.
use std::fmt::Write as _;
use std::fs;
use std::process::Stdio;
use std::time::{Duration, Instant};

use assert_cmd::Command;
use serde_json::{Value, json};

fn fabio(home: &tempfile::TempDir) -> Command {
    Command::from_std(process(home))
}

fn process(home: &tempfile::TempDir) -> std::process::Command {
    let mut command = std::process::Command::new(assert_cmd::cargo::cargo_bin("fabio"));
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("FABIO_NO_VERSION_CHECK", "1");
    command
}

fn entry(id: &str, status: &str) -> Value {
    json!({"id":id,"kind":"mock-job","status":status,"workspace":"mock-workspace",
        "item_id":"mock-item","started_at":"2026-01-01T00:00:00Z"})
}

fn fixture(entries: &[Value]) -> tempfile::TempDir {
    let home = tempfile::TempDir::new().unwrap();
    let directory = home.path().join(".fabio");
    fs::create_dir(&directory).unwrap();
    let mut contents = String::new();
    for value in entries {
        writeln!(contents, "{value}").unwrap();
    }
    fs::write(directory.join("jobs.jsonl"), contents).unwrap();
    home
}

fn data(output: &assert_cmd::assert::Assert) -> Value {
    serde_json::from_slice::<Value>(&output.get_output().stdout).unwrap()["data"].clone()
}

#[test]
fn jobs_list_returns_array_without_creating_a_missing_snapshot() {
    let home = tempfile::TempDir::new().unwrap();
    let output = fabio(&home).args(["jobs", "list"]).assert().success();
    assert_eq!(data(&output), json!([]));
    assert!(!home.path().join(".fabio").join("jobs.jsonl").exists());
}

#[test]
fn jobs_list_with_status_filter() {
    let home = fixture(&[entry("first", "running"), entry("second", "completed")]);
    let output = fabio(&home)
        .args(["jobs", "list", "--status", "completed"])
        .assert()
        .success();
    let result = data(&output);
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["id"], "second");
}

#[test]
fn jobs_get_nonexistent_fails() {
    let home = tempfile::TempDir::new().unwrap();
    let output = fabio(&home)
        .args(["jobs", "get", "--id", "missing"])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "NOT_FOUND");
}

#[test]
fn receipt_storage_safety_guidance_is_published_without_network_access() {
    let home = tempfile::TempDir::new().unwrap();
    let output = fabio(&home)
        .args(["--readonly", "context", "best-practices", "receipt-storage"])
        .assert()
        .success();
    let guidance = data(&output);
    assert_eq!(guidance["topic"], "receipt-storage");
    assert!(
        guidance["summary"]
            .as_str()
            .unwrap()
            .contains("never replay")
    );
}

#[test]
fn prune_is_transactional_and_preserves_every_uncertain_receipt_and_future_field() {
    let mut running = entry("running", "running");
    running["futureEvidence"] = json!({"verified":true});
    let home = fixture(&[
        running,
        entry("preparing", "preparing"),
        entry("accepted", "accepted"),
        entry("unknown", "unknown"),
        entry("completed", "completed"),
        entry("failed", "failed"),
        entry("future", "new-unrecognized-state"),
        entry("timeout", "timeout"),
    ]);
    let preview = fabio(&home)
        .args(["--readonly", "--dry-run", "jobs", "prune"])
        .assert()
        .success();
    assert_eq!(
        data(&preview)["details"]["ids"],
        json!(["completed", "failed"])
    );
    assert_eq!(data(&preview)["details"]["count"], 2);
    assert_eq!(data(&preview)["details"]["remaining"], 6);
    let output = fabio(&home).args(["jobs", "prune"]).assert().success();
    assert_eq!(
        data(&output),
        json!({"pruned":2,"remaining":6,"status":"pruned","destructive":true,"warnings":[]})
    );
    let output = fabio(&home)
        .args(["--readonly", "jobs", "get", "--id", "running"])
        .assert()
        .success();
    assert_eq!(data(&output)["futureEvidence"], json!({"verified":true}));
    let output = fabio(&home)
        .args(["jobs", "prune", "--include-running"])
        .assert()
        .success();
    assert_eq!(data(&output)["remaining"], 0);
    assert_eq!(data(&output)["destructive"], true);
    assert!(
        data(&output)["warnings"][0]
            .as_str()
            .unwrap()
            .contains("irreversibly")
    );
    assert_eq!(
        fs::read(home.path().join(".fabio").join("jobs.jsonl")).unwrap(),
        b""
    );
}

#[test]
fn readonly_and_destructive_dry_run_never_rewrite_receipts() {
    let home = fixture(&[entry("completed", "completed")]);
    let path = home.path().join(".fabio").join("jobs.jsonl");
    let original = fs::read(&path).unwrap();
    let output = fabio(&home)
        .args(["--readonly", "jobs", "prune"])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "READONLY_MODE");
    let output = fabio(&home)
        .env("OPENCODE", "1")
        .args([
            "--readonly",
            "--dry-run",
            "jobs",
            "prune",
            "--include-running",
        ])
        .assert()
        .success();
    let result = data(&output);
    assert_eq!(result["dry_run"], true);
    assert_eq!(result["destructive"], true);
    assert!(
        result["agentNotice"]
            .as_str()
            .unwrap()
            .contains("Confirm with the user")
    );
    assert_eq!(result["would_execute"], "jobs prune");
    assert_eq!(result["details"]["include_running"], true);
    assert_eq!(result["details"]["count"], 1);
    assert_eq!(result["details"]["ids"], json!(["completed"]));
    assert_eq!(fs::read(&path).unwrap(), original);
}

#[test]
fn corrupt_ledger_blocks_reads_and_pruning_without_disclosing_or_dropping_evidence() {
    let good = format!("{}\n", entry("first", "accepted"));
    for bad in [
        b"{\"private-secret\":".as_slice(),
        b"\xff",
        b"{}",
        good.as_bytes(),
    ] {
        let home = fixture(&[]);
        let path = home.path().join(".fabio").join("jobs.jsonl");
        let original = [good.as_bytes(), bad].concat();
        fs::write(&path, &original).unwrap();
        for args in [
            vec!["jobs", "list"],
            vec!["jobs", "get", "--id", "first"],
            vec!["jobs", "prune"],
            vec!["jobs", "prune", "--include-running"],
        ] {
            let output = fabio(&home).args(args).assert().failure();
            assert_eq!(output.get_output().stdout.len(), 0);
            let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
            assert_eq!(error["error"]["code"], "INVALID_INPUT");
            assert!(
                error["error"]["message"]
                    .as_str()
                    .unwrap()
                    .contains("line 2")
            );
            assert_eq!(error["error"]["retriable"], false);
            assert!(
                !String::from_utf8_lossy(&output.get_output().stderr).contains("private-secret")
            );
            assert_eq!(fs::read(&path).unwrap(), original);
        }
    }
}

#[test]
fn concurrent_cli_pruners_and_readers_observe_only_complete_snapshots() {
    let home = fixture(&[entry("running", "running"), entry("completed", "completed")]);
    let children: Vec<_> = (0..8)
        .map(|index| {
            let mut command = process(&home);
            command.args(["jobs", if index % 2 == 0 { "prune" } else { "list" }]);
            command
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        serde_json::from_slice::<Value>(&output.stdout).unwrap();
    }
    let output = fabio(&home).args(["jobs", "list"]).assert().success();
    let result = data(&output);
    assert_eq!(result.as_array().unwrap().len(), 1);
    assert_eq!(result[0]["id"], "running");
}

#[test]
fn busy_ledger_fails_with_a_bounded_conflict_without_unsafe_retry_advice() {
    let home = fixture(&[entry("first", "accepted")]);
    let path = home.path().join(".fabio").join("jobs.lock");
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .unwrap();
    lock.lock().unwrap();
    let start = Instant::now();
    let output = fabio(&home).args(["jobs", "list"]).assert().failure();
    assert!(start.elapsed() < Duration::from_secs(10));
    let error: Value = serde_json::from_slice(&output.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "CONFLICT");
    assert_eq!(error["error"]["retriable"], false);
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("Never delete jobs.lock")
    );
}
