use std::io::{Cursor, Read as _};
use std::process::{Command, Stdio};

use serde_json::json;

use super::*;

fn fixture() -> (tempfile::TempDir, Ledger) {
    let home = tempfile::TempDir::new().unwrap();
    let ledger = Ledger::new(home.path().join(".fabio").join("jobs.jsonl"));
    (home, ledger)
}

fn entry(id: &str) -> JobEntry {
    JobEntry::new(id, "mock-job", "mock-workspace", "mock-item")
}

#[test]
fn legacy_crlf_blank_lines_and_final_record_without_newline_are_supported() {
    let first = serde_json::to_string(&entry("first")).unwrap();
    let legacy = json!({"id":"second","kind":"notebook","status":"running","workspace":"mock","item_id":"mock","started_at":"2026-01-01T00:00:00Z","futureReceiptEvidence":{"verified":true}});
    let input = format!("\r\n{first}\r\n \r\n{legacy}");
    let entries = parse_entries(Cursor::new(input)).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].id, "first");
    assert_eq!(entries[1].target_ids.len(), 0);
    assert_eq!(
        serde_json::to_value(&entries[1]).unwrap()["futureReceiptEvidence"],
        json!({"verified":true})
    );
}

#[test]
fn damaged_records_are_errors_not_partial_or_empty_success() {
    let good = serde_json::to_string(&entry("first")).unwrap();
    let empty = serde_json::to_string(&entry("")).unwrap();
    for bad in [
        "{\"private-secret\":",
        "{\"id\":\"private-secret\"}",
        &good,
        &empty,
    ] {
        let error = parse_entries(Cursor::new(format!("{good}\n{bad}\n"))).unwrap_err();
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::InvalidInput);
        assert!(error.message.contains("line 2"));
        assert!(!error.to_string().contains("private-secret"));
        assert_eq!(error.retriable, Some(false));
    }
    let error = parse_entries(Cursor::new([good.as_bytes(), b"\n\xff\n"].concat())).unwrap_err();
    assert!(error.to_string().contains("line 2"));
}

#[test]
fn io_failure_after_a_valid_record_cannot_truncate_the_observed_history() {
    struct BrokenReader;
    impl io::Read for BrokenReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::other("private-secret-path"))
        }
    }
    let good = format!("{}\n", serde_json::to_string(&entry("first")).unwrap());
    let reader = BufReader::new(Cursor::new(good.into_bytes()).chain(BrokenReader));
    let error = parse_entries(reader).unwrap_err();
    assert!(error.to_string().contains("line 2"));
    assert!(!error.to_string().contains("private-secret-path"));
}

#[test]
fn edits_and_duplicate_appends_preserve_the_previous_snapshot_on_error() {
    let (_home, ledger) = fixture();
    ledger.append(&entry("first")).unwrap();
    let original = fs::read(&ledger.path).unwrap();
    assert!(ledger.append(&entry("first")).is_err());
    assert!(ledger.append(&entry("")).is_err());
    let error = ledger.edit(|entries| {
        entries.clear();
        Err::<(), _>(anyhow::anyhow!("injected edit failure"))
    });
    assert!(error.is_err());
    assert_eq!(fs::read(&ledger.path).unwrap(), original);
}

#[test]
fn corruption_blocks_both_append_and_edit_without_rewriting_any_bytes() {
    let (_home, ledger) = fixture();
    ledger.append(&entry("first")).unwrap();
    let damaged = format!(
        "{}{{\"private-secret\":",
        fs::read_to_string(&ledger.path).unwrap()
    );
    fs::write(&ledger.path, &damaged).unwrap();
    assert!(ledger.read_all().is_err());
    assert!(ledger.append(&entry("second")).is_err());
    assert!(
        ledger
            .edit(|entries| {
                entries.clear();
                Ok(())
            })
            .is_err()
    );
    assert_eq!(fs::read_to_string(&ledger.path).unwrap(), damaged);
}

#[test]
fn staging_failure_keeps_the_old_snapshot_and_cleans_unpublished_tempfile() {
    let (_home, ledger) = fixture();
    ledger.append(&entry("first")).unwrap();
    let original = fs::read(&ledger.path).unwrap();
    let _lock = ledger.lock(true, LOCK_TIMEOUT).unwrap();
    let result = ledger.write_atomic(&[entry("second")], |stage, _| {
        assert!(stage == CommitStage::Staged);
        Err(anyhow::anyhow!("injected pre-publication failure"))
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&ledger.path).unwrap(), original);
    assert_eq!(fs::read_dir(ledger.parent()).unwrap().count(), 2);
}

#[test]
fn publication_failure_does_not_remove_the_previous_destination() {
    let home = tempfile::TempDir::new().unwrap();
    let path = home.path().join("destination-directory");
    fs::create_dir(&path).unwrap();
    fs::write(path.join("old-evidence"), b"retain").unwrap();
    let ledger = Ledger::new(path.clone());
    assert!(ledger.write_atomic(&[entry("new")], |_, _| Ok(())).is_err());
    assert_eq!(fs::read(path.join("old-evidence")).unwrap(), b"retain");
}

#[test]
fn stable_sidecar_lock_survives_snapshot_replacement_and_waits_are_bounded() {
    let (_home, ledger) = fixture();
    ledger.append(&entry("first")).unwrap();
    let held = ledger.lock(true, LOCK_TIMEOUT).unwrap();
    ledger
        .write_atomic(&[entry("second")], |_, _| Ok(()))
        .unwrap();
    for exclusive in [false, true] {
        let start = Instant::now();
        let error = ledger
            .lock(exclusive, Duration::from_millis(20))
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<FabioError>().unwrap().code,
            ErrorCode::Conflict
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    drop(held);
    let first_reader = ledger.lock(false, LOCK_TIMEOUT).unwrap();
    let second_reader = ledger.lock(false, LOCK_TIMEOUT).unwrap();
    assert!(ledger.lock(true, Duration::ZERO).is_err());
    drop((first_reader, second_reader));
    assert_eq!(ledger.read_all().unwrap()[0].id, "second");
}

fn child(test: &str, ledger: &Ledger) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            &format!("commands::jobs::ledger::tests::{test}"),
            "--nocapture",
        ])
        .env("FABIO_UNIT_LEDGER_PATH", &ledger.path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

#[test]
fn abrupt_exit_before_or_after_publication_leaves_a_complete_recoverable_snapshot() {
    for stage in ["staged", "published"] {
        let (_home, ledger) = fixture();
        ledger.append(&entry("first")).unwrap();
        let output = child("crash_writer", &ledger)
            .env("FABIO_UNIT_LEDGER_CRASH_STAGE", stage)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(73), "{output:?}");
        let entries = ledger.read_all().unwrap();
        assert_eq!(entries.len(), if stage == "staged" { 1 } else { 2 });
        assert_eq!(entries[0].id, "first");
        // The child's lock is released even without destructors. Orphan staging
        // files remain private evidence and are never auto-promoted or deleted.
        ledger.append(&entry("after-crash")).unwrap();
        assert_eq!(ledger.read_all().unwrap().len(), entries.len() + 1);
        let orphans = fs::read_dir(ledger.parent())
            .unwrap()
            .flatten()
            .filter(|file| file.file_name().to_string_lossy().starts_with(".jobs-"))
            .count();
        assert_eq!(orphans, usize::from(stage == "staged"));
    }
}

#[test]
fn crash_writer() {
    let Some(path) = std::env::var_os("FABIO_UNIT_LEDGER_PATH") else {
        return;
    };
    let stage = std::env::var("FABIO_UNIT_LEDGER_CRASH_STAGE").unwrap();
    let ledger = Ledger::new(path.into());
    let _lock = ledger.lock(true, LOCK_TIMEOUT).unwrap();
    let mut entries = ledger.read_unlocked().unwrap();
    entries.push(entry("crash-write"));
    ledger
        .write_atomic(&entries, |observed, _| {
            if (stage == "staged" && observed == CommitStage::Staged)
                || (stage == "published" && observed == CommitStage::Published)
            {
                std::process::exit(73);
            }
            Ok(())
        })
        .unwrap();
    panic!("crash checkpoint was not reached");
}

#[test]
fn concurrent_processes_do_not_lose_unrelated_appends_or_updates() {
    let (_home, ledger) = fixture();
    ledger.append(&entry("initial")).unwrap();
    let children: Vec<_> = (0..4)
        .map(|worker| {
            child("concurrent_writer", &ledger)
                .env("FABIO_UNIT_LEDGER_WORKER", worker.to_string())
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
    }
    let entries = ledger.read_all().unwrap();
    assert_eq!(entries.len(), 33);
    assert_eq!(
        entries
            .iter()
            .filter(|entry| entry.status == "completed")
            .count(),
        32
    );
}

#[test]
fn concurrent_writer() {
    let Some(path) = std::env::var_os("FABIO_UNIT_LEDGER_PATH") else {
        return;
    };
    let worker = std::env::var("FABIO_UNIT_LEDGER_WORKER").unwrap();
    let ledger = Ledger::new(path.into());
    for index in 0..8 {
        let id = format!("worker-{worker}-{index}");
        ledger.append(&entry(&id)).unwrap();
        ledger
            .edit(|entries| {
                entries
                    .iter_mut()
                    .find(|entry| entry.id == id)
                    .unwrap()
                    .status = "completed".to_string();
                Ok(())
            })
            .unwrap();
        assert_ne!(ledger.read_all().unwrap().len(), 0);
    }
}

#[cfg(unix)]
#[test]
fn snapshots_locks_and_staged_files_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let (_home, ledger) = fixture();
    ledger.append(&entry("first")).unwrap();
    let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(ledger.parent()), 0o700);
    assert_eq!(mode(&ledger.path), 0o600);
    assert_eq!(mode(&ledger.path.with_extension("lock")), 0o600);
    ledger
        .write_atomic(&[entry("second")], |_, path| {
            assert_eq!(mode(path), 0o600);
            Ok(())
        })
        .unwrap();
}
