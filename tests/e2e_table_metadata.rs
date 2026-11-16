//! Safe, storage-authenticated `OneLake` table and column metadata editing.
mod common;

use serde_json::{Value, json};
use wiremock::matchers::{body_json, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const WS: &str = "00000000-0000-0000-0000-000000000001";
const ITEM: &str = "00000000-0000-0000-0000-000000000002";

fn command(server: &MockServer, verb: &str) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .env("FABIO_STORAGE_ACCESS_TOKEN", "storage-test-token")
        .env("FABIO_NO_VERSION_CHECK", "1")
        .args([
            "item",
            verb,
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--table",
            "sales",
        ]);
    command
}

fn baseline() -> Value {
    json!({"description":"before","tags":{"keep":"yes"},"columns":[{"columnName":"id","columnId":1,"description":"identifier"}]})
}

#[test]
fn metadata_merge_preview_and_readonly_never_post_and_retain_unrelated_values() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(header("authorization", "Bearer storage-test-token"))
            .and(path(format!(
                "/common/v1/{WS}/{ITEM}/schemas/dbo/tables/sales/additionalMetadata"
            )))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .expect(2)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .env("OPENCODE", "1")
        .args(["--dry-run", "--content", r#"{"description":"after"}"#])
        .assert()
        .success();
    let v = common::parse_json(&result);
    assert_eq!(v["data"]["dry_run"], true);
    assert_eq!(v["data"]["destructive"], true);
    assert_eq!(v["data"]["would_execute"], "item update-table-metadata");
    assert!(v["data"]["agentNotice"].is_string());
    assert_eq!(v["data"]["details"]["after"]["tags"], baseline()["tags"]);
    assert_eq!(
        v["data"]["details"]["after"]["columns"],
        json!([{"columnName":"id","description":"identifier"}])
    );
    let blocked = command(&server, "update-table-metadata")
        .args([
            "--readonly",
            "--quiet",
            "--content",
            r#"{"description":"after"}"#,
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&blocked.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "READONLY_MODE");
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        2
    );
}

#[test]
fn metadata_post_replaces_with_complete_merged_body_and_verifies_readback() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut after = baseline();
    after["description"] = json!("after");
    after["columns"][0]
        .as_object_mut()
        .unwrap()
        .remove("columnId");
    let mut readback = after.clone();
    readback["columns"][0]["columnId"] = json!(1);
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(body_json(&after))
            .and(header("authorization", "Bearer storage-test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&after))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&readback))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args(["--content", r#"{"description":"after"}"#])
        .assert()
        .success();
    let v = common::parse_json(&result);
    assert_eq!(v["data"]["verified"], true);
    assert_eq!(v["data"]["metadata"], after);
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        3
    );
}

#[test]
fn metadata_bad_scope_and_types_fail_before_reads() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    for content in [
        "[]",
        r#"{"columns":[{}]}"#,
        r#"{"description":false}"#,
        "{}",
    ] {
        let result = command(&server, "update-table-metadata")
            .args(["--quiet", "--dry-run", "--content", content])
            .assert()
            .failure();
        let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_INPUT");
    }
    let result = common::mock_fabio(&server)
        .env("FABIO_ONELAKE_TABLE_ENDPOINT", server.uri())
        .args([
            "item",
            "get-table-metadata",
            "--workspace",
            WS,
            "--id",
            ITEM,
            "--table",
            "../all",
        ])
        .assert()
        .failure();
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        0
    );
    assert_ne!(result.get_output().status.code(), Some(0));
}

#[test]
fn metadata_overwrite_preview_explicitly_removes_omitted_values() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args([
            "--dry-run",
            "--overwrite",
            "--content",
            r#"{"description":"only"}"#,
        ])
        .assert()
        .success();
    let result = common::parse_json(&result);
    assert_eq!(result["data"]["details"]["overwrite"], true);
    assert_eq!(
        result["data"]["details"]["after"],
        json!({"description":"only"})
    );
    assert_eq!(result["data"]["destructive"], true);
    assert!(
        result["data"]["details"]["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning
                .as_str()
                .is_some_and(|s| s.contains("--overwrite is active")))
    );
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        1
    );
}

#[test]
fn metadata_readback_failure_retains_acknowledgement_without_authorizing_replay() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .up_to_n_times(1)
            .with_priority(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"description":"after"})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_json(
                json!({"error":{"code":"Forbidden","message":"synthetic verification denial"}}),
            ))
            .with_priority(2)
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args(["--content", r#"{"description":"after"}"#])
        .assert()
        .success();
    let result = common::parse_json(&result);
    assert_eq!(result["data"]["status"], "acknowledged");
    assert_eq!(result["data"]["verified"], false);
    assert_eq!(result["data"]["verificationError"], "FORBIDDEN");
    assert_eq!(result["data"]["retriable"], false);
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        3
    );
}

#[test]
fn metadata_uncertain_post_is_never_replayed_or_verified() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(503).set_body_string("synthetic gateway error"))
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args(["--quiet", "--content", r#"{"description":"after"}"#])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["retriable"], false);
    assert!(error["error"]["submission"].is_object());
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        2
    );
}

#[test]
fn metadata_rejects_async_acknowledgement_without_replaying() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(async {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(baseline()))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(202).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args(["--quiet", "--content", r#"{"description":"after"}"#])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert_eq!(error["error"]["retriable"], false);
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        2
    );
}

#[test]
fn metadata_unresolved_shortcut_never_posts_or_repairs_underlying_files() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server=runtime.block_on(async {
        let server=MockServer::start().await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(400).set_body_json(json!({"error":{"code":"MetadataMissing","message":"The 'metadata' directory for the Iceberg table is missing the expected metadata files."}})))
            .expect(1).mount(&server).await;
        server
    });
    let result = command(&server, "update-table-metadata")
        .args(["--quiet", "--content", r#"{"description":"after"}"#])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&result.get_output().stderr).unwrap();
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("No metadata POST was attempted")
    );
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        1
    );
}

#[test]
#[ignore = "requires explicit FABIO_TEST_METADATA_WORKSPACE/ITEM/TABLE pointing ONLY to a disposable table"]
fn metadata_live_disposable_roundtrip() {
    let ws = std::env::var("FABIO_TEST_METADATA_WORKSPACE").expect("disposable workspace");
    let item = std::env::var("FABIO_TEST_METADATA_ITEM").expect("disposable item");
    let table = std::env::var("FABIO_TEST_METADATA_TABLE").expect("disposable table");
    let scope = [
        "--workspace",
        &ws,
        "--id",
        &item,
        "--schema",
        "dbo",
        "--table",
        &table,
    ];
    let get = || -> Option<Value> {
        let output = common::fabio()
            .args(["--readonly", "item", "get-table-metadata"])
            .args(scope)
            .output()
            .expect("run fabio");
        if !output.status.success() {
            return None;
        }
        let mut data = serde_json::from_slice::<Value>(&output.stdout).ok()?["data"].clone();
        unwrap_metadata_descriptions(&mut data);
        Some(data)
    };
    let before =
        get().expect("live metadata GET failed before mutation; inspect private diagnostics");
    let mut restore_body = before.clone();
    if let Some(columns) = restore_body
        .get_mut("columns")
        .and_then(Value::as_array_mut)
    {
        for column in columns {
            column.as_object_mut().unwrap().remove("columnId");
        }
    }
    let result=common::fabio().args(["item","update-table-metadata"]).args(scope)
        .args(["--content",r#"{"description":"Synthetic live metadata validation","tags":{"test":"metadata"},"columns":[{"columnName":"id","description":"Synthetic identifier"}]}"#])
        .output().expect("run fabio");
    let observed = get();
    // Always attempt restoration before checking update assertions; never print payloads.
    let restore = common::fabio()
        .args(["item", "update-table-metadata"])
        .args(scope)
        .args(["--overwrite", "--content", &restore_body.to_string()])
        .output()
        .expect("restore disposable metadata");
    assert!(
        restore.status.success(),
        "disposable metadata restoration failed"
    );
    assert!(result.status.success(), "live metadata update failed");
    let acknowledgement: Value =
        serde_json::from_slice(&result.stdout).expect("JSON acknowledgement");
    assert_eq!(acknowledgement["data"]["verified"], true);
    assert_eq!(acknowledgement["data"]["retriable"], false);
    let observed = observed.expect("live readback failed after restoration attempt");
    assert!(
        observed["description"]
            .as_str()
            .is_some_and(|s| s.contains("Synthetic live metadata validation")),
        "metadata readback mismatch"
    );
    let after = get().expect("live restoration readback failed");
    assert!(
        after == before,
        "restored metadata differs; inspect privately"
    );
}

#[test]
#[ignore = "requires disposable FABIO_TEST_METADATA_WORKSPACE/ITEM/SHORTCUT; read-only preview availability probe"]
fn metadata_live_shortcut_read_reports_data_or_the_known_resolution_limit() {
    let workspace = std::env::var("FABIO_TEST_METADATA_WORKSPACE").expect("disposable workspace");
    let item = std::env::var("FABIO_TEST_METADATA_ITEM").expect("disposable item");
    let shortcut = std::env::var("FABIO_TEST_METADATA_SHORTCUT").expect("disposable shortcut");
    let output = common::fabio()
        .args([
            "--readonly",
            "--quiet",
            "item",
            "get-table-metadata",
            "--workspace",
            &workspace,
            "--id",
            &item,
            "--table",
            &shortcut,
        ])
        .output()
        .expect("run fabio");
    if output.status.success() {
        return;
    }
    let error: Value = serde_json::from_slice(&output.stderr).expect("JSON teaching error");
    assert!(
        error["error"]["hint"]
            .as_str()
            .is_some_and(|s| s.contains("Do not create, delete or rewrite metadata files")),
        "unexpected live shortcut failure; inspect private diagnostics"
    );
}

fn unwrap_metadata_descriptions(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                if key == "description"
                    && let Some(text) = value.as_str()
                {
                    *value = Value::from(
                        text.trim_prefix("<<<UNTRUSTED>>>")
                            .trim_suffix("<<<END_UNTRUSTED>>>"),
                    );
                } else {
                    unwrap_metadata_descriptions(value);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                unwrap_metadata_descriptions(value);
            }
        }
        _ => {}
    }
}
