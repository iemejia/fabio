//! End-to-end tests for `fabio item validate-definition` (offline validator).
//!
//! These require no live tenant — the validator runs entirely locally.

mod common;

use std::io::Write;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use common::{fabio, parse_json};
use serial_test::serial;
use tempfile::TempDir;

fn b64(s: &str) -> String {
    BASE64.encode(s.as_bytes())
}

#[test]
#[serial]
fn validate_definition_valid_notebook_envelope() {
    let payload = b64("# Fabric notebook source\nprint('hi')");
    let platform = b64(r#"{"metadata":{"type":"Notebook","displayName":"nb"}}"#);
    let envelope = format!(
        r#"{{"definition":{{"parts":[
            {{"path":"notebook-content.py","payload":"{payload}","payloadType":"InlineBase64"}},
            {{"path":".platform","payload":"{platform}","payloadType":"InlineBase64"}}
        ]}}}}"#
    );
    let assert = fabio()
        .args([
            "item",
            "validate-definition",
            "--type",
            "Notebook",
            "--definition",
            &envelope,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let data = &json["data"];
    assert_eq!(data["valid"], true);
    assert_eq!(data["errorCount"], 0);
    assert_eq!(data["warningCount"], 0);
    assert_eq!(data["partCount"], 2);
}

#[test]
#[serial]
fn validate_definition_invalid_base64_is_error_exit_nonzero() {
    let envelope =
        r#"{"parts":[{"path":"a.json","payload":"@@@not-base64","payloadType":"InlineBase64"}]}"#;
    let assert = fabio()
        .args(["item", "validate-definition", "--definition", envelope])
        .assert()
        .failure();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).expect("stdout JSON");
    assert_eq!(json["data"]["valid"], false);
    let codes: Vec<&str> = json["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"INVALID_BASE64"), "codes={codes:?}");
}

#[test]
#[serial]
fn validate_definition_invalid_json_part_is_error() {
    let payload = b64("this is not json");
    let envelope = format!(
        r#"{{"parts":[{{"path":"pipeline-content.json","payload":"{payload}","payloadType":"InlineBase64"}}]}}"#
    );
    let assert = fabio()
        .args(["item", "validate-definition", "--definition", &envelope])
        .assert()
        .failure();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["valid"], false);
}

#[test]
#[serial]
fn validate_definition_wrong_part_path_is_warning_not_error() {
    // A DataPipeline whose only part uses a non-canonical name: warning, still valid.
    let payload = b64(r#"{"properties":{"activities":[]}}"#);
    let envelope = format!(
        r#"{{"definition":{{"parts":[{{"path":"wrong-name.json","payload":"{payload}","payloadType":"InlineBase64"}}]}}}}"#
    );
    let assert = fabio()
        .args([
            "item",
            "validate-definition",
            "--type",
            "DataPipeline",
            "--definition",
            &envelope,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["valid"], true);
    assert_eq!(json["data"]["warningCount"], 1);
    let codes: Vec<&str> = json["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"MISSING_CANONICAL_PART"), "codes={codes:?}");
}

#[test]
#[serial]
fn validate_definition_strict_promotes_warning_to_failure() {
    let payload = b64(r#"{"properties":{"activities":[]}}"#);
    let envelope = format!(
        r#"{{"parts":[{{"path":"wrong-name.json","payload":"{payload}","payloadType":"InlineBase64"}}]}}"#
    );
    fabio()
        .args([
            "item",
            "validate-definition",
            "--strict",
            "--type",
            "DataPipeline",
            "--definition",
            &envelope,
        ])
        .assert()
        .failure();
}

#[test]
#[serial]
fn validate_definition_bad_payload_type_enumerates_valid_value() {
    let envelope = format!(
        r#"{{"parts":[{{"path":"a.json","payload":"{}","payloadType":"Base64"}}]}}"#,
        b64("{}")
    );
    let assert = fabio()
        .args(["item", "validate-definition", "--definition", &envelope])
        .assert()
        .failure();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("InlineBase64"), "stdout={stdout}");
}

#[test]
#[serial]
fn validate_definition_dir_mode_assembles_and_validates() {
    let dir = TempDir::new().unwrap();
    let part = dir.path().join("SparkJobDefinitionV1.json");
    let mut f = std::fs::File::create(&part).unwrap();
    f.write_all(br#"{"executableFile":null,"language":"Python"}"#)
        .unwrap();

    let assert = fabio()
        .args([
            "item",
            "validate-definition",
            "--type",
            "SparkJobDefinition",
            "--dir",
            dir.path().to_str().unwrap(),
            "--strict",
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    assert_eq!(json["data"]["valid"], true);
    assert_eq!(json["data"]["partCount"], 1);
    assert_eq!(json["data"]["warningCount"], 0);
}

#[test]
#[serial]
fn validate_definition_unknown_type_is_warning() {
    let envelope = format!(
        r#"{{"parts":[{{"path":"a.json","payload":"{}","payloadType":"InlineBase64"}}]}}"#,
        b64("{}")
    );
    let assert = fabio()
        .args([
            "item",
            "validate-definition",
            "--type",
            "BogusType",
            "--definition",
            &envelope,
        ])
        .assert()
        .success();
    let json = parse_json(&assert);
    let codes: Vec<&str> = json["data"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["code"].as_str().unwrap())
        .collect();
    assert!(codes.contains(&"UNKNOWN_ITEM_TYPE"), "codes={codes:?}");
}

#[test]
#[serial]
fn validate_definition_requires_exactly_one_input() {
    // No input source at all.
    fabio()
        .args(["item", "validate-definition", "--type", "Notebook"])
        .assert()
        .failure();
}
