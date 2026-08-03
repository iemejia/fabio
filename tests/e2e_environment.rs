//! End-to-end integration tests for `fabio environment` commands.

mod common;

use common::{TestConfig, extract_data, fabio, parse_json};
use serial_test::serial;

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_list_returns_array() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args(["environment", "list", "--workspace", &cfg.source_workspace])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert!(data.is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_create_and_delete() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_test");

    // Create
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], name);
    let env_id = data["id"].as_str().unwrap().to_string();

    // Delete
    let assert = fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["status"], "deleted");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_update_name() {
    let cfg = TestConfig::from_env();
    let original = common::unique_name("env_upd_o");
    let updated = common::unique_name("env_upd_n");

    // Create
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.dest_workspace,
            "--name",
            &original,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    let env_id = data["id"].as_str().unwrap().to_string();

    // Update
    let assert = fabio()
        .args([
            "environment",
            "update",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
            "--name",
            &updated,
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["displayName"], updated);

    // Cleanup
    fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.dest_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_update_requires_field() {
    let cfg = TestConfig::from_env();

    let assert = fabio()
        .args([
            "environment",
            "update",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            "00000000-0000-0000-0000-000000000000",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let err_json: serde_json::Value = serde_json::from_str(&stderr).unwrap();
    assert_eq!(err_json["error"]["code"], "INVALID_INPUT");
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-dry-run",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "environment create");
}

// ─── Upload Staging Library ─────────────────────────────────────────────────

#[test]
fn environment_upload_staging_library_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "Cargo.toml",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(data["would_execute"], "environment upload-staging-library");
    assert_eq!(data["details"]["libraryName"], "Cargo.toml");
    assert!(data["details"]["sizeBytes"].as_u64().unwrap() > 0);
}

#[test]
fn environment_upload_staging_library_custom_name_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "Cargo.toml",
            "--library-name",
            "my_lib-1.0.0.whl",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["details"]["libraryName"], "my_lib-1.0.0.whl");
}

#[test]
fn environment_upload_staging_library_missing_file() {
    let assert = fabio()
        .args([
            "environment",
            "upload-staging-library",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            "/nonexistent/path/lib.whl",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("Failed to read file"));
}

// ─── Staging Spark Compute (runtime version + spark properties) ──────────────

#[test]
fn environment_update_staging_spark_compute_typed_dry_run() {
    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--runtime-version",
            "2.0",
            "--spark-property",
            "spark.native.enabled=true",
        ])
        .assert()
        .success();

    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "environment update-staging-spark-compute"
    );
    assert_eq!(data["details"]["runtimeVersion"], "2.0");
    assert_eq!(
        data["details"]["sparkProperties"]["spark.native.enabled"],
        "true"
    );
}

#[test]
fn environment_update_staging_spark_compute_requires_input() {
    // Neither --file/--content nor --runtime-version/--spark-property provided.
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("--runtime-version"));
}

#[test]
fn environment_update_staging_spark_compute_content_conflicts_with_typed() {
    // clap should reject combining raw JSON with typed override flags.
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--content",
            "{}",
            "--runtime-version",
            "2.0",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("cannot be used with"));
}

#[test]
fn environment_update_staging_spark_compute_rejects_bad_property() {
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--spark-property",
            "no-equals-sign",
        ])
        .assert()
        .failure();

    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("KEY=VALUE"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_staging_spark_compute_runtime_and_properties_lifecycle() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_spark_rt");

    // Create a throwaway environment.
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let env_id = extract_data(&json)["id"].as_str().unwrap().to_string();

    // Set runtime 2.0 + a spark property (read-merge-write).
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--runtime-version",
            "2.0",
            "--spark-property",
            "spark.native.enabled=true",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");
    assert_eq!(data["sparkProperties"]["spark.native.enabled"], "true");

    // Add a second property; runtime + first property must be preserved (merge).
    let assert = fabio()
        .args([
            "environment",
            "update-staging-spark-compute",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--spark-property",
            "spark.remote.shuffle.enabled=true",
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");
    assert_eq!(data["sparkProperties"]["spark.native.enabled"], "true");
    assert_eq!(
        data["sparkProperties"]["spark.remote.shuffle.enabled"],
        "true"
    );

    // Read back via get-staging-spark-settings.
    let assert = fabio()
        .args([
            "environment",
            "get-staging-spark-settings",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["runtimeVersion"], "2.0");

    // Clean up.
    fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
}

// ─── External libraries (import/export environment.yml) ──────────────────────

#[test]
fn environment_import_staging_libraries_dry_run() {
    let dir = tempfile::tempdir().unwrap();
    let yml = dir.path().join("environment.yml");
    std::fs::write(&yml, "dependencies:\n  - pip:\n    - requests==2.32.3\n").unwrap();

    let assert = fabio()
        .args([
            "--dry-run",
            "environment",
            "import-staging-libraries",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
            "--file",
            yml.to_str().unwrap(),
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    assert_eq!(data["dry_run"], true);
    assert_eq!(
        data["would_execute"],
        "environment import-staging-libraries"
    );
    assert!(data["details"]["contentLength"].as_u64().unwrap() > 0);
}

#[test]
fn environment_import_staging_libraries_requires_input() {
    let assert = fabio()
        .args([
            "environment",
            "import-staging-libraries",
            "--workspace",
            "aaaaaaaa-1111-2222-3333-444444444444",
            "--id",
            "bbbbbbbb-1111-2222-3333-444444444444",
        ])
        .assert()
        .failure();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("--file") || stderr.contains("--content"));
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn environment_external_libraries_roundtrip() {
    let cfg = TestConfig::from_env();
    let name = common::unique_name("env_extlibs");

    // Create a throwaway environment.
    let assert = fabio()
        .args([
            "environment",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            &name,
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    let env_id = extract_data(&parse_json(&assert))["id"]
        .as_str()
        .unwrap()
        .to_string();

    // Import an environment.yml (octet-stream, not JSON).
    let yml = "dependencies:\n  - pip:\n    - requests==2.32.3\n";
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("environment.yml");
    std::fs::write(&path, yml).unwrap();
    let assert = fabio()
        .args([
            "environment",
            "import-staging-libraries",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
            "--file",
            path.to_str().unwrap(),
        ])
        .timeout(std::time::Duration::from_mins(2))
        .assert()
        .success();
    assert_eq!(
        extract_data(&parse_json(&assert))["status"],
        "libraries_imported"
    );

    // Export it back and confirm the content round-trips (raw text, not JSON).
    let assert = fabio()
        .args([
            "environment",
            "export-staging-libraries",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = extract_data(&json);
    let exported = data["externalLibraries"].as_str().unwrap();
    assert!(
        exported.contains("dependencies") && exported.contains("requests"),
        "expected the imported environment.yml back, got: {exported}"
    );

    // Clean up.
    fabio()
        .args([
            "environment",
            "delete",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &env_id,
        ])
        .assert()
        .success();
}
