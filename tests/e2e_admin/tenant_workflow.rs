use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use base64::Engine;
use serde_json::{Value, json};
use wiremock::matchers::{body_json, method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

struct Harness {
    server: MockServer,
    dir: tempfile::TempDir,
    tenant: String,
}

impl Harness {
    async fn new() -> Self {
        Self {
            server: MockServer::start().await,
            dir: tempfile::tempdir_in(".").unwrap(),
            tenant: uuid::Uuid::new_v4().to_string(),
        }
    }

    fn file(&self, name: &str) -> String {
        self.dir.path().join(name).to_str().unwrap().to_owned()
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("fabio"));
        for (key, _) in std::env::vars().filter(|(k, _)| k.starts_with("FABIO_")) {
            command.env_remove(key);
        }
        command
            .env("FABIO_ACCESS_TOKEN", token(&self.tenant))
            .env("FABIO_FABRIC_API_ENDPOINT", self.server.uri())
            .env("FABIO_NO_UPDATE_CHECK", "1")
            .args(args);
        command
    }

    async fn run(&self, args: &[&str]) -> Output {
        let mut command = self.command(args);
        tokio::task::spawn_blocking(move || command.output().unwrap())
            .await
            .unwrap()
    }

    fn desired(&self, name: &str, settings: Value) -> String {
        let file = self.file(name);
        let mut document = json!({
            "schemaVersion":1, "kind":"tenant-settings", "tenantId":self.tenant,
            "endpoint":self.server.uri(),
        });
        document["settings"] = settings;
        write_yaml(&file, &document);
        file
    }

    async fn plan(&self, current: Value, desired: Value) -> String {
        list_mock(current).mount(&self.server).await;
        let file = self.desired("desired.yaml", desired);
        let plan = self.file("plan.yaml");
        success(
            &self
                .run(&[
                    "admin",
                    "plan-tenant-settings",
                    "--file",
                    &file,
                    "--out",
                    &plan,
                ])
                .await,
        );
        self.server.reset().await;
        plan
    }
}

fn token(tenant: &str) -> String {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(json!({"tid":tenant}).to_string());
    format!("header.{payload}.signature")
}

fn write_yaml(path: impl AsRef<Path>, value: &Value) {
    std::fs::write(path, serde_yaml::to_string(value).unwrap()).unwrap();
}

fn yaml(path: impl AsRef<Path>) -> Value {
    serde_yaml::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn success(output: &Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn error(output: &Output, code: &str) -> Value {
    assert!(!output.status.success());
    let value: Value = serde_json::from_str(
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .rev()
            .find(|line| line.starts_with('{'))
            .unwrap_or_default(),
    )
    .unwrap_or_else(|e| panic!("{e}: {}", String::from_utf8_lossy(&output.stderr)));
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn setting(name: &str, enabled: bool) -> Value {
    json!({"settingName":name,"enabled":enabled,"title":format!("Setting {name}"),"tenantSettingGroup":"Test","canSpecifySecurityGroups":true})
}

fn desired_setting(name: &str, enabled: bool) -> Value {
    json!({"settingName":name,"values":{"enabled":enabled}})
}

fn list_mock(settings: Value) -> Mock {
    let mut response = json!({});
    response["tenantSettings"] = settings;
    Mock::given(method("GET"))
        .and(path("/admin/tenantsettings"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
}

fn checkpoint(output: &Output, code: &str) -> String {
    error(output, code)["error"]["progress"]["checkpoint"]
        .as_str()
        .unwrap()
        .to_owned()
}

async fn count(h: &Harness, method: &str, resource: &str) -> usize {
    h.server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .filter(|r| r.method.as_str() == method && r.url.path() == resource)
        .count()
}

#[tokio::test]
async fn test_export_paginated_resume_repeated_throttles_and_completed_reuse() {
    let h = Harness::new().await;
    Mock::given(method("GET"))
        .and(path("/admin/tenantsettings"))
        .and(|r: &Request| r.url.query().is_none())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tenantSettings":[setting("Z", false)], "continuationToken":"page +/2",
        })))
        .expect(1)
        .mount(&h.server)
        .await;
    let requests = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&requests);
    Mock::given(method("GET"))
        .and(path("/admin/tenantsettings"))
        .and(query_param("continuationToken", "page +/2"))
        .respond_with(
            move |_: &Request| match calls.fetch_add(1, Ordering::SeqCst) {
                0 => ResponseTemplate::new(429).insert_header("Retry-After", "3600"),
                1 => ResponseTemplate::new(430)
                    .insert_header("Retry-After", "Tue, 13 Oct 2026 06:00:00 GMT"),
                _ => ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("A",true)]})),
            },
        )
        .expect(3)
        .mount(&h.server)
        .await;
    let out = h.file("export.yaml");
    let first = h
        .run(&[
            "admin",
            "export-tenant-settings",
            "--out",
            &out,
            "--limit",
            "1",
            "--query",
            "status",
        ])
        .await;
    let cp = checkpoint(&first, "RATE_LIMITED");
    assert_eq!(first.status.code(), Some(7));
    assert_eq!(
        error(&first, "RATE_LIMITED")["error"]["retry_after"],
        "3600"
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert!(!Path::new(&out).exists());
    assert_eq!(yaml(&cp)["snapshot"].as_array().unwrap().len(), 1);
    let second = h
        .run(&["admin", "export-tenant-settings", "--continue", &cp])
        .await;
    error(&second, "RATE_LIMITED");
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    let done = success(
        &h.run(&["admin", "export-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(done["data"]["total"], 2);
    let artifact = yaml(&out);
    assert_eq!(artifact["settings"][0]["settingName"], "A");
    assert_eq!(artifact["settings"][1]["metadata"]["title"], "Setting Z");
    assert_eq!(artifact["settings"][0]["values"]["enabled"], true);
    success(
        &h.run(&["admin", "export-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(requests.load(Ordering::SeqCst), 3);
    assert_eq!(count(&h, "GET", "/admin/tenantsettings").await, 4);
    let state_text = std::fs::read_to_string(&cp).unwrap();
    assert!(!state_text.contains(&token(&h.tenant)));
}

#[tokio::test]
async fn test_plan_uri_pagination_resumes_without_replaying_first_page() {
    let h = Harness::new().await;
    let file = h.desired(
        "desired.yaml",
        json!([desired_setting("A", true), desired_setting("B", false)]),
    );
    let out = h.file("plan.yaml");
    Mock::given(method("GET")).and(path("/admin/tenantsettings"))
        .and(|r: &Request| r.url.query().is_none())
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "tenantSettings":[setting("A",false)], "continuationUri":format!("{}/admin/tenantsettings?continuationToken=next",h.server.uri()),
        }))).expect(1).mount(&h.server).await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(query_param("continuationToken", "next"))
        .respond_with(move |_: &Request| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(429)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("B",false)]}))
            }
        })
        .expect(2)
        .mount(&h.server)
        .await;
    let first = h
        .run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &file,
            "--out",
            &out,
        ])
        .await;
    let cp = checkpoint(&first, "RATE_LIMITED");
    success(
        &h.run(&["admin", "plan-tenant-settings", "--continue", &cp])
            .await,
    );
    let plan = yaml(out);
    assert_eq!(plan["updates"].as_array().unwrap().len(), 1);
    assert_eq!(plan["unchanged"], 1);
    assert_eq!(
        plan["updates"][0]["changes"]["enabled"],
        json!({"before":false,"after":true})
    );
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 0);
}

#[tokio::test]
async fn test_apply_rate_limit_resume_grouped_acknowledgment_and_no_replay() {
    let h = Harness::new().await;
    let plan = h
        .plan(
            json!([setting("A", false), setting("B", false)]),
            json!([desired_setting("A", true), desired_setting("B", true)]),
        )
        .await;
    let list_calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(path("/admin/tenantsettings"))
        .respond_with(move |_: &Request| {
            let first = list_calls.fetch_add(1, Ordering::SeqCst) == 0;
            ResponseTemplate::new(200)
                .set_body_json(json!({"tenantSettings":[setting("A",!first),setting("B",false)]}))
        })
        .expect(2)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/A/update"))
        .and(body_json(json!({"enabled":true})))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"tenantSettings":[setting("B",false),setting("A",true)]})),
        )
        .expect(1)
        .mount(&h.server)
        .await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/B/update"))
        .and(body_json(json!({"enabled":true})))
        .respond_with(move |_: &Request| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(429).insert_header("Retry-After", "3600")
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("A",true),setting("B",true)]}))
            }
        })
        .expect(2)
        .mount(&h.server)
        .await;
    let first = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    let cp = checkpoint(&first, "RATE_LIMITED");
    let progress = &error(&first, "RATE_LIMITED")["error"]["progress"];
    assert_eq!(progress["completed"], 1);
    assert_eq!(progress["pending"], 1);
    assert_eq!(progress["uncertain"], 0);
    let done = success(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(done["data"]["completed"], 2);
    assert_eq!(done["data"]["destructive"], true);
    success(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 1);
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/B/update").await, 2);
}

#[tokio::test]
async fn test_apply_uncertain_write_reconciles_without_replaying_success() {
    for response in [
        ResponseTemplate::new(503),
        ResponseTemplate::new(200)
            .set_body_json(json!({"tenantSettings":[setting("Sibling",true)]})),
        ResponseTemplate::new(200).set_body_string("not json"),
    ] {
        let h = Harness::new().await;
        let plan = h
            .plan(
                json!([setting("A", false)]),
                json!([desired_setting("A", true)]),
            )
            .await;
        let calls = Arc::new(AtomicUsize::new(0));
        Mock::given(method("GET"))
            .and(path("/admin/tenantsettings"))
            .respond_with(move |_: &Request| {
                let enabled = calls.fetch_add(1, Ordering::SeqCst) > 0;
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("A",enabled)]}))
            })
            .expect(2)
            .mount(&h.server)
            .await;
        Mock::given(method("POST"))
            .and(path("/admin/tenantsettings/A/update"))
            .respond_with(response)
            .expect(1)
            .mount(&h.server)
            .await;
        let first = h
            .run(&["admin", "apply-tenant-settings", "--file", &plan])
            .await;
        let cp = checkpoint(&first, "API_ERROR");
        assert_eq!(
            error(&first, "API_ERROR")["error"]["progress"]["uncertain"],
            1
        );
        let done = success(
            &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
                .await,
        );
        assert_eq!(done["data"]["completed"], 1);
        assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 1);
    }
}

#[tokio::test]
async fn test_apply_stale_plan_aborts_before_any_write_and_already_desired_skips() {
    let h = Harness::new().await;
    let id = uuid::Uuid::new_v4().to_string();
    let mut current = setting("B", false);
    current["enabledSecurityGroups"] = json!([]);
    let plan = h
        .plan(
            json!([setting("A", false), current.clone()]),
            json!([desired_setting("A", true), desired_setting("B", true)]),
        )
        .await;
    current["enabledSecurityGroups"] = json!([{"graphId":id}]);
    list_mock(json!([setting("A", false), current]))
        .mount(&h.server)
        .await;
    let first = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    let cp = checkpoint(&first, "CONFLICT");
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 0);
    h.server.reset().await;
    let mut after = setting("B", true);
    after["enabledSecurityGroups"] = json!([]);
    list_mock(json!([setting("A", true), after]))
        .expect(1)
        .mount(&h.server)
        .await;
    let done = success(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(done["data"]["skipped"], 2);
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/B/update").await, 0);
}

#[tokio::test]
async fn test_offline_dry_run_readonly_wrong_tenant_and_changed_plan() {
    let h = Harness::new().await;
    let plan = h
        .plan(
            json!([setting("A", false)]),
            json!([desired_setting("A", true)]),
        )
        .await;
    let mut command = h.command(&[
        "admin",
        "apply-tenant-settings",
        "--file",
        &plan,
        "--dry-run",
    ]);
    command
        .env_remove("FABIO_ACCESS_TOKEN")
        .env("CLAUDECODE", "1");
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    let preview = success(&output);
    assert_eq!(preview["data"]["dry_run"], true);
    assert_eq!(
        preview["data"]["would_execute"],
        "admin apply-tenant-settings"
    );
    assert_eq!(preview["data"]["details"]["settings"], json!(["A"]));
    assert_eq!(preview["data"]["destructive"], true);
    assert!(preview["data"]["agentNotice"].is_string());
    assert!(!Path::new(&format!("{plan}.apply.checkpoint.yaml")).exists());
    error(
        &h.run(&[
            "admin",
            "apply-tenant-settings",
            "--file",
            &plan,
            "--readonly",
        ])
        .await,
        "READONLY_MODE",
    );
    let mut command = h.command(&["admin", "apply-tenant-settings", "--file", &plan]);
    command.env(
        "FABIO_ACCESS_TOKEN",
        token(&uuid::Uuid::new_v4().to_string()),
    );
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    error(&output, "INVALID_INPUT");
    assert_eq!(h.server.received_requests().await.unwrap().len(), 0);
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&h.server)
        .await;
    let output = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    let cp = checkpoint(&output, "RATE_LIMITED");
    writeln!(
        std::fs::OpenOptions::new()
            .append(true)
            .open(&plan)
            .unwrap(),
        "# edited"
    )
    .unwrap();
    error(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
        "INVALID_INPUT",
    );
    assert_eq!(h.server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn test_strict_input_and_path_failures_before_network() {
    let h = Harness::new().await;
    let file = h.desired("desired.yaml", json!([desired_setting("A", true)]));
    let base = std::fs::read_to_string(&file).unwrap();
    for content in [
        format!("{base}\nschemaVersion: 1\n"),
        base.replace("schemaVersion: 1", "schemaVersion: 99"),
        base.replace("enabled: true", "enabled: null"),
        base.replace("enabled: true", "enabled: 'true'"),
        base.replace("enabled: true", "unsupported: true"),
        base.replace("settingName: A", "settingName: '../A'"),
    ] {
        std::fs::write(&file, content).unwrap();
        error(
            &h.run(&[
                "admin",
                "plan-tenant-settings",
                "--file",
                &file,
                "--out",
                &h.file("plan.yaml"),
            ])
            .await,
            "INVALID_INPUT",
        );
    }
    std::fs::write(&file, &base).unwrap();
    error(
        &h.run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &file,
            "--out",
            &file,
        ])
        .await,
        "INVALID_INPUT",
    );
    error(
        &h.run(&["admin", "export-tenant-settings", "--out", &file])
            .await,
        "INVALID_INPUT",
    );
    error(
        &h.run(&[
            "admin",
            "export-tenant-settings",
            "--out",
            &h.file("missing/subdir.yaml"),
        ])
        .await,
        "INVALID_INPUT",
    );
    error(
        &h.run(&[
            "admin",
            "export-tenant-settings",
            "--out",
            &h.file("export.yaml"),
            "--continuation-token",
            "page2",
        ])
        .await,
        "INVALID_INPUT",
    );
    let args = h
        .run(&[
            "admin",
            "export-tenant-settings",
            "--out",
            &h.file("export.yaml"),
            "--continue",
            &file,
        ])
        .await;
    assert_eq!(args.status.code(), Some(2));
    assert_eq!(h.server.received_requests().await.unwrap().len(), 0);
}

#[tokio::test]
async fn test_malformed_and_unsafe_continuations_preserve_checkpoint() {
    for response in [
        json!({"tenantSettings":[setting("A",false)],"continuationUri":"https://other.fabric.microsoft.com/v1/admin/tenantsettings"}),
        json!({"tenantSettings":{}}),
        json!({"tenantSettings":[{"settingName":"A","enabled":"yes"}]}),
        json!({"tenantSettings":[setting("A",true),setting("A",false)]}),
    ] {
        let h = Harness::new().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response))
            .expect(1)
            .mount(&h.server)
            .await;
        let out = h.file("export.yaml");
        let output = h
            .run(&["admin", "export-tenant-settings", "--out", &out])
            .await;
        assert!(!output.status.success());
        let value: Value = serde_json::from_str(
            String::from_utf8_lossy(&output.stderr)
                .lines()
                .rev()
                .find(|line| line.starts_with('{'))
                .unwrap(),
        )
        .unwrap();
        assert!(value["error"]["progress"]["checkpoint"].is_string());
        assert!(!Path::new(&out).exists());
        assert_eq!(count(&h, "GET", "/admin/tenantsettings").await, 1);
    }
}

#[tokio::test]
async fn test_checkpoint_integrity_unsafe_cursor_and_operation_binding() {
    let h = Harness::new().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(429))
        .expect(1)
        .mount(&h.server)
        .await;
    let output = h
        .run(&[
            "admin",
            "export-tenant-settings",
            "--out",
            &h.file("export.yaml"),
        ])
        .await;
    let cp = checkpoint(&output, "RATE_LIMITED");
    error(
        &h.run(&["admin", "plan-tenant-settings", "--continue", &cp])
            .await,
        "INVALID_INPUT",
    );
    let original = yaml(&cp);
    let mut altered = original.clone();
    altered["nextPage"] = json!("https://other.fabric.microsoft.com/admin/tenantsettings");
    seal_checkpoint(&mut altered);
    write_yaml(&cp, &altered);
    error(
        &h.run(&["admin", "export-tenant-settings", "--continue", &cp])
            .await,
        "INVALID_INPUT",
    );
    altered = original;
    altered["pages"] = json!(99);
    write_yaml(&cp, &altered);
    error(
        &h.run(&["admin", "export-tenant-settings", "--continue", &cp])
            .await,
        "INVALID_INPUT",
    );
    assert_eq!(h.server.received_requests().await.unwrap().len(), 1);
}

fn seal_checkpoint(value: &mut Value) {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    fn sort(v: &mut Value) {
        match v {
            Value::Object(m) => {
                m.sort_keys();
                for v in m.values_mut() {
                    sort(v);
                }
            }
            Value::Array(a) => {
                for v in a {
                    sort(v);
                }
            }
            _ => {}
        }
    }
    value["integrity"] = json!("");
    sort(value);
    let mut hash = String::new();
    for b in Sha256::digest(serde_json::to_vec(value).unwrap()) {
        write!(hash, "{b:02x}").unwrap();
    }
    value["integrity"] = hash.into();
}

#[tokio::test]
async fn test_single_writer_and_checkpoint_write_failure_stop_updates() {
    let h = Harness::new().await;
    let plan = h
        .plan(
            json!([setting("A", false), setting("B", false)]),
            json!([desired_setting("A", true), desired_setting("B", true)]),
        )
        .await;
    let cp_path = PathBuf::from(format!("{plan}.apply.checkpoint.yaml"));
    let lock_path = PathBuf::from(format!("{}.lock", cp_path.display()));
    let lock = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .unwrap();
    fs2::FileExt::lock_exclusive(&lock).unwrap();
    error(
        &h.run(&["admin", "apply-tenant-settings", "--file", &plan])
            .await,
        "CONFLICT",
    );
    assert_eq!(h.server.received_requests().await.unwrap().len(), 0);
    drop(lock);
    list_mock(json!([setting("A", false), setting("B", false)]))
        .mount(&h.server)
        .await;
    let cp = cp_path.clone();
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/A/update"))
        .respond_with(move |_: &Request| {
            // Simulate losing the checkpoint destination immediately after the server accepts A.
            std::fs::rename(&cp, cp.with_extension("saved")).unwrap();
            std::fs::create_dir(&cp).unwrap();
            ResponseTemplate::new(200).set_body_json(json!({"tenantSettings":[setting("A",true)]}))
        })
        .expect(1)
        .mount(&h.server)
        .await;
    let output = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    error(&output, "INVALID_INPUT");
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/B/update").await, 0);
    assert_eq!(
        yaml(cp_path.with_extension("saved"))["outcomes"][0],
        "uncertain"
    );
    std::fs::remove_dir(&cp_path).unwrap();
    std::fs::rename(cp_path.with_extension("saved"), &cp_path).unwrap();
    h.server.reset().await;
    list_mock(json!([setting("A", true), setting("B", false)]))
        .expect(1)
        .mount(&h.server)
        .await;
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/B/update"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"tenantSettings":[setting("B",true)]})),
        )
        .expect(1)
        .mount(&h.server)
        .await;
    let done = success(
        &h.run(&[
            "admin",
            "apply-tenant-settings",
            "--continue",
            cp_path.to_str().unwrap(),
        ])
        .await,
    );
    assert_eq!(done["data"]["completed"], 2);
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 0);
}

#[tokio::test]
async fn test_export_noop_plan_empty_apply_and_quiet_output() {
    let h = Harness::new().await;
    list_mock(json!([setting("A", false)]))
        .expect(2)
        .mount(&h.server)
        .await;
    let out = h.file("export.yaml");
    let export = h
        .run(&["admin", "export-tenant-settings", "--out", &out, "--quiet"])
        .await;
    assert!(export.status.success());
    assert_eq!(export.stdout.len(), 0);
    let plan = h.file("plan.yaml");
    let planned = success(
        &h.run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &out,
            "--out",
            &plan,
        ])
        .await,
    );
    assert_eq!(planned["data"]["unchanged"], 1);
    assert_eq!(planned["data"]["destructive"], false);
    let applied = success(
        &h.run(&["admin", "apply-tenant-settings", "--file", &plan])
            .await,
    );
    assert_eq!(applied["data"]["total"], 0);
    assert_eq!(h.server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn test_apply_preflight_resumes_pages_and_security_group_payload_is_exact() {
    let h = Harness::new().await;
    let id = uuid::Uuid::new_v4().to_string();
    let mut a = setting("A", false);
    a["enabledSecurityGroups"] = json!([]);
    a["excludedSecurityGroups"] = json!([]);
    let plan = h.plan(
        json!([a.clone(),setting("B",false)]),
        json!([{"settingName":"A","values":{"enabledSecurityGroups":[{"graphId":id}]}},desired_setting("B",false)]),
    ).await;
    let approved = yaml(&plan);
    assert_eq!(approved["updates"][0]["after"]["enabled"], false);
    Mock::given(method("GET"))
        .and(path("/admin/tenantsettings"))
        .and(|r: &Request| r.url.query().is_none())
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({"tenantSettings":[a],"continuationToken":"next"})),
        )
        .expect(1)
        .mount(&h.server)
        .await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("GET"))
        .and(query_param("continuationToken", "next"))
        .respond_with(move |_: &Request| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(429)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("B",false)]}))
            }
        })
        .expect(2)
        .mount(&h.server)
        .await;
    let expected = json!({"enabled":false,"enabledSecurityGroups":[{"graphId":id}],"excludedSecurityGroups":[]});
    let mut after = setting("A", false);
    after["enabledSecurityGroups"] = expected["enabledSecurityGroups"].clone();
    after["excludedSecurityGroups"] = json!([]);
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/A/update"))
        .and(body_json(expected))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"tenantSettings":[after]})))
        .expect(1)
        .mount(&h.server)
        .await;
    let first = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    let cp = checkpoint(&first, "RATE_LIMITED");
    assert_eq!(count(&h, "POST", "/admin/tenantsettings/A/update").await, 0);
    let done = success(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
    );
    assert_eq!(done["data"]["completed"], 1);
    assert_eq!(count(&h, "GET", "/admin/tenantsettings").await, 3);
}

#[tokio::test]
async fn test_uncertain_baseline_is_reconciled_before_retry_and_tampered_plan_is_rejected() {
    let h = Harness::new().await;
    let plan = h
        .plan(
            json!([setting("A", false)]),
            json!([desired_setting("A", true)]),
        )
        .await;
    list_mock(json!([setting("A", false)]))
        .expect(2)
        .mount(&h.server)
        .await;
    let calls = Arc::new(AtomicUsize::new(0));
    Mock::given(method("POST"))
        .and(path("/admin/tenantsettings/A/update"))
        .respond_with(move |_: &Request| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(503)
            } else {
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tenantSettings":[setting("A",true)]}))
            }
        })
        .expect(2)
        .mount(&h.server)
        .await;
    let first = h
        .run(&["admin", "apply-tenant-settings", "--file", &plan])
        .await;
    let cp = checkpoint(&first, "API_ERROR");
    success(
        &h.run(&["admin", "apply-tenant-settings", "--continue", &cp])
            .await,
    );
    let requests = h.server.received_requests().await.unwrap();
    let verbs: Vec<_> = requests.iter().map(|r| r.method.as_str()).collect();
    assert_eq!(verbs, ["GET", "POST", "GET", "POST"]);
    let mut content = yaml(&plan);
    content["updates"][0]["after"]["enabled"] = json!(false);
    let tampered = h.file("tampered.yaml");
    write_yaml(&tampered, &content);
    error(
        &h.run(&[
            "admin",
            "apply-tenant-settings",
            "--file",
            &tampered,
            "--dry-run",
        ])
        .await,
        "INVALID_INPUT",
    );
    assert_eq!(h.server.received_requests().await.unwrap().len(), 4);
}

#[test]
#[ignore = "requires explicit administrator approval, designated tenant, setting, and live credentials"]
#[allow(clippy::too_many_lines)]
fn test_live_tenant_settings_approved_roundtrip_restores_captured_values() {
    assert_eq!(
        std::env::var("FABIO_TEST_TENANT_SETTINGS_APPLY").as_deref(),
        Ok("I_APPROVE_TENANT_SETTING_CHANGE")
    );
    let expected_tenant =
        std::env::var("FABIO_TEST_TENANT_ID").expect("designated tenant required");
    let name = std::env::var("FABIO_TEST_TENANT_SETTING_NAME")
        .expect("administrator-approved setting required");
    let dir = tempfile::tempdir_in(".").unwrap();
    let file = |name: &str| dir.path().join(name).to_str().unwrap().to_owned();
    let run = |args: &[&str]| {
        Command::new(assert_cmd::cargo::cargo_bin!("fabio"))
            .args(args)
            .output()
            .unwrap()
    };
    let exported = file("export.yaml");
    success(&run(&[
        "admin",
        "export-tenant-settings",
        "--out",
        &exported,
    ]));
    let export = yaml(&exported);
    assert_eq!(export["tenantId"], expected_tenant);
    let noop = file("noop.yaml");
    success(&run(&[
        "admin",
        "plan-tenant-settings",
        "--file",
        &exported,
        "--out",
        &noop,
    ]));
    assert_eq!(yaml(noop)["updates"].as_array().unwrap().len(), 0);
    let original = export["settings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["settingName"] == name)
        .expect("setting exists")
        .clone();
    let mut restore = export;
    restore["settings"] = json!([{"settingName":name,"values":original["values"]}]);
    let restore_file = file("restore-desired.yaml");
    write_yaml(&restore_file, &restore);
    let mut changed = restore.clone();
    changed["settings"][0]["values"]["enabled"] =
        json!(!original["values"]["enabled"].as_bool().unwrap());
    let desired_file = file("desired.yaml");
    write_yaml(&desired_file, &changed);
    let plan = file("plan.yaml");
    success(&run(&[
        "admin",
        "plan-tenant-settings",
        "--file",
        &desired_file,
        "--out",
        &plan,
    ]));
    let result = std::panic::catch_unwind(|| {
        success(&run(&["admin", "apply-tenant-settings", "--file", &plan]));
        let verify = file("verify.yaml");
        success(&run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &desired_file,
            "--out",
            &verify,
        ]));
        assert_eq!(yaml(verify)["updates"].as_array().unwrap().len(), 0);
    });
    let restore_plan = file("restore-plan.yaml");
    let restored = std::panic::catch_unwind(|| {
        success(&run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &restore_file,
            "--out",
            &restore_plan,
        ]));
        success(&run(&[
            "admin",
            "apply-tenant-settings",
            "--file",
            &restore_plan,
        ]));
        let verify = file("restore-verify.yaml");
        success(&run(&[
            "admin",
            "plan-tenant-settings",
            "--file",
            &restore_file,
            "--out",
            &verify,
        ]));
        assert_eq!(yaml(verify)["updates"].as_array().unwrap().len(), 0);
    });
    if let Err(error) = restored {
        eprintln!(
            "RESTORATION FAILED: recovery files retained at {}",
            dir.keep().display()
        );
        std::panic::resume_unwind(error);
    }
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
