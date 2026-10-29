//! Deterministic support for opt-in live tests. Included only by the governance
//! test binary so these tests are not duplicated in every integration suite.

use assert_cmd::Command;
use serde_json::Value;
use std::time::{Duration, Instant};

use crate::common::fabio;

type Response = Result<Value, String>;
type Runner = fn(&[&str]) -> Response;

fn parse_response(success: bool, stdout: &[u8], stderr: &[u8]) -> Response {
    if success {
        return serde_json::from_slice(stdout)
            .map_err(|_| "successful command returned invalid JSON".to_string());
    }
    let error = String::from_utf8_lossy(stderr)
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<Value>(line).ok());
    let code = error.as_ref().and_then(|v| v["error"]["code"].as_str());
    let code = code
        .filter(|code| {
            matches!(
                *code,
                "AUTH_REQUIRED"
                    | "FORBIDDEN"
                    | "NOT_FOUND"
                    | "API_ERROR"
                    | "INVALID_INPUT"
                    | "CONFLICT"
                    | "TIMEOUT"
                    | "CAPACITY_INACTIVE"
                    | "READONLY_MODE"
                    | "RATE_LIMITED"
                    | "NETWORK_ERROR"
            )
        })
        .unwrap_or("UNKNOWN_ERROR");
    Err(format!(
        "fabio failed with {code} (tenant details withheld)"
    ))
}

/// Unlike `assert_cmd`'s assertion reports, failures never print raw commands,
/// resource names, IDs, response bodies, or subprocess error strings.
pub fn execute(mut command: Command) -> Response {
    let output = command
        .timeout(Duration::from_mins(5))
        .output()
        .map_err(|_| "failed to execute fabio".to_string())?;
    parse_response(output.status.success(), &output.stdout, &output.stderr)
}

pub fn command(args: &[&str]) -> Command {
    let mut command = fabio();
    command.args(["--wrap-untrusted", "--lro-timeout", "240"]);
    command.args(args);
    command
}

pub fn try_run(args: &[&str]) -> Response {
    execute(command(args))
}

pub fn run(args: &[&str]) -> Value {
    try_run(args).unwrap_or_else(|error| panic!("{} {}: {error}", args[0], args[1]))
}

pub fn field(value: &Value, key: &str) -> String {
    value[key]
        .as_str()
        .unwrap_or_else(|| panic!("expected string field {key} (tenant details withheld)"))
        .to_string()
}

pub fn list(value: &Value) -> &[Value] {
    let items = value["data"].as_array().expect("expected list envelope");
    assert_eq!(value["count"].as_u64(), Some(items.len() as u64));
    items
}

pub fn contains_id(value: &Value, id: &str) -> bool {
    list(value)
        .iter()
        .any(|item| item["id"].as_str() == Some(id))
}

pub fn assert_reference(actual: &Value, expected: &str) {
    assert!(
        actual.as_str().is_some_and(|id| id == expected),
        "resource reference mismatch; tenant details withheld"
    );
}

/// Register immediately after creation. Never construct this guard for an
/// existing tenant resource. The injected runner lets tests prove cleanup
/// behavior without credentials, network requests, or global environment edits.
pub struct CreatedResource<R: Fn(&[&str]) -> Response = Runner> {
    delete_args: Vec<String>,
    active: bool,
    runner: R,
}

impl CreatedResource {
    pub fn new(args: &[&str]) -> Self {
        Self::with_runner(args, try_run)
    }
}

impl<R: Fn(&[&str]) -> Response> CreatedResource<R> {
    fn with_runner(args: &[&str], runner: R) -> Self {
        Self {
            delete_args: args.iter().map(|arg| (*arg).to_string()).collect(),
            active: true,
            runner,
        }
    }

    /// Use only after another guard has confirmed deletion of this same resource.
    pub const fn disarm(&mut self) {
        self.active = false;
    }

    pub fn delete(&mut self) {
        if !self.active {
            return;
        }
        let args: Vec<&str> = self.delete_args.iter().map(String::as_str).collect();
        let preview = (self.runner)(&[args.as_slice(), &["--dry-run"]].concat())
            .unwrap_or_else(|_| panic!("cleanup preview failed; tenant details withheld"));
        assert!(
            preview["data"]["dry_run"] == true && preview["data"]["destructive"] == true,
            "invalid destructive cleanup preview; tenant details withheld"
        );
        self.cleanup().unwrap_or_else(|_| {
            panic!("cleanup failed for a test-created resource; tenant details withheld")
        });
    }

    fn cleanup(&mut self) -> Result<(), String> {
        let args: Vec<&str> = self.delete_args.iter().map(String::as_str).collect();
        let result = (self.runner)(&args)?;
        if result["data"]["status"] != "deleted" {
            return Err("cleanup did not confirm deletion; tenant details withheld".to_string());
        }
        self.disarm();
        Ok(())
    }
}

impl<R: Fn(&[&str]) -> Response> Drop for CreatedResource<R> {
    fn drop(&mut self) {
        if self.active && self.cleanup().is_err() {
            if std::thread::panicking() {
                eprintln!("cleanup failed for a test-created resource; tenant details withheld");
            } else {
                panic!("cleanup failed for a test-created resource; tenant details withheld");
            }
        }
    }
}

pub fn wait_for_ready(
    read: impl FnMut() -> Response,
    ready: impl Fn(&Value) -> bool,
    timeout: Duration,
) -> Result<(), String> {
    wait_for_ready_with(
        read,
        ready,
        timeout,
        Duration::from_secs(2),
        Instant::now,
        std::thread::sleep,
    )
}

fn wait_for_ready_with(
    mut read: impl FnMut() -> Response,
    ready: impl Fn(&Value) -> bool,
    timeout: Duration,
    interval: Duration,
    mut now: impl FnMut() -> Instant,
    mut sleep: impl FnMut(Duration),
) -> Result<(), String> {
    if interval.is_zero() {
        return Err("poll interval must be positive".to_string());
    }
    let deadline = now() + timeout;
    loop {
        if ready(&read()?) {
            return Ok(());
        }
        let Some(remaining) = deadline
            .checked_duration_since(now())
            .filter(|value| !value.is_zero())
        else {
            return Err("resource readiness timed out; tenant details withheld".to_string());
        };
        sleep(interval.min(remaining));
        if now() >= deadline {
            return Err("resource readiness timed out; tenant details withheld".to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::{Cell, RefCell};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    const DELETE: &[&str] = &[
        "item",
        "delete",
        "--workspace",
        "fixture-workspace",
        "--id",
        "fixture-item",
    ];

    fn deleted() -> Value {
        json!({"data": {"status": "deleted"}})
    }
    fn preview() -> Value {
        json!({"data": {"dry_run": true, "destructive": true}})
    }

    #[test]
    fn parser_accepts_json_and_rejects_empty_or_invalid_success_output() {
        let body = json!({"data": {"id": "fixture-item"}});
        assert_eq!(
            parse_response(true, body.to_string().as_bytes(), b"private diagnostic").unwrap(),
            body
        );
        for invalid in [b"".as_slice(), b"private invalid payload".as_slice()] {
            assert_eq!(
                parse_response(true, invalid, b"").unwrap_err(),
                "successful command returned invalid JSON"
            );
        }
    }

    #[test]
    fn parser_only_exposes_allowlisted_error_codes() {
        for code in [
            "AUTH_REQUIRED",
            "FORBIDDEN",
            "NOT_FOUND",
            "API_ERROR",
            "INVALID_INPUT",
            "CONFLICT",
            "TIMEOUT",
            "CAPACITY_INACTIVE",
            "READONLY_MODE",
            "RATE_LIMITED",
            "NETWORK_ERROR",
            "private-code",
        ] {
            let body =
                json!({"error": {"code": code, "message": "private-body", "hint": "private-hint"}});
            let stderr = format!("[timing] private-diagnostic\r\n{body}\r\n");
            let error = parse_response(false, b"private-stdout", stderr.as_bytes()).unwrap_err();
            let expected = if code == "private-code" {
                "UNKNOWN_ERROR"
            } else {
                code
            };
            assert_eq!(
                error,
                format!("fabio failed with {expected} (tenant details withheld)")
            );
        }
        for stderr in ["private non-JSON stderr", "{}", r#"{"error":{"code":7}}"#] {
            assert_eq!(
                parse_response(false, b"", stderr.as_bytes()).unwrap_err(),
                "fabio failed with UNKNOWN_ERROR (tenant details withheld)"
            );
        }
    }

    #[test]
    fn guard_cleans_up_once_after_normal_scope_exit() {
        let calls = RefCell::new(Vec::new());
        {
            let _resource = CreatedResource::with_runner(DELETE, |args: &[&str]| {
                calls.borrow_mut().push(
                    args.iter()
                        .map(|arg| (*arg).to_string())
                        .collect::<Vec<_>>(),
                );
                Ok(deleted())
            });
        }
        assert_eq!(*calls.borrow(), vec![DELETE]);
    }

    #[test]
    fn process_launch_errors_do_not_disclose_the_program_path() {
        let directory = tempfile::tempdir().unwrap();
        let nonexistent = directory.path().join("private-missing-executable");
        let command = Command::new(&nonexistent);
        assert_eq!(execute(command).unwrap_err(), "failed to execute fabio");
    }

    #[test]
    fn envelope_helpers_validate_lists_fields_and_references() {
        let body = json!({"data": [{"id": "fixture-item"}, {"id": "other-item"}], "count": 2});
        assert_eq!(list(&body).len(), 2);
        assert!(contains_id(&body, "fixture-item"));
        assert!(!contains_id(&body, "missing-item"));
        assert_eq!(field(&body["data"][0], "id"), "fixture-item");
        assert_reference(&body["data"][0]["id"], "fixture-item");
        assert_eq!(list(&json!({"data": [], "count": 0})).len(), 0);
        for invalid in [
            json!({}),
            json!({"data": {"id": "private-id"}, "count": 1}),
            json!({"data": [], "count": 1}),
        ] {
            assert!(catch_unwind(|| list(&invalid)).is_err());
        }
    }

    #[test]
    fn field_and_reference_failures_do_not_print_tenant_values() {
        let body = json!({"id": {"private": "private-value"}});
        let panic = catch_unwind(|| field(&body, "id")).unwrap_err();
        let message = panic.downcast_ref::<String>().unwrap();
        assert_eq!(
            message,
            "expected string field id (tenant details withheld)"
        );
        for actual in [json!("private-id"), Value::Null, json!(7)] {
            let panic = catch_unwind(|| assert_reference(&actual, "expected-id")).unwrap_err();
            assert_eq!(
                panic.downcast_ref::<&str>().copied(),
                Some("resource reference mismatch; tenant details withheld")
            );
        }
    }

    #[test]
    fn polling_detects_a_read_that_exhausts_the_wait_budget() {
        let start = Instant::now();
        let elapsed = Cell::new(Duration::ZERO);
        let error = wait_for_ready_with(
            || {
                elapsed.set(Duration::from_secs(6));
                Ok(json!(false))
            },
            |_| false,
            Duration::from_secs(5),
            Duration::from_secs(2),
            || start + elapsed.get(),
            |_| panic!("overdue poll must not sleep"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "resource readiness timed out; tenant details withheld"
        );
    }

    #[test]
    fn guard_cleans_up_during_assertion_unwinding() {
        let calls = Cell::new(0);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _resource = CreatedResource::with_runner(DELETE, |_: &[&str]| {
                calls.set(calls.get() + 1);
                Ok(deleted())
            });
            panic!("synthetic test assertion");
        }));
        assert!(result.is_err());
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn explicit_delete_previews_and_disarms_without_double_deletion() {
        let calls = RefCell::new(Vec::new());
        {
            let mut resource = CreatedResource::with_runner(DELETE, |args: &[&str]| {
                calls.borrow_mut().push(
                    args.iter()
                        .map(|arg| (*arg).to_string())
                        .collect::<Vec<_>>(),
                );
                Ok(if args.last() == Some(&"--dry-run") {
                    preview()
                } else {
                    deleted()
                })
            });
            resource.delete();
            resource.delete();
        }
        assert_eq!(
            *calls.borrow(),
            vec![[DELETE, &["--dry-run"]].concat(), DELETE.to_vec()]
        );
    }

    #[test]
    fn disarmed_guard_never_calls_runner() {
        let calls = Cell::new(0);
        let mut resource = CreatedResource::with_runner(DELETE, |_: &[&str]| {
            calls.set(calls.get() + 1);
            Ok(deleted())
        });
        resource.disarm();
        resource.delete();
        drop(resource);
        assert_eq!(calls.get(), 0);
    }

    #[test]
    fn cleanup_failure_is_redacted_without_a_double_panic() {
        let result = catch_unwind(|| {
            let _resource = CreatedResource::with_runner(DELETE, |_: &[&str]| {
                Err("private cleanup details".to_string())
            });
        });
        let panic = result.unwrap_err();
        assert_eq!(
            panic.downcast_ref::<&str>().copied(),
            Some("cleanup failed for a test-created resource; tenant details withheld")
        );

        let calls = Cell::new(0);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _resource = CreatedResource::with_runner(DELETE, |_: &[&str]| {
                calls.set(calls.get() + 1);
                Err("private cleanup details".to_string())
            });
            panic!("original assertion");
        }));
        assert_eq!(
            result.unwrap_err().downcast_ref::<&str>().copied(),
            Some("original assertion")
        );
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn failed_explicit_cleanup_is_retried_by_the_guard() {
        for fail_preview in [false, true] {
            let mutations = Cell::new(0);
            let previews = Cell::new(0);
            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut resource = CreatedResource::with_runner(DELETE, |args: &[&str]| {
                    if args.last() == Some(&"--dry-run") {
                        previews.set(previews.get() + 1);
                        if fail_preview {
                            return Err("private preview error".to_string());
                        }
                        return Ok(preview());
                    }
                    mutations.set(mutations.get() + 1);
                    if !fail_preview && mutations.get() == 1 {
                        return Err("private delete error".to_string());
                    }
                    Ok(deleted())
                });
                resource.delete();
            }));
            assert!(result.is_err());
            assert_eq!(previews.get(), 1);
            assert_eq!(mutations.get(), if fail_preview { 1 } else { 2 });
        }
    }

    #[test]
    fn guard_rejects_invalid_preview_and_unconfirmed_deletion() {
        for invalid in [
            json!({"data": {"dry_run": false, "destructive": true}}),
            json!({"data": {"dry_run": true, "destructive": false}}),
        ] {
            let mutations = Cell::new(0);
            let result = catch_unwind(AssertUnwindSafe(|| {
                let mut resource = CreatedResource::with_runner(DELETE, |args: &[&str]| {
                    if args.last() == Some(&"--dry-run") {
                        return Ok(invalid.clone());
                    }
                    mutations.set(mutations.get() + 1);
                    Ok(deleted())
                });
                resource.delete();
            }));
            assert!(result.is_err());
            assert_eq!(mutations.get(), 1, "only the fallback guard should mutate");
        }
        let result = catch_unwind(|| {
            let _resource = CreatedResource::with_runner(DELETE, |_: &[&str]| {
                Ok(json!({"data": {"status": "not-deleted", "id": "private-id"}}))
            });
        });
        assert!(result.is_err());
    }

    #[test]
    fn polling_succeeds_immediately_without_sleeping() {
        let start = Instant::now();
        wait_for_ready_with(
            || Ok(json!(true)),
            |value| value == true,
            Duration::ZERO,
            Duration::from_secs(2),
            || start,
            |_| panic!("ready poll must not sleep"),
        )
        .unwrap();
    }

    #[test]
    fn polling_waits_for_readiness_with_a_fake_clock() {
        let start = Instant::now();
        let elapsed = Cell::new(Duration::ZERO);
        let reads = Cell::new(0);
        wait_for_ready_with(
            || {
                reads.set(reads.get() + 1);
                Ok(json!(reads.get() == 3))
            },
            |value| value == true,
            Duration::from_secs(5),
            Duration::from_secs(2),
            || start + elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
        )
        .unwrap();
        assert_eq!(reads.get(), 3);
        assert_eq!(elapsed.get(), Duration::from_secs(4));
    }

    #[test]
    fn polling_times_out_at_the_deadline_and_bounds_the_last_sleep() {
        let start = Instant::now();
        let elapsed = Cell::new(Duration::ZERO);
        let reads = Cell::new(0);
        let error = wait_for_ready_with(
            || {
                reads.set(reads.get() + 1);
                Ok(json!(false))
            },
            |value| value == true,
            Duration::from_secs(5),
            Duration::from_secs(2),
            || start + elapsed.get(),
            |duration| elapsed.set(elapsed.get() + duration),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "resource readiness timed out; tenant details withheld"
        );
        assert_eq!(reads.get(), 3);
        assert_eq!(elapsed.get(), Duration::from_secs(5));
    }

    #[test]
    fn polling_propagates_read_errors_without_retrying_or_sleeping() {
        let error = wait_for_ready_with(
            || Err("synthetic read failure".to_string()),
            |_| false,
            Duration::from_secs(5),
            Duration::from_secs(2),
            Instant::now,
            |_| panic!("failed poll must not sleep"),
        )
        .unwrap_err();
        assert_eq!(error, "synthetic read failure");
    }

    #[test]
    fn polling_rejects_zero_interval_and_expires_without_sleeping_at_zero_timeout() {
        let error = wait_for_ready_with(
            || panic!("invalid interval must not read"),
            |_| false,
            Duration::from_secs(5),
            Duration::ZERO,
            Instant::now,
            |_| panic!("invalid interval must not sleep"),
        )
        .unwrap_err();
        assert_eq!(error, "poll interval must be positive");
        let start = Instant::now();
        let error = wait_for_ready_with(
            || Ok(json!(false)),
            |_| false,
            Duration::ZERO,
            Duration::from_secs(2),
            || start,
            |_| panic!("expired poll must not sleep"),
        )
        .unwrap_err();
        assert_eq!(
            error,
            "resource readiness timed out; tenant details withheld"
        );
    }
}
