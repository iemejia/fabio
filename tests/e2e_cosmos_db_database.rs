use assert_cmd::Command;
use serial_test::serial;

mod common;
use common::TestConfig;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

fn parse(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    serde_json::from_str(&stdout).unwrap()
}

fn parse_err(assert: &assert_cmd::assert::Assert) -> serde_json::Value {
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    serde_json::from_str(&stderr).unwrap()
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn cosmos_db_database_list_returns_array() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "cosmos-db-database",
            "list",
            "--workspace",
            &cfg.source_workspace,
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"].is_array());
}

#[test]
#[ignore = "requires live Fabric tenant"]
#[serial]
fn cosmos_db_database_dry_run_create() {
    let cfg = TestConfig::from_env();
    let assert = fabio()
        .args([
            "cosmos-db-database",
            "create",
            "--workspace",
            &cfg.source_workspace,
            "--name",
            "test-cosmos",
            "--dry-run",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["would_execute"], "cosmos-db-database create");
}

// ── Data-plane: hermetic guards (no network — dry-run/validation fire first) ──

#[test]
fn cosmos_create_container_dry_run_previews_without_network() {
    let json = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "create-container",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "11111111-1111-1111-1111-111111111111",
                "--container",
                "products",
                "--partition-key",
                "categoryId",
                "--dry-run",
            ])
            .assert()
            .success(),
    );
    assert_eq!(
        json["data"]["would_execute"],
        "cosmos-db-database create-container"
    );
    // Leading slash is normalized into the preview.
    assert_eq!(json["data"]["details"]["partitionKey"], "/categoryId");
}

#[test]
fn cosmos_delete_container_dry_run_is_destructive() {
    let json = parse(
        &fabio()
            .env("CLAUDECODE", "1")
            .args([
                "cosmos-db-database",
                "delete-container",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "11111111-1111-1111-1111-111111111111",
                "--container",
                "products",
                "--dry-run",
            ])
            .assert()
            .success(),
    );
    assert_eq!(
        json["data"]["would_execute"],
        "cosmos-db-database delete-container"
    );
    assert_eq!(json["data"]["destructive"], true);
    assert!(json["data"]["agentNotice"].is_string());
}

#[test]
fn cosmos_delete_container_rejects_blast_radius() {
    let assert = fabio()
        .args([
            "cosmos-db-database",
            "delete-container",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--container",
            "",
        ])
        .assert()
        .failure();
    let json = parse_err(&assert);
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
}

#[test]
fn cosmos_import_dry_run_counts_documents() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fabio_cosmos_import_{}.jsonl", std::process::id()));
    std::fs::write(
        &path,
        "{\"id\":\"a\",\"pk\":\"x\"}\n{\"id\":\"b\",\"pk\":\"y\"}\n",
    )
    .unwrap();
    let json = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "import",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "11111111-1111-1111-1111-111111111111",
                "--container",
                "products",
                "--source",
                path.to_str().unwrap(),
                "--dry-run",
            ])
            .assert()
            .success(),
    );
    assert_eq!(json["data"]["would_execute"], "cosmos-db-database import");
    assert_eq!(json["data"]["details"]["documentCount"], 2);
    assert_eq!(json["data"]["details"]["mode"], "upsert");
    std::fs::remove_file(&path).ok();
}

#[test]
fn cosmos_delete_document_dry_run_is_destructive() {
    let json = parse(
        &fabio()
            .env("CLAUDECODE", "1")
            .args([
                "cosmos-db-database",
                "delete-document",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "11111111-1111-1111-1111-111111111111",
                "--container",
                "products",
                "--document-id",
                "p1",
                "--partition-key",
                "electronics",
                "--dry-run",
            ])
            .assert()
            .success(),
    );
    assert_eq!(
        json["data"]["would_execute"],
        "cosmos-db-database delete-document"
    );
    assert_eq!(json["data"]["destructive"], true);
    assert!(json["data"]["agentNotice"].is_string());
}

#[test]
fn cosmos_delete_document_rejects_empty_id() {
    let assert = fabio()
        .args([
            "cosmos-db-database",
            "delete-document",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--container",
            "products",
            "--document-id",
            "",
            "--partition-key",
            "electronics",
        ])
        .assert()
        .failure();
    let json = parse_err(&assert);
    assert_eq!(json["error"]["code"], "INVALID_INPUT");
}

#[test]
fn cosmos_create_document_dry_run_previews() {
    let json = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "create-document",
                "--workspace",
                "00000000-0000-0000-0000-000000000000",
                "--id",
                "11111111-1111-1111-1111-111111111111",
                "--container",
                "products",
                "--content",
                "{\"id\":\"p9\",\"categoryId\":\"x\"}",
                "--dry-run",
            ])
            .assert()
            .success(),
    );
    assert_eq!(
        json["data"]["would_execute"],
        "cosmos-db-database create-document"
    );
    assert_eq!(json["data"]["details"]["documentId"], "p9");
    assert_eq!(json["data"]["details"]["mode"], "upsert");
}

#[test]
fn cosmos_import_readonly_is_blocked() {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fabio_cosmos_ro_{}.jsonl", std::process::id()));
    std::fs::write(&path, "{\"id\":\"a\",\"pk\":\"x\"}\n").unwrap();
    let assert = fabio()
        .args([
            "--readonly",
            "cosmos-db-database",
            "import",
            "--workspace",
            "00000000-0000-0000-0000-000000000000",
            "--id",
            "11111111-1111-1111-1111-111111111111",
            "--container",
            "products",
            "--source",
            path.to_str().unwrap(),
        ])
        .assert()
        .failure();
    let json = parse_err(&assert);
    assert_eq!(json["error"]["code"], "READONLY_MODE");
    std::fs::remove_file(&path).ok();
}

// ── Data-plane: live lifecycle (gated on FABIO_TEST_COSMOS_ID) ────────────────

/// End-to-end data-plane lifecycle: create a container, import JSONL, query it
/// back, then delete the container. Requires an existing Cosmos DB database item
/// (`FABIO_TEST_COSMOS_ID`) in the source workspace on an F4+ capacity.
#[test]
#[ignore = "requires live Fabric tenant + FABIO_TEST_COSMOS_ID"]
#[serial]
fn cosmos_data_plane_lifecycle() {
    let cfg = TestConfig::from_env();
    let Ok(cosmos_id) = std::env::var("FABIO_TEST_COSMOS_ID") else {
        eprintln!("skipping: FABIO_TEST_COSMOS_ID not set");
        return;
    };
    let container = format!("e2e_{}", std::process::id());

    // 1. create-container
    fabio()
        .args([
            "cosmos-db-database",
            "create-container",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cosmos_id,
            "--container",
            &container,
            "--partition-key",
            "categoryId",
        ])
        .assert()
        .success();

    // 2. import a small JSONL batch
    let dir = std::env::temp_dir();
    let path = dir.join(format!("fabio_cosmos_e2e_{}.jsonl", std::process::id()));
    std::fs::write(
        &path,
        "{\"id\":\"p1\",\"categoryId\":\"a\",\"price\":10}\n{\"id\":\"p2\",\"categoryId\":\"b\",\"price\":200}\n",
    )
    .unwrap();
    let imp = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "import",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cosmos_id,
                "--container",
                &container,
                "--source",
                path.to_str().unwrap(),
            ])
            .assert()
            .success(),
    );
    assert_eq!(imp["data"]["documentsImported"], 2);
    assert_eq!(imp["data"]["status"], "imported");
    std::fs::remove_file(&path).ok();

    // 3. query it back (cross-partition, parameterized)
    let q = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "query",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cosmos_id,
                "--container",
                &container,
                "--query-text",
                "SELECT c.id FROM c WHERE c.price > @min",
                "--parameter",
                "min=100",
            ])
            .assert()
            .success(),
    );
    let ids: Vec<&str> = q["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["p2"], "only p2 has price > 100");

    // 4. list-containers includes the new one
    let list = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "list-containers",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cosmos_id,
            ])
            .assert()
            .success(),
    );
    assert!(
        list["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == container)
    );

    // 5. show-container reports the partition-key path
    let sc = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "show-container",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cosmos_id,
                "--container",
                &container,
            ])
            .assert()
            .success(),
    );
    assert_eq!(sc["data"]["partitionKey"]["paths"][0], "/categoryId");

    // 6. create-document (derive partition key from the doc), then get-document
    fabio()
        .args([
            "cosmos-db-database",
            "create-document",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cosmos_id,
            "--container",
            &container,
            "--content",
            "{\"id\":\"p3\",\"categoryId\":\"c\",\"price\":50}",
        ])
        .assert()
        .success();
    let got = parse(
        &fabio()
            .args([
                "cosmos-db-database",
                "get-document",
                "--workspace",
                &cfg.source_workspace,
                "--id",
                &cosmos_id,
                "--container",
                &container,
                "--document-id",
                "p3",
                "--partition-key",
                "c",
            ])
            .assert()
            .success(),
    );
    assert_eq!(got["data"]["price"], 50);

    // 7. export strips system fields (clean, re-importable JSONL)
    let out = dir.join(format!("fabio_cosmos_export_{}.jsonl", std::process::id()));
    fabio()
        .args([
            "cosmos-db-database",
            "export",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cosmos_id,
            "--container",
            &container,
            "--output-file",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let exported = std::fs::read_to_string(&out).unwrap();
    assert!(
        !exported.contains("_rid") && !exported.contains("_etag"),
        "export must strip Cosmos system metadata fields"
    );
    assert_eq!(exported.lines().count(), 3, "3 documents exported");
    std::fs::remove_file(&out).ok();

    // 8. delete-document, then confirm it is gone
    fabio()
        .args([
            "cosmos-db-database",
            "delete-document",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cosmos_id,
            "--container",
            &container,
            "--document-id",
            "p3",
            "--partition-key",
            "c",
        ])
        .assert()
        .success();

    // 9. delete-container (cleanup)
    fabio()
        .args([
            "cosmos-db-database",
            "delete-container",
            "--workspace",
            &cfg.source_workspace,
            "--id",
            &cosmos_id,
            "--container",
            &container,
        ])
        .assert()
        .success();
}
