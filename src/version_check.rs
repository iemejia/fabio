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
//! Opt-out: set `FABIO_NO_VERSION_CHECK` to any value to disable the feature
//! entirely. Set `FABIO_NO_BACKGROUND_REFRESH` to keep the passive cached
//! notice but never spawn the network refresher (air-gapped / hermetic runs).
//!
//! Opt-in auto-upgrade: set `FABIO_AUTO_UPGRADE` (to a truthy value) so that when
//! the cached check finds a newer release, fabio spawns a detached
//! `fabio upgrade` in the background — the current command is unaffected and the
//! new binary takes effect on the *next* invocation. This is the true
//! "self-updating" behaviour, off by default because silently swapping a binary
//! under a running agent/CI is risky. It applies only to **standalone** installs
//! (the install method `fabio upgrade` owns; cargo/docker users update via their
//! package manager), is throttled to at most one attempt per
//! [`AUTO_UPGRADE_RETRY_INTERVAL_HOURS`] (so a persistently-failing upgrade never
//! hammers), and is disabled by `FABIO_NO_VERSION_CHECK` / `FABIO_NO_BACKGROUND_REFRESH`.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::cli::{Cli, Command};

const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// How old the cache may be before a background refresh is triggered.
const REFRESH_INTERVAL_HOURS: i64 = 24;

/// Minimum time between opt-in auto-upgrade attempts. Without this throttle, once
/// the cache knows a newer version EVERY subsequent command (until the binary is
/// replaced) — or every command while an upgrade keeps failing (offline,
/// read-only install dir) — would spawn another `fabio upgrade`.
const AUTO_UPGRADE_RETRY_INTERVAL_HOURS: i64 = 1;

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

/// Path to the auto-upgrade attempt marker (`~/.fabio/auto-upgrade-attempt`).
/// Holds a single RFC 3339 timestamp — the throttle for opt-in auto-upgrade.
fn auto_upgrade_marker_path() -> Option<PathBuf> {
    home::home_dir().map(|home| home.join(".fabio").join("auto-upgrade-attempt"))
}

/// Prime the version-check for this invocation.
///
/// Cheap: reads one small local file (and, at most once per
/// [`REFRESH_INTERVAL_HOURS`], writes back a small timestamp and spawns a
/// detached refresher). No network I/O on this path.
pub fn prime(cli: &Cli) {
    let agent_gated = should_check(cli);
    let auto_upgrade = auto_upgrade_requested(cli);
    // Run the (cheap, cached) check if EITHER surface needs it: the agent-gated
    // passive notice, or opt-in auto-upgrade (which is not agent-gated — a user
    // who sets FABIO_AUTO_UPGRADE wants a current binary regardless).
    if !agent_gated && !auto_upgrade {
        return;
    }
    if let Some(cache) = read_cache() {
        if crate::commands::upgrade::is_version_newer(&cache.latest_version, CURRENT_VERSION) {
            // Opt-in: launch a background upgrade (standalone installs only,
            // throttled). Do this BEFORE building the notice so the notice can
            // report that an auto-upgrade is already in flight.
            let auto_upgrade_launched = auto_upgrade && maybe_spawn_auto_upgrade();
            if agent_gated {
                let notice = build_notice(
                    CURRENT_VERSION,
                    &cache.latest_version,
                    auto_upgrade_launched,
                );
                if let Ok(mut guard) = PENDING_NOTICE.lock() {
                    *guard = Some(notice);
                }
            }
        }
        if is_stale(&cache) {
            // Bump the attempt timestamp BEFORE spawning (preserving the
            // known version). This is the throttle/backoff: if the refresh
            // is slow or fails (offline, rate-limited), subsequent
            // invocations see a fresh timestamp and do NOT re-spawn a check
            // for another full interval. Without this, a persistently
            // failing check would spawn a GitHub request on *every* command.
            // Only spawn if we could persist the attempt — an un-throttleable
            // check (read-only home) must not run, or the runaway returns.
            if write_cache(&cache.latest_version) {
                spawn_background_refresh();
            }
        }
    } else {
        // No cache yet. Record the attempt time immediately (empty version
        // is a placeholder until a successful check writes the real one) so
        // that (a) a burst of near-simultaneous invocations — an agent or a
        // parallel `cargo test` run — does not each spawn a check, and
        // (b) a check that keeps failing does not re-spawn on every future
        // invocation. At most one refresh per interval, success or failure.
        // If the attempt cannot be persisted (no/read-only home), skip the
        // spawn: an un-throttleable check would hammer GitHub on every command.
        if write_cache("") {
            spawn_background_refresh();
        }
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

/// Whether opt-in auto-upgrade is requested for this invocation.
///
/// Enabled by a truthy `FABIO_AUTO_UPGRADE`, but never inside the `upgrade`
/// command itself (that IS the upgrade — and it is what the background child
/// runs, so this prevents recursion) and never when the whole feature is opted
/// out via `FABIO_NO_VERSION_CHECK`. Not agent-gated: an explicit opt-in should
/// keep the binary current whether or not the caller is an agent.
fn auto_upgrade_requested(cli: &Cli) -> bool {
    env_flag_enabled("FABIO_AUTO_UPGRADE")
        && std::env::var_os("FABIO_NO_VERSION_CHECK").is_none()
        && !matches!(cli.command, Command::Upgrade { .. })
}

/// Whether an environment variable is set to a truthy value. Absent, empty, and
/// the usual falsey spellings (`0`, `false`, `no`, `off`) are all disabled; any
/// other value enables. Case-insensitive.
fn env_flag_enabled(var: &str) -> bool {
    std::env::var(var).is_ok_and(|v| is_truthy(&v))
}

/// Pure truthiness classifier for [`env_flag_enabled`].
fn is_truthy(value: &str) -> bool {
    !matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

/// If a background auto-upgrade should run now, record the attempt and spawn it.
/// Returns `true` when a `fabio upgrade` child was launched.
///
/// Gated to **standalone** installs (the method `fabio upgrade` owns; a cargo
/// binary should be updated via `cargo install --force`, a docker image via
/// `docker pull`), and throttled to at most one attempt per
/// [`AUTO_UPGRADE_RETRY_INTERVAL_HOURS`] via a marker file written BEFORE the
/// spawn — so concurrent invocations and a persistently-failing upgrade cannot
/// launch a storm of upgrade processes.
fn maybe_spawn_auto_upgrade() -> bool {
    if detect_install_method() != InstallMethod::Standalone {
        return false;
    }
    if !auto_upgrade_throttle_allows(chrono::Utc::now()) {
        return false;
    }
    // Record the attempt first; only spawn if it persisted (an un-throttleable
    // attempt on a read-only home must not run, mirroring the refresh throttle).
    if !record_auto_upgrade_attempt() {
        return false;
    }
    spawn_background_upgrade()
}

/// Whether the auto-upgrade throttle permits an attempt now (no marker, or the
/// last attempt is older than [`AUTO_UPGRADE_RETRY_INTERVAL_HOURS`]).
fn auto_upgrade_throttle_allows(now: chrono::DateTime<chrono::Utc>) -> bool {
    let Some(path) = auto_upgrade_marker_path() else {
        return false; // no home dir → cannot throttle → do not spawn
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return true; // no marker yet → allowed
    };
    auto_upgrade_due_at(contents.trim(), now)
}

/// Pure throttle decision for testing: a missing/malformed timestamp is "due",
/// otherwise due once the interval has elapsed.
fn auto_upgrade_due_at(last_attempt: &str, now: chrono::DateTime<chrono::Utc>) -> bool {
    chrono::DateTime::parse_from_rfc3339(last_attempt).map_or(true, |then| {
        now.signed_duration_since(then.with_timezone(&chrono::Utc))
            .num_hours()
            >= AUTO_UPGRADE_RETRY_INTERVAL_HOURS
    })
}

/// Persist the auto-upgrade attempt timestamp. Returns `true` if written.
fn record_auto_upgrade_attempt() -> bool {
    let Some(path) = auto_upgrade_marker_path() else {
        return false;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, chrono::Utc::now().to_rfc3339()).is_ok() {
        return std::fs::rename(&tmp, &path).is_ok();
    }
    false
}

/// Spawn a detached `fabio upgrade` to replace the binary for the next
/// invocation. Fire-and-forget: the current command never waits and is
/// unaffected if it fails. Guards on the child prevent any recursion (the child
/// runs `upgrade`, which suppresses the check, and the env vars belt-and-suspender
/// it). Returns `true` if the child was spawned.
///
/// Disabled by `FABIO_NO_BACKGROUND_REFRESH` — the same air-gap escape hatch that
/// stops the network refresher also stops the (network) auto-upgrade.
fn spawn_background_upgrade() -> bool {
    if std::env::var_os("FABIO_NO_BACKGROUND_REFRESH").is_some() {
        return false;
    }
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("fabio"));
    std::process::Command::new(exe)
        .arg("upgrade")
        .env("FABIO_NO_VERSION_CHECK", "1")
        .env("FABIO_NO_BACKGROUND_REFRESH", "1")
        .env("FABIO_AUTO_UPGRADE", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Read and parse the cache file, or `None` if missing/unreadable/corrupt.
fn read_cache() -> Option<VersionCache> {
    let path = cache_path()?;
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Persist the latest known version to the cache. Returns `true` if the cache
/// was written to disk, `false` if it could not be (no home dir, read-only
/// home, sandbox). Best-effort — never errors out.
///
/// Called from `upgrade --check` (which has just fetched the latest release),
/// so a manual check and the background refresher both warm the same cache.
/// The boolean lets [`prime`] avoid spawning a refresh it cannot throttle.
pub fn write_cache(latest_version: &str) -> bool {
    let Some(path) = cache_path() else {
        return false;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let cache = VersionCache {
        last_checked: chrono::Utc::now().to_rfc3339(),
        latest_version: latest_version.to_string(),
    };
    let Ok(serialized) = serde_json::to_string(&cache) else {
        return false;
    };
    // Write to a temp file then rename for an atomic-ish replace.
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &serialized).is_ok() {
        return std::fs::rename(&tmp, &path).is_ok();
    }
    false
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
///
/// Skipped entirely when `FABIO_NO_BACKGROUND_REFRESH` is set — an escape hatch
/// for air-gapped environments (and hermetic tests) that keeps the local cache
/// bookkeeping and the passive notice, but never launches a network child.
fn spawn_background_refresh() {
    if std::env::var_os("FABIO_NO_BACKGROUND_REFRESH").is_some() {
        return;
    }
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
fn build_notice(current: &str, latest: &str, auto_upgrade_launched: bool) -> Value {
    let method = detect_install_method();
    notice_value(
        current,
        latest,
        method,
        crate::agent::version_update_notice(latest),
        auto_upgrade_launched,
    )
}

/// Pure constructor for the notice JSON, for deterministic testing.
fn notice_value(
    current: &str,
    latest: &str,
    method: InstallMethod,
    agent_notice: Option<String>,
    auto_upgrade_launched: bool,
) -> Value {
    let mut obj = json!({
        "current": current,
        "latest": latest,
        "installMethod": method_name(method),
        "upgradeCommand": upgrade_command(method),
    });
    if auto_upgrade_launched {
        // FABIO_AUTO_UPGRADE is on and a background `fabio upgrade` was launched;
        // it takes effect on the next invocation. Signals the agent that it does
        // NOT need to tell the user to run the upgrade command manually.
        obj["autoUpgrade"] = Value::String("launched".to_string());
    }
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
// On non-Linux targets the body reduces to `false`, which is const-eligible;
// the Linux body does filesystem I/O and is not, so keep it a plain fn everywhere.
#[cfg_attr(not(target_os = "linux"), allow(clippy::missing_const_for_fn))]
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
            false,
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
        assert!(v.get("autoUpgrade").is_none());
    }

    #[test]
    fn notice_value_omits_agent_notice_when_absent() {
        let v = notice_value("0.60.0", "0.63.0", InstallMethod::Standalone, None, false);
        assert!(v.get("agentNotice").is_none());
        assert_eq!(v["upgradeCommand"], "fabio upgrade");
    }

    #[test]
    fn notice_value_flags_auto_upgrade_when_launched() {
        let v = notice_value("0.60.0", "0.63.0", InstallMethod::Standalone, None, true);
        assert_eq!(v["autoUpgrade"], "launched");
    }

    #[test]
    fn env_flag_truthiness() {
        assert!(is_truthy("1"));
        assert!(is_truthy("true"));
        assert!(is_truthy("yes"));
        assert!(is_truthy("ON"));
        assert!(is_truthy("anything-else"));
        assert!(!is_truthy(""));
        assert!(!is_truthy("0"));
        assert!(!is_truthy("false"));
        assert!(!is_truthy("No"));
        assert!(!is_truthy(" off "));
    }

    #[test]
    fn auto_upgrade_throttle_boundary() {
        let now = chrono::Utc::now();
        let recent = (now - chrono::Duration::minutes(30)).to_rfc3339();
        let old =
            (now - chrono::Duration::hours(AUTO_UPGRADE_RETRY_INTERVAL_HOURS + 1)).to_rfc3339();
        let exactly =
            (now - chrono::Duration::hours(AUTO_UPGRADE_RETRY_INTERVAL_HOURS)).to_rfc3339();
        assert!(!auto_upgrade_due_at(&recent, now)); // throttled
        assert!(auto_upgrade_due_at(&old, now)); // due
        assert!(auto_upgrade_due_at(&exactly, now)); // due at the boundary
        assert!(auto_upgrade_due_at("not-a-timestamp", now)); // malformed → due
        assert!(auto_upgrade_due_at("", now));
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
