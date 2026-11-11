//! Offline/native discovery and mock-only safety-signal coverage.
mod common;

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
const ID: &str = "bbbbbbbb-1111-2222-3333-444444444444";
const DEFINITION: &str =
    r#"{"parts":[{"path":"fixture.json","payload":"e30=","payloadType":"InlineBase64"}]}"#;
const MODEL: &str = r#"{"compatibilityLevel":1604,"model":{"culture":"en-US","tables":[]}}"#;

fn command(server: &MockServer, home: &tempfile::TempDir) -> assert_cmd::Command {
    let mut command = common::mock_fabio(server);
    command
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("OPENCODE", "1")
        .env("FABIO_NO_VERSION_CHECK", "1");
    command
}

fn data(output: &assert_cmd::assert::Assert) -> Value {
    common::parse_json(output)["data"].clone()
}

fn confirm(value: &Value) {
    assert_eq!(value["destructive"], true, "{value}");
    assert!(
        value["agentNotice"]
            .as_str()
            .unwrap()
            .contains("Confirm with the user")
    );
}

fn required(metadata: &Value, file: &std::path::Path, directory: &std::path::Path) -> Vec<String> {
    let mut args = Vec::new();
    for (flag, value) in metadata["flags"].as_object().into_iter().flatten() {
        if value["required"] != true {
            continue;
        }
        args.push(flag.clone());
        if value["type"] == "bool" {
            continue;
        }
        args.push(match flag.as_str() {
            "--workspace" => WS.to_string(),
            "--file" => file.to_string_lossy().into_owned(),
            "--dir" => directory.to_string_lossy().into_owned(),
            "--name" => "Fixture".to_string(),
            _ if value["type"] == "integer" || value["type"] == "number" => "1".to_string(),
            _ => ID.to_string(),
        });
    }
    args
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn running_work_stop_and_cancel_previews_keep_the_confirmation_signal() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let schema: Value = serde_json::from_str(include_str!(
        "../src/commands/context/data/agent/commands.json"
    ))
    .unwrap();
    for (group, subcommand) in [
        ("notebook", "stop"),
        ("environment", "cancel-publish"),
        ("spark", "cancel-statement"),
        ("apache-airflow-job", "stop-environment"),
        ("job-scheduler", "cancel-instance"),
    ] {
        let metadata = &schema[group]["subcommands"][subcommand];
        assert_eq!(metadata["destructive"], true);
        let mut args = vec![group.to_string(), subcommand.to_string()];
        args.extend(required(metadata, home.path(), home.path()));
        for flag in ["--job-instance-id", "--job-id"] {
            if metadata["flags"].get(flag).is_some() && !args.iter().any(|arg| arg == flag) {
                args.extend([flag.to_string(), ID.to_string()]);
            }
        }
        args.extend(["--readonly".to_string(), "--dry-run".to_string()]);
        let output = command(&server, &home).args(&args).assert().success();
        confirm(&data(&output));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn definition_overwrites_publish_and_emit_confirm_signals_without_requests() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let file = home.path().join("definition.json");
    std::fs::write(&file, DEFINITION).unwrap();
    let model = home.path().join("model.bim");
    std::fs::write(&model, MODEL).unwrap();
    let schema: Value = serde_json::from_str(include_str!(
        "../src/commands/context/data/agent/commands.json"
    ))
    .unwrap();
    let mut checked = 0;
    for (group, metadata) in schema.as_object().unwrap() {
        let Some(command_meta) = metadata["subcommands"].get("update-definition") else {
            continue;
        };
        assert_eq!(command_meta["destructive"], true, "{group}");
        let mut args = vec![group.clone(), "update-definition".to_string()];
        let selected_file = if group == "semantic-model" {
            &model
        } else {
            &file
        };
        args.extend(required(command_meta, selected_file, home.path()));
        let flags = command_meta["flags"].as_object().unwrap();
        if !args
            .iter()
            .any(|arg| arg == "--file" || arg == "--content" || arg == "--definition")
        {
            if group == "ontology" {
                args.extend([
                    "--definition".to_string(),
                    file.to_string_lossy().into_owned(),
                ]);
            } else if flags.contains_key("--content") {
                args.extend(["--content".to_string(), DEFINITION.to_string()]);
            } else if flags.contains_key("--file") {
                args.extend([
                    "--file".to_string(),
                    selected_file.to_string_lossy().into_owned(),
                ]);
            } else if flags.contains_key("--definition") {
                args.extend(["--definition".to_string(), DEFINITION.to_string()]);
            }
        }
        args.extend(["--readonly".to_string(), "--dry-run".to_string()]);
        let output = command(&server, &home).args(&args).assert().success();
        let preview = data(&output);
        assert_eq!(preview["dry_run"], true, "{group}");
        assert_eq!(
            preview["would_execute"],
            format!("{group} update-definition")
        );
        confirm(&preview);
        checked += 1;
    }
    assert!(
        checked > 40,
        "Expected broad definition-overwrite coverage, got {checked}"
    );
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn conditional_flags_signal_risk_only_when_selected() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let file = home.path().join("fixture.csv");
    std::fs::write(&file, "id,value\n1,mock\n").unwrap();
    let model = home.path().join("model.bim");
    std::fs::write(&model, MODEL).unwrap();
    Mock::given(method("GET"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"value":[]})))
        .mount(&server)
        .await;
    let schema: Value = serde_json::from_str(include_str!(
        "../src/commands/context/data/agent/commands.json"
    ))
    .unwrap();
    let mut checked = 0;
    for (group, metadata) in schema.as_object().unwrap() {
        for (subcommand, metadata) in metadata["subcommands"].as_object().unwrap() {
            let Some(flags) = metadata["destructive_flags"].as_array() else {
                continue;
            };
            if metadata["destructive"] == true {
                continue;
            }
            for flag in flags {
                let flag = flag.as_str().unwrap();
                for active in [false, true] {
                    let mut args = vec![group.clone(), subcommand.clone()];
                    args.extend(required(metadata, &file, &home.path().join("export")));
                    if group == "semantic-model" && subcommand == "create" {
                        args.extend(["--file".to_string(), model.to_string_lossy().into_owned()]);
                    }
                    if active {
                        args.push(flag.to_string());
                        if flag == "--cancel-on-timeout" {
                            args.push("--wait".to_string());
                        }
                    }
                    args.extend(["--readonly".to_string(), "--dry-run".to_string()]);
                    let output = command(&server, &home).args(&args).assert().success();
                    let preview = data(&output);
                    if active {
                        confirm(&preview);
                    } else {
                        assert_ne!(preview["destructive"], true, "{group} {subcommand}");
                        assert!(preview["agentNotice"].is_null());
                    }
                }
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 10,
        "Expected conditional flag coverage, got {checked}"
    );
    assert!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|request| request.method.as_str() == "GET")
    );
    assert!(!home.path().join("export").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn checkout_and_nested_delete_preview_without_reads_and_readonly_blocks_execution() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    for args in [
        vec![
            "git",
            "checkout",
            "--workspace",
            WS,
            "--branch",
            "feature/review",
        ],
        vec![
            "git",
            "relation",
            "delete",
            "--workspace",
            WS,
            "--relation-id",
            ID,
        ],
    ] {
        let output = command(&server, &home)
            .args(&args)
            .args(["--readonly", "--dry-run"])
            .assert()
            .success();
        confirm(&data(&output));
        command(&server, &home)
            .args(&args)
            .arg("--readonly")
            .assert()
            .failure();
    }
    command(&server, &home)
        .args([
            "git",
            "checkout",
            "--workspace",
            WS,
            "--branch",
            "",
            "--dry-run",
        ])
        .assert()
        .failure();
    let output = command(&server, &home)
        .args(["context", "agent", "--group", "git"])
        .assert()
        .success();
    let metadata = data(&output);
    assert_eq!(
        metadata["group_details"]["subcommands"]["relation"]["subcommands"]["delete"]["destructive"],
        true
    );
    assert_eq!(
        metadata["group_details"]["subcommands"]["relation"]["subcommands"]["list"]["mutates"],
        false
    );
    let output = command(&server, &home)
        .args([
            "context",
            "describe",
            "mirrored-database",
            "update-definition",
        ])
        .assert()
        .success();
    assert_eq!(data(&output)["destructive"], true);
    let output = command(&server, &home)
        .args([
            "context",
            "agent",
            "--group",
            "policy-set",
            "--format",
            "mcp",
        ])
        .assert()
        .success();
    let tools = data(&output);
    let activation = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "fabio_policy_set_activate")
        .unwrap();
    assert_eq!(activation["annotations"]["destructiveHint"], true);
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn action_scoped_deployment_previews_distinguish_updates_from_create_only_and_noop() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let source = home.path().join("source");
    std::fs::create_dir(&source).unwrap();
    for (action, destructive) in [
        ("create", false),
        ("update", true),
        ("rename", true),
        ("delete", true),
        ("skip", false),
    ] {
        let plan = home.path().join("plan.json");
        std::fs::write(&plan, json!({"workspace_id":WS,"source_path":source,"changeset":{"changes":[{
            "name":"Fixture","item_type":"Notebook","action":action,"reason":"test plan","deployed_id":ID
        }]}}).to_string()).unwrap();
        let output = command(&server, &home)
            .args(["deploy", "apply", "--plan"])
            .arg(&plan)
            .args(["--readonly", "--dry-run"])
            .assert()
            .success();
        let preview = data(&output);
        assert_eq!(preview["destructive"], destructive);
        if destructive {
            confirm(&preview);
        } else {
            assert!(preview["agentNotice"].is_null());
        }
        command(&server, &home)
            .args(["deploy", "apply", "--plan"])
            .arg(&plan)
            .arg("--readonly")
            .assert()
            .failure();
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_drop_import_and_statistics_writes_fail_before_any_connection_under_readonly() {
    let server = MockServer::start().await;
    let home = tempfile::TempDir::new().unwrap();
    let file = home.path().join("fixture.csv");
    std::fs::write(&file, "id\n1\n").unwrap();
    command(&server, &home)
        .args([
            "--readonly",
            "sql-database",
            "import",
            "--workspace",
            WS,
            "--id",
            ID,
            "--file",
        ])
        .arg(&file)
        .arg("--drop-if-exists")
        .assert()
        .failure();
    command(&server, &home)
        .args([
            "sql-database",
            "import",
            "--workspace",
            WS,
            "--id",
            ID,
            "--file",
        ])
        .arg(&file)
        .args(["--drop-if-exists", "--no-create-table", "--dry-run"])
        .assert()
        .failure();
    for group in ["warehouse", "sql-database"] {
        command(&server, &home)
            .args([
                "--readonly",
                group,
                "statistics-delete",
                "--workspace",
                WS,
                "--id",
                ID,
                "--name",
                "Fixture",
            ])
            .assert()
            .failure();
        let output = command(&server, &home)
            .args([
                group,
                "statistics-delete",
                "--workspace",
                WS,
                "--id",
                ID,
                "--name",
                "Fixture",
                "--dry-run",
            ])
            .assert()
            .success();
        confirm(&data(&output));
    }
    assert_eq!(server.received_requests().await.unwrap().len(), 0);
}
