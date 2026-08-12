//! Passive, agent-facing "a newer fabio is available" notice.
//!
//! Modeled on `gh`/`az`/`gogcli`: a locally-cached, throttled check that never
//! blocks the command and never performs a synchronous network request on the
//! hot path. It differs from those human-facing CLIs in three deliberate ways
//! for fabio's agent-first design:
//!
//! 1. **Agent-gated, not TTY-gated.** `gh`/`az` suppress the notice in
//!    non-interactive/CI contexts — exactly where agents run. fabio does the
//!    opposite: it emits *only* when an AI agent is detected
//!    ([`crate::agent::detect_agent`]).
//! 2. **Delivered in the JSON envelope**, not as stderr prose, because that is
//!    what the agent reliably parses (see [`crate::output`]).
//! 3. **Schema-refresh nudge.** An outdated agent is usually operating on a
//!    stale cached command schema, so the notice tells it to re-run
//!    `fabio context agent` after upgrading.
//!
//! Flow (per invocation):
//! - [`prime`] reads the cache file (the only I/O on the hot path). If a newer
//!   version is known, it stashes a notice that [`take_notice`] later injects
//!   into the first JSON success envelope.
//! - If the cache is missing or older than [`REFRESH_INTERVAL_HOURS`], a
//!   detached `fabio upgrade --check` child is spawned fire-and-forget to
//!   refresh the cache for the *next* invocation. The current command never
//!   waits on it and is unaffected if it fails (offline, sandbox, etc.).
//!
//! Cache file: `~/.fabio/version-check.json` (same directory as the token cache
//! and job ledger; resolved via `home::home_dir()` for Windows compatibility).
//!
//! Opt-out: set `FABIO_NO_VERSION_CHECK` to any value.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{Cli, Command};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How old the cache may be before a background refresh is triggered.
const REFRESH_INTERVAL_HOURS: i64 = 24;

/// Notice built during [`prime`], consumed once by [`take_notice`] so that only
/// the first JSON envelope in a process carries it (avoids duplication in
/// commands that render multiple objects).
static PENDING_NOTICE: Mutex<Option<Value>> = Mutex::new(None);

/// On-disk cache of the last known latest release.
#[derive(Debug, Serialize, Deserialize)]
struct VersionCache {
    /// RFC 3339 timestamp of when the latest version was last fetched.
    last_checked: String,
    /// The latest release version (no leading `v`), e.g. `"0.63.0"`.
    latest_version: String,
}

/// How fabio was installed, inferred from the running executable's path and
/// runtime environment. Determines the upgrade command surfaced to the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstallMethod {
    Cargo,
    Docker,
    Standalone,
}

/// Path to the version-check cache file (`~/.fabio/version-check.json`).
fn cache_path() -> Option<PathBuf> {
    home::home_dir().map(|home| home.join(".fabio").join("version-check.json"))
}

/// Prime the version-check for this invocation.
///
/// Cheap: reads one small local file and (at most once per
/// [`REFRESH_INTERVAL_HOURS`]) spawns a detached refresher. No network I/O.
pub fn prime(cli: &Cli) {
    if !should_check(cli) {
        return;
    }
    match read_cache() {
        Some(cache) => {
            if crate::commands::upgrade::is_version_newer(&cache.latest_version, CURRENT_VERSION) {
                let notice = build_notice(CURRENT_VERSION, &cache.latest_version);
                if let Ok(mut guard) = PENDING_NOTICE.lock() {
                    *guard = Some(notice);
                }
            }
            if is_stale(&cache) {
                spawn_background_refresh();
            }
        }
        None => spawn_background_refresh(),
    }
}

/// Take the pending update notice, if any. Called by the JSON output path so the
/// notice appears exactly once (in the first JSON envelope of the process).
pub fn take_notice() -> Option<Value> {
    PENDING_NOTICE
        .lock()
        .ok()
        .and_then(|mut guard| guard.take())
}

/// Whether the version check should run for this invocation.
fn should_check(cli: &Cli) -> bool {
    // Suppressed when opted out, quiet (no stdout anyway), or inside the
    // `upgrade` command (avoids noise + child recursion, since the background
    // refresher itself invokes `upgrade --check`).
    let suppressed = std::env::var_os("FABIO_NO_VERSION_CHECK").is_some()
        || cli.quiet
        || matches!(cli.command, Command::Upgrade { .. });
    should_check_core(suppressed, crate::agent::detect_agent().is_some())
}

/// Pure decision core for [`should_check`], for deterministic testing: the check
/// runs only when it is not suppressed and an AI agent is the caller.
const fn should_check_core(suppressed: bool, agent_detected: bool) -> bool {
    !suppressed && agent_detected
}

/// Read and parse the cache file, or `None` if missing/unreadable/corrupt.
fn read_cache() -> Option<VersionCache> {
    let path = cache_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Persist the latest known version to the cache (best-effort, never errors out).
///
/// Called from `upgrade --check` (which has just fetched the latest release),
/// so a manual check and the background refresher both warm the same cache.
pub fn write_cache(latest_version: &str) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = VersionCache {
        last_checked: chrono::Utc::now().to_rfc3339(),
        latest_version: latest_version.to_string(),
    };
    let Ok(serialized) = serde_json::to_string(&cache) else {
        return;
    };
    // Write to a temp file then rename for an atomic-ish replace.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &serialized).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}

/// Whether the cache is old enough to warrant a background refresh.
fn is_stale(cache: &VersionCache) -> bool {
    is_stale_at(&cache.last_checked, chrono::Utc::now())
}

/// Pure staleness check against a reference time, for deterministic testing.
/// A malformed timestamp is treated as stale.
fn is_stale_at(last_checked: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::DateTime::parse_from_rfc3339(last_checked).map_or(true, |then| {
        now.signed_duration_since(then.with_timezone(&chrono::Utc))
            .num_hours()
            >= REFRESH_INTERVAL_HOURS
    })
}

/// Spawn a detached `fabio upgrade --check` to refresh the cache for the next
/// invocation. Fire-and-forget: the current process does not wait on it, and
/// any failure (offline, sandbox that blocks spawning, read-only home) is
/// silently ignored. `FABIO_NO_VERSION_CHECK=1` on the child prevents it from
/// recursively spawning another refresher.
fn spawn_background_refresh() {
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("fabio"));
    let _ = std::process::Command::new(exe)
        .args(["upgrade", "--check"])
        .env("FABIO_NO_VERSION_CHECK", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

/// Build the `updateAvailable` envelope object for a known-newer release.
fn build_notice(current: &str, latest: &str) -> Value {
    let method = detect_install_method();
    notice_value(
        current,
        latest,
        method,
        crate::agent::version_update_notice(latest),
    )
}

/// Pure constructor for the notice JSON, for deterministic testing.
fn notice_value(
    current: &str,
    latest: &str,
    method: InstallMethod,
    agent_notice: Option<String>,
) -> Value {
    let mut obj = json!({
        "current": current,
        "latest": latest,
        "installMethod": method_name(method),
        "upgradeCommand": upgrade_command(method),
    });
    if let Some(notice) = agent_notice {
        obj["agentNotice"] = Value::String(notice);
    }
    obj
}

/// Detect how the running binary was installed.
fn detect_install_method() -> InstallMethod {
    let exe = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    classify_install_method(&exe, running_in_docker())
}

/// Pure classifier for [`detect_install_method`], for deterministic testing.
fn classify_install_method(exe_path_lower: &str, in_docker: bool) -> InstallMethod {
    if in_docker {
        return InstallMethod::Docker;
    }
    // `cargo install` places binaries under `<CARGO_HOME>/bin`, typically
    // `~/.cargo/bin/fabio`.
    if exe_path_lower.contains(".cargo") {
        return InstallMethod::Cargo;
    }
    InstallMethod::Standalone
}

/// Best-effort container detection (Linux only; other platforms return false).
fn running_in_docker() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/.dockerenv").exists() {
            return true;
        }
        if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
            let lower = cgroup.to_ascii_lowercase();
            return lower.contains("docker")
                || lower.contains("kubepods")
                || lower.contains("containerd");
        }
        false
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// The `installMethod` label surfaced to the agent.
const fn method_name(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::Cargo => "cargo",
        InstallMethod::Docker => "docker",
        InstallMethod::Standalone => "standalone",
    }
}

/// The upgrade command appropriate for the detected install method.
const fn upgrade_command(method: InstallMethod) -> &'static str {
    match method {
        InstallMethod::Cargo => "cargo install --git https://github.com/iemejia/fabio.git --force",
        InstallMethod::Docker => "docker pull ghcr.io/iemejia/fabio:latest",
        InstallMethod::Standalone => "fabio upgrade",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_check_requires_agent_and_no_suppression() {
        // Runs only when not suppressed AND an agent is the caller.
        assert!(should_check_core(false, true));
        assert!(!should_check_core(true, true)); // suppressed (opt-out/quiet/upgrade)
        assert!(!should_check_core(false, false)); // no agent
        assert!(!should_check_core(true, false));
    }

    #[test]
    fn stale_when_older_than_interval() {
        let now = chrono::Utc::now();
        let fresh = (now - chrono::Duration::hours(1)).to_rfc3339();
        let old = (now - chrono::Duration::hours(REFRESH_INTERVAL_HOURS + 1)).to_rfc3339();
        assert!(!is_stale_at(&fresh, now));
        assert!(is_stale_at(&old, now));
    }

    #[test]
    fn stale_at_exact_interval_boundary() {
        let now = chrono::Utc::now();
        let exactly = (now - chrono::Duration::hours(REFRESH_INTERVAL_HOURS)).to_rfc3339();
        assert!(is_stale_at(&exactly, now));
    }

    #[test]
    fn malformed_timestamp_is_stale() {
        assert!(is_stale_at("not-a-timestamp", chrono::Utc::now()));
        assert!(is_stale_at("", chrono::Utc::now()));
    }

    #[test]
    fn install_method_classification() {
        assert_eq!(
            classify_install_method("/home/u/.cargo/bin/fabio", false),
            InstallMethod::Cargo
        );
        assert_eq!(
            classify_install_method("/usr/local/bin/fabio", false),
            InstallMethod::Standalone
        );
        // Docker takes precedence over path heuristics.
        assert_eq!(
            classify_install_method("/home/u/.cargo/bin/fabio", true),
            InstallMethod::Docker
        );
        assert_eq!(
            classify_install_method("/usr/local/bin/fabio", true),
            InstallMethod::Docker
        );
    }

    #[test]
    fn upgrade_command_matches_method() {
        assert!(upgrade_command(InstallMethod::Cargo).starts_with("cargo install"));
        assert_eq!(
            upgrade_command(InstallMethod::Docker),
            "docker pull ghcr.io/iemejia/fabio:latest"
        );
        assert_eq!(upgrade_command(InstallMethod::Standalone), "fabio upgrade");
    }

    #[test]
    fn notice_value_shape_with_agent() {
        let v = notice_value(
            "0.60.0",
            "0.63.0",
            InstallMethod::Cargo,
            Some("hello agent".to_string()),
        );
        assert_eq!(v["current"], "0.60.0");
        assert_eq!(v["latest"], "0.63.0");
        assert_eq!(v["installMethod"], "cargo");
        assert!(
            v["upgradeCommand"]
                .as_str()
                .unwrap()
                .contains("cargo install")
        );
        assert_eq!(v["agentNotice"], "hello agent");
    }

    #[test]
    fn notice_value_omits_agent_notice_when_absent() {
        let v = notice_value("0.60.0", "0.63.0", InstallMethod::Standalone, None);
        assert!(v.get("agentNotice").is_none());
        assert_eq!(v["upgradeCommand"], "fabio upgrade");
    }

    #[test]
    fn version_cache_roundtrips() {
        let cache = VersionCache {
            last_checked: "2026-08-12T09:00:00+00:00".to_string(),
            latest_version: "0.63.0".to_string(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        let parsed: VersionCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.latest_version, "0.63.0");
        assert_eq!(parsed.last_checked, "2026-08-12T09:00:00+00:00");
    }
}
