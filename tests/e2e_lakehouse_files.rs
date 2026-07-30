//! End-to-end integration tests for `fabio lakehouse` file operations
//! (copy-file, move-file, delete-file, create-directory).

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;
use std::fs;
use tempfile::TempDir;

/// Helper: upload a test file and return its remote path.
fn upload_test_file(cfg: &TestConfig, name: &str, content: &str) -> String {
    let tmp_dir = TempDir::new().unwrap();
    let local_path = tmp_dir.path().join("file.txt");
    fs::write(&local_path, content).unwrap();
    let remote_path = format!("Files/{name}");

    fabio()
        .args([
            "lakehouse",
            "upload",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            local_path.to_str().unwrap(),
            "--dest-path",
            &remote_path,
        ])
        .assert()
        .success();

    remote_path
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_copy_file_across_workspaces() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("copy_src");
    let src_path = upload_test_file(&cfg, &format!("{name}.txt"), "copy test content");
    let dst_path = format!("Files/{name}_dest.txt");

    // Copy from source to dest lakehouse
    let assert = fabio()
        .args([
            "lakehouse",
            "copy-file",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-path",
            &src_path,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
            "--dest-path",
            &dst_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "copied");

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            &src_path,
        ])
        .assert()
        .success();

    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--path",
            &dst_path,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_move_file_within_workspace() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("move_src");
    let src_path = upload_test_file(&cfg, &format!("{name}.txt"), "move test content");
    let dst_path = format!("Files/{name}_moved.txt");

    // Move within same workspace/lakehouse
    let assert = fabio()
        .args([
            "lakehouse",
            "move-file",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-path",
            &src_path,
            "--dest-workspace",
            &cfg.source_workspace,
            "--dest-id",
            &cfg.source_lakehouse,
            "--dest-path",
            &dst_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");

    // Cleanup destination (source was already deleted by move)
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--path",
            &dst_path,
        ])
        .assert()
        .success();
}

// ─── create-directory ────────────────────────────────────────────────────────

#[test]
fn lakehouse_create_directory_missing_path() {
    fabio()
        .args([
            "lakehouse",
            "create-directory",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_directory_creates_and_cleanup() {
    let cfg = TestConfig::from_env();
    let dir_path = "Files/test-mkdir-e2e-auto";

    // Create the directory
    let assert = fabio()
        .args([
            "lakehouse",
            "create-directory",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            dir_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "created");
    assert_eq!(data["path"], dir_path);

    // Idempotent: creating again should also succeed (DFS PUT is idempotent)
    fabio()
        .args([
            "lakehouse",
            "create-directory",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            dir_path,
        ])
        .assert()
        .success();

    // Cleanup
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            dir_path,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_create_nested_directory() {
    let cfg = TestConfig::from_env();
    let dir_path = "Files/test-nested/sub1/sub2";

    // Create nested directory (DFS should create intermediate dirs)
    let assert = fabio()
        .args([
            "lakehouse",
            "create-directory",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            dir_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "created");

    // Cleanup: just delete leaf dirs bottom-up (best-effort)
    for path in &[
        "Files/test-nested/sub1/sub2",
        "Files/test-nested/sub1",
        "Files/test-nested",
    ] {
        let _ = fabio()
            .args([
                "lakehouse",
                "delete-file",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cfg.source_lakehouse,
                "--path",
                path,
            ])
            .assert();
    }
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_delete_file_nonexistent_returns_error() {
    let cfg = TestConfig::from_env();

    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--path",
            "Files/nonexistent_file_12345.txt",
        ])
        .assert()
        .failure();
}

// ---------------------------------------------------------------------------
// Copy file within same workspace/lakehouse
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_copy_file_within_same_lakehouse() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("samecopy");
    let src_path = upload_test_file(&cfg, &format!("{name}.txt"), "same lakehouse copy");
    let dst_path = format!("Files/{name}_dup.txt");

    // Copy within same workspace/lakehouse
    let assert = fabio()
        .args([
            "lakehouse",
            "copy-file",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-path",
            &src_path,
            "--dest-workspace",
            &cfg.source_workspace,
            "--dest-id",
            &cfg.source_lakehouse,
            "--dest-path",
            &dst_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "copied");

    // Both files should exist — download the copy to verify
    let tmp_dir = TempDir::new().unwrap();
    let dl_path = tmp_dir.path().join("downloaded.txt");
    fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &dst_path,
            "--dest-path",
            dl_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let content = fs::read_to_string(&dl_path).unwrap();
    assert_eq!(content, "same lakehouse copy");

    // Cleanup both
    for path in [&src_path, &dst_path] {
        fabio()
            .args([
                "lakehouse",
                "delete-file",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cfg.source_lakehouse,
                "--path",
                path,
            ])
            .assert()
            .success();
    }
}

// ---------------------------------------------------------------------------
// Move file across workspaces (verify source is gone, dest has content)
// ---------------------------------------------------------------------------

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn lakehouse_move_file_across_workspaces() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("xmove");
    let content = "cross workspace move content";
    let src_path = upload_test_file(&cfg, &format!("{name}.txt"), content);
    let dst_path = format!("Files/{name}_moved.txt");

    // Move from source to dest lakehouse
    let assert = fabio()
        .args([
            "lakehouse",
            "move-file",
            "--source-workspace",
            &cfg.source_workspace,
            "--source-id",
            &cfg.source_lakehouse,
            "--source-path",
            &src_path,
            "--dest-workspace",
            &cfg.dest_workspace,
            "--dest-id",
            &cfg.dest_lakehouse,
            "--dest-path",
            &dst_path,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "moved");

    // Verify source is gone (download should fail)
    let tmp_dir = TempDir::new().unwrap();
    let dl_path = tmp_dir.path().join("should_fail.txt");
    fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cfg.source_lakehouse,
            "--source-path",
            &src_path,
            "--dest-path",
            dl_path.to_str().unwrap(),
        ])
        .assert()
        .failure();

    // Verify dest has the content
    let dl_dest_path = tmp_dir.path().join("dest_content.txt");
    fabio()
        .args([
            "lakehouse",
            "download",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--source-path",
            &dst_path,
            "--dest-path",
            dl_dest_path.to_str().unwrap(),
        ])
        .assert()
        .success();

    let downloaded = fs::read_to_string(&dl_dest_path).unwrap();
    assert_eq!(downloaded, content);

    // Cleanup dest
    fabio()
        .args([
            "lakehouse",
            "delete-file",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &cfg.dest_lakehouse,
            "--path",
            &dst_path,
        ])
        .assert()
        .success();
}

#[test]
fn lakehouse_delete_directory_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "lakehouse",
            "delete-directory",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "00000000-0000-0000-0000-000000000000",
            "--path",
            "Files/staging",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "lakehouse delete-directory");
    assert_eq!(data["details"]["recursive"], true);
    assert_eq!(data["details"]["path"], "Files/staging");
}

#[test]
fn lakehouse_delete_directory_rejects_root_path() {
    // A recursive delete of the item root would wipe the whole lakehouse; it
    // must be refused before any network call (even without --dry-run).
    for path in ["/", ""] {
        let assert = fabio()
            .args([
                "lakehouse",
                "delete-directory",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "00000000-0000-0000-0000-000000000000",
                "--path",
                path,
            ])
            .assert()
            .failure();
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(
            stderr.contains("INVALID_INPUT") && stderr.contains("item root"),
            "root path {path:?} should be refused: {stderr}"
        );
    }
}
