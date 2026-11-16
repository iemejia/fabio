//! Eventstream ALM references and pre-publication value-set activation.
mod common;

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

const WS: &str = "00000000-0000-0000-0000-000000000001";
const LIB: &str = "00000000-0000-0000-0000-000000000002";
const STREAM: &str = "00000000-0000-0000-0000-000000000003";

fn stream_definition() -> Value {
    json!({"sources":[{"name":"input","type":"CustomEndpoint","properties":{}}],"operators":[],
        "streams":[{"name":"input-stream","type":"DefaultStream","properties":{},"inputNodes":[{"name":"input"}]}],
        "destinations":[{"name":"target","type":"Lakehouse","properties":{"itemReference":"$(/**/Settings/Target)","schema":"dbo","deltaTable":"events","inputSerialization":{"type":"Json","properties":{"encoding":"UTF8"}}},"inputNodes":[{"name":"input-stream"}]}]})
}

fn library_definition() -> Value {
    let part = |name: &str, value: Value| json!({"path":name,"payloadType":"InlineBase64","payload":BASE64.encode(value.to_string())});
    json!({"definition":{"parts":[
        part("variables.json",json!({"variables":[{"name":"Target","type":"ItemReference","value":{"workspaceId":WS,"itemId":STREAM}}]})),
        part("settings.json",json!({"valueSetsOrder":["test"]})),
        part("valueSets/test.json",json!({"name":"test","variableOverrides":[]}))]}})
}

struct Fixture {
    home: tempfile::TempDir,
    source: PathBuf,
    plan: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let source = home.path().join("source");
        std::fs::create_dir(&source).unwrap();
        for (name, kind, file, contents) in [
            (
                "Settings",
                "VariableLibrary",
                "variables.json",
                json!({"variables":[]}),
            ),
            (
                "Stream",
                "Eventstream",
                "eventstream.json",
                stream_definition(),
            ),
        ] {
            let dir = source.join(format!("{name}.{kind}"));
            std::fs::create_dir(&dir).unwrap();
            std::fs::write(
                dir.join(".platform"),
                json!({"metadata":{"type":kind,"displayName":name}}).to_string(),
            )
            .unwrap();
            std::fs::write(dir.join(file), contents.to_string()).unwrap();
        }
        let library_dir = source.join("Settings.VariableLibrary");
        for part in library_definition()["definition"]["parts"]
            .as_array()
            .unwrap()
        {
            let file = library_dir.join(part["path"].as_str().unwrap());
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(
                file,
                BASE64.decode(part["payload"].as_str().unwrap()).unwrap(),
            )
            .unwrap();
        }
        let plan = home.path().join("plan.json");
        std::fs::write(&plan,json!({"workspace_id":WS,"source_path":source,"changeset":{"changes":[
            {"name":"Settings","item_type":"VariableLibrary","action":"skip","reason":"unchanged","deployed_id":LIB},
            {"name":"Stream","item_type":"Eventstream","action":"create","reason":"new"}],"warnings":[],"errors":[]}}).to_string()).unwrap();
        Self { home, source, plan }
    }

    fn command(&self, server: &MockServer) -> assert_cmd::Command {
        let mut command = common::mock_fabio(server);
        command
            .env("HOME", self.home.path())
            .env("USERPROFILE", self.home.path())
            .env("FABIO_NO_VERSION_CHECK", "1")
            .args(["deploy", "apply", "--plan"])
            .arg(&self.plan)
            .args(["--no-folders", "--env", "test"]);
        command
    }
}

fn data(result: &assert_cmd::assert::Assert) -> Value {
    common::parse_json(result)["data"].clone()
}

#[test]
fn builder_previews_preserve_supported_references_without_resolved_identifiers() {
    let result=common::fabio().args(["--dry-run","eventstream","add-destination","--workspace",WS,"--id",STREAM,
        "--name","target","--destination-type","Eventhouse","--input-node","input-stream","--properties",
        r#"{"dataIngestionMode":"ProcessedIngestion","itemReference":"$(/**/Settings/Target)","tableNameReference":"$(/**/Settings/Table)","inputSerialization":{"type":"Json","properties":{"encoding":"UTF8"}}}"#])
        .assert().success();
    let result = data(&result);
    assert_eq!(
        result["details"]["destination"]["properties"]["itemReference"],
        "$(/**/Settings/Target)"
    );
    assert_eq!(
        result["details"]["destination"]["properties"].get("itemId"),
        None
    );
    let invalid = common::fabio()
        .args([
            "--dry-run",
            "--quiet",
            "eventstream",
            "add-source",
            "--workspace",
            WS,
            "--id",
            STREAM,
            "--name",
            "bad",
            "--source-type",
            "CustomEndpoint",
            "--properties",
            r#"{"connectionReference":"$(/**/Settings/Connection)"}"#,
        ])
        .assert()
        .failure();
    let error: Value = serde_json::from_slice(&invalid.get_output().stderr).unwrap();
    assert_eq!(error["error"]["code"], "INVALID_INPUT");
}

#[test]
fn deploy_previews_reference_prerequisites_and_rejects_bulk_without_network() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let server = runtime.block_on(MockServer::start());
    let fixture = Fixture::new();
    let result = fixture
        .command(&server)
        .arg("--dry-run")
        .env("OPENCODE", "1")
        .assert()
        .success();
    let result = data(&result);
    assert_eq!(result["destructive"], true);
    assert_eq!(result["variableLibraryPrerequisites"]["valueSet"], "test");
    assert_eq!(result["variableLibraryPrerequisites"]["verified"], false);
    for args in [["--strategy", "bulk"], ["--no-post-hooks", "--quiet"]] {
        fixture.command(&server).args(args).assert().failure();
    }
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        0
    );
    assert!(fixture.source.exists());
}

#[test]
fn readonly_definition_reads_preserve_symbols_instead_of_using_resolved_topology() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let definition = stream_definition();
    let server=runtime.block_on(async {
        let server=MockServer::start().await;
        Mock::given(method("POST")).and(path(format!("/workspaces/{WS}/eventstreams/{STREAM}/getDefinition")))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"definition":{"parts":[{"path":"eventstream.json","payloadType":"InlineBase64","payload":BASE64.encode(definition.to_string())}]}})))
            .expect(1).mount(&server).await;
        server
    });
    let result = common::mock_fabio(&server)
        .args([
            "--readonly",
            "eventstream",
            "get-definition",
            "--workspace",
            WS,
            "--id",
            STREAM,
            "--decode",
        ])
        .assert()
        .success();
    assert!(data(&result).to_string().contains("$(/**/Settings/Target)"));
    assert_eq!(
        runtime.block_on(server.received_requests()).unwrap().len(),
        1
    );
}

async fn mount_library(
    server: &MockServer,
    active: Arc<AtomicBool>,
    patch_status: u16,
    verify_active: bool,
) {
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/variableLibraries/{LIB}/getDefinition"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(library_definition()))
        .expect(1)
        .mount(server)
        .await;
    let observed = Arc::clone(&active);
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/variableLibraries/{LIB}")))
        .respond_with(move|_:&wiremock::Request|ResponseTemplate::new(200).set_body_json(json!({"id":LIB,"displayName":"Settings","properties":{"activeValueSetName":if observed.load(Ordering::SeqCst)&&verify_active {"test"}else{"Default value set"}}})))
        .mount(server).await;
    Mock::given(method("PATCH")).and(path(format!("/workspaces/{WS}/variableLibraries/{LIB}")))
        .respond_with(move|_:&wiremock::Request|{
            active.store(true,Ordering::SeqCst);
            ResponseTemplate::new(patch_status).set_body_json(if patch_status==200 {json!({"id":LIB})}else{json!({"error":{"code":"SyntheticFailure","message":"synthetic activation failure"}})})
        }).expect(1).mount(server).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deploy_activates_and_verifies_a_skipped_library_before_stream_publication() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    let active = Arc::new(AtomicBool::new(false));
    mount_library(&server, Arc::clone(&active), 200, true).await;
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(move |request: &wiremock::Request| {
            assert!(
                active.load(Ordering::SeqCst),
                "stream published before activation"
            );
            let body: Value = request.body_json().unwrap();
            let parts = body["definition"]["parts"].as_array().unwrap();
            let part = parts
                .iter()
                .find(|part| part["path"] == "eventstream.json")
                .unwrap();
            let definition: Value =
                serde_json::from_slice(&BASE64.decode(part["payload"].as_str().unwrap()).unwrap())
                    .unwrap();
            assert_eq!(
                definition["destinations"][0]["properties"]["itemReference"],
                "$(/**/Settings/Target)"
            );
            ResponseTemplate::new(201)
                .set_body_json(json!({"id":STREAM,"type":"Eventstream","displayName":"Stream"}))
        })
        .expect(1)
        .mount(&server)
        .await;
    let result = fixture.command(&server).assert().success();
    let result = data(&result);
    assert_eq!(result["destructive"], true);
    assert!(
        result["post_hooks"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hook| hook["phase"] == "pre_stream" && hook["verified"] == true)
    );
    let activation = result["post_hooks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|hook| hook["phase"] == "pre_stream")
        .unwrap();
    assert_eq!(activation["item_id"], LIB);
    assert_eq!(activation["workspace_id"], WS);
    assert_eq!(server.received_requests().await.unwrap().len(), 5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn activation_failure_or_unverified_readback_blocks_streams_and_later_mutations() {
    for (status, verify) in [(503, true), (403, true), (200, false)] {
        let server = MockServer::start().await;
        let fixture = Fixture::new();
        let active = Arc::new(AtomicBool::new(false));
        mount_library(&server, active, status, verify).await;
        let result = fixture.command(&server).assert().failure();
        let result = data(&result);
        assert_eq!(result["halted"], true);
        assert_eq!(result["retryAllowed"], false);
        let requests = server.received_requests().await.unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.method == "PATCH")
                .count(),
            1
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path().ends_with("/items"))
                .count(),
            0
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mismatched_library_name_and_id_never_activates_an_unrelated_library() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    Mock::given(method("POST"))
        .and(path(format!(
            "/workspaces/{WS}/variableLibraries/{LIB}/getDefinition"
        )))
        .respond_with(ResponseTemplate::new(200).set_body_json(library_definition()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path(format!("/workspaces/{WS}/variableLibraries/{LIB}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":LIB,"displayName":"OtherLibrary","properties":{"activeValueSetName":"Default value set"}})))
        .expect(1).mount(&server).await;
    let result = fixture.command(&server).assert().failure();
    let result = data(&result);
    assert_eq!(result["halted"], true);
    assert_eq!(result["retryAllowed"], false);
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn newly_created_library_is_activated_once_before_its_consumer() {
    let server = MockServer::start().await;
    let fixture = Fixture::new();
    let mut plan: Value = serde_json::from_slice(&std::fs::read(&fixture.plan).unwrap()).unwrap();
    plan["changeset"]["changes"][0]["action"] = json!("create");
    plan["changeset"]["changes"][0]
        .as_object_mut()
        .unwrap()
        .remove("deployed_id");
    std::fs::write(&fixture.plan, plan.to_string()).unwrap();
    let active = Arc::new(AtomicBool::new(false));
    mount_library(&server, Arc::clone(&active), 200, true).await;
    Mock::given(method("POST"))
        .and(path(format!("/workspaces/{WS}/items")))
        .respond_with(move |request: &wiremock::Request| {
            let body: Value = request.body_json().unwrap();
            let id = if body["type"] == "VariableLibrary" {
                LIB
            } else {
                assert!(active.load(Ordering::SeqCst));
                STREAM
            };
            ResponseTemplate::new(201).set_body_json(json!({"id":id}))
        })
        .expect(2)
        .mount(&server)
        .await;
    let result = fixture.command(&server).assert().success();
    let result = data(&result);
    assert_eq!(
        result["post_hooks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|hook| hook["hook"] == "activate_value_set")
            .count(),
        1
    );
    assert_eq!(
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .filter(|request| request.method == "PATCH")
            .count(),
        1
    );
}

#[test]
#[ignore = "requires explicit FABIO_TEST_VAR_* disposable fixtures; read-only binding/activation checks, not event delivery"]
fn live_authored_references_and_runtime_bindings_match_the_reviewed_alternate_set() {
    let variable = |name: &str| {
        std::env::var(name).expect("explicit disposable variable-library fixture required")
    };
    let workspace = variable("FABIO_TEST_METADATA_WORKSPACE");
    let library = variable("FABIO_TEST_VAR_LIBRARY");
    let library_name = variable("FABIO_TEST_VAR_LIBRARY_NAME");
    let value_set = variable("FABIO_TEST_VAR_VALUE_SET");
    let variable_name = variable("FABIO_TEST_VAR_VARIABLE");
    let stream = variable("FABIO_TEST_VAR_STREAM");
    let destination = variable("FABIO_TEST_VAR_DEST_ITEM");
    let read = |args: &[&str]| -> Value {
        let output = common::fabio()
            .arg("--readonly")
            .args(args)
            .output()
            .expect("run fabio");
        assert!(
            output.status.success(),
            "live scoped binding read failed; inspect private diagnostics"
        );
        serde_json::from_slice::<Value>(&output.stdout).expect("JSON response")["data"].clone()
    };
    let observed = read(&[
        "variable-library",
        "show",
        "--workspace",
        &workspace,
        "--id",
        &library,
    ]);
    assert!(
        observed
            .pointer("/properties/activeValueSetName")
            .and_then(Value::as_str)
            == Some(value_set.as_str()),
        "reviewed set is not active"
    );
    let definition = read(&[
        "eventstream",
        "get-definition",
        "--workspace",
        &workspace,
        "--id",
        &stream,
    ]);
    let part = definition["definition"]["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|part| part["path"] == "eventstream.json")
        .unwrap();
    let authored: Value =
        serde_json::from_slice(&BASE64.decode(part["payload"].as_str().unwrap()).unwrap()).unwrap();
    let reference = format!("$(/**/{library_name}/{variable_name})");
    let node = authored["destinations"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["properties"]["itemReference"].as_str() == Some(reference.as_str()))
        .expect("authored symbol was not preserved");
    assert!(
        ["workspaceId", "itemId"]
            .iter()
            .all(|key| node["properties"].get(*key).is_none_or(Value::is_null)),
        "authored reference unexpectedly contains resolved IDs; inspect privately"
    );
    let topology = read(&[
        "eventstream",
        "get-topology",
        "--workspace",
        &workspace,
        "--id",
        &stream,
    ]);
    assert!(
        topology["destinations"].as_array().unwrap().iter().any(
            |node| node["properties"]["itemId"].as_str() == Some(destination.as_str())
                && node["properties"]["workspaceId"].as_str() == Some(workspace.as_str())
        ),
        "resolved destination does not match the reviewed alternate"
    );
}
