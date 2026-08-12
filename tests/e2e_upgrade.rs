use assert_cmd::Command;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

/// Returns true when a failed `upgrade --check` was caused by GitHub being
/// unreachable or rate-limited (network-restricted / shared-IP CI runners),
/// as opposed to a real bug. Detected from the structured error code on stderr,
/// with a legacy substring fallback.
fn github_check_unavailable(stderr: &str) -> bool {
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(stderr)
        && let Some(code) = json["error"]["code"].as_str()
    {
        return matches!(
            code,
            "RATE_LIMITED" | "NETWORK_ERROR" | "API_ERROR" | "TIMEOUT"
        );
    }
    stderr.contains("Failed to fetch latest release from GitHub")
        || stderr.contains("latest fabio release")
}

#[test]
fn upgrade_help_shows_usage() {
    fabio()
        .args(["upgrade", "--help"])
        .assert()
        .success()
        .stdout(predicates::str::contains(
            "Upgrade fabio to the latest release",
        ));
}

#[test]
fn upgrade_dry_run_shows_plan() {
    // --force needed because current version (dev) would refuse upgrade
    let assert = fabio()
        .args([
            "upgrade",
            "--dry-run",
            "--force",
            "--target-version",
            "0.23.0",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
    // Should show the target version and artifact name
    assert!(
        json["data"]["details"]["target_version"]
            .as_str()
            .unwrap()
            .contains("0.23.0")
    );
    assert!(
        json["data"]["details"]["artifact"]
            .as_str()
            .unwrap()
            .starts_with("fabio-")
    );
}

#[test]
fn upgrade_check_reports_version() {
    let output = fabio()
        .args(["upgrade", "--check"])
        .output()
        .expect("failed to run fabio");
    // Skip if GitHub API is unreachable (network-restricted CI runners)
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if github_check_unavailable(&stderr) {
            eprintln!("SKIP: GitHub API unreachable/rate-limited on this runner");
            return;
        }
        panic!(
            "Unexpected failure.\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Should have current_version and latest_version fields
    assert!(json["data"]["current_version"].is_string());
    assert!(json["data"]["latest_version"].is_string());
    assert!(json["data"]["update_available"].is_boolean());
}

#[test]
fn upgrade_dry_run_specific_version() {
    // --force needed because 0.23.0 < current version (would be a downgrade)
    let assert = fabio()
        .args([
            "upgrade",
            "--dry-run",
            "--force",
            "--target-version",
            "0.23.0",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["dry_run"], true);
    assert!(
        json["data"]["details"]["target_version"]
            .as_str()
            .unwrap()
            .contains("0.23.0")
    );
}

#[test]
fn upgrade_dry_run_with_v_prefix_version() {
    // Should strip the v prefix gracefully (--force needed for downgrade)
    let assert = fabio()
        .args([
            "upgrade",
            "--dry-run",
            "--force",
            "--target-version",
            "v0.23.0",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["details"]["target_version"], "0.23.0");
}

#[test]
fn upgrade_json_output() {
    let output = fabio()
        .args(["--output", "json", "upgrade", "--check"])
        .output()
        .expect("failed to run fabio");
    // Skip if GitHub API is unreachable (network-restricted CI runners)
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if github_check_unavailable(&stderr) {
            eprintln!("SKIP: GitHub API unreachable/rate-limited on this runner");
            return;
        }
        panic!(
            "Unexpected failure.\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Should be valid JSON
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"].is_object());
}

#[test]
fn upgrade_refuses_on_dev_build_even_with_target_version() {
    // On dev builds: "dev_build"; on release builds: "up_to_date" (0.0.1 < 0.25.0)
    let assert = fabio()
        .args(["upgrade", "--target-version", "0.0.1"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let status = json["data"]["status"].as_str().unwrap();
    assert!(
        status == "dev_build" || status == "up_to_date",
        "Unexpected status: {status}"
    );
}

#[test]
fn upgrade_check_reports_not_available_for_older() {
    // --check should report update_available: false when current >= latest
    let output = fabio()
        .args(["upgrade", "--check"])
        .output()
        .expect("failed to run fabio");
    // Skip if GitHub API is unreachable (network-restricted CI runners)
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if github_check_unavailable(&stderr) {
            eprintln!("SKIP: GitHub API unreachable/rate-limited on this runner");
            return;
        }
        panic!(
            "Unexpected failure.\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Current version (0.30.0-dev) is newer than last GitHub release
    assert_eq!(json["data"]["update_available"], false);
}

#[test]
fn upgrade_dev_build_refuses_without_force() {
    // Dev builds (version contains -dev) should refuse upgrade
    let output = fabio()
        .args(["upgrade"])
        .output()
        .expect("failed to run fabio");
    // Skip if GitHub API is unreachable (network-restricted CI runners)
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if github_check_unavailable(&stderr) {
            eprintln!("SKIP: GitHub API unreachable/rate-limited on this runner");
            return;
        }
        panic!(
            "Unexpected failure.\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let status = json["data"]["status"].as_str().unwrap();
    // On dev builds: "dev_build"; on release builds: "up_to_date" (since 0.30.0 > latest release)
    assert!(
        status == "dev_build" || status == "up_to_date",
        "Unexpected status: {status}"
    );
}

#[test]
fn upgrade_dev_build_check_still_works() {
    // --check should always work regardless of dev/release build
    let output = fabio()
        .args(["upgrade", "--check"])
        .output()
        .expect("failed to run fabio");
    // Skip if GitHub API is unreachable (network-restricted CI runners)
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if github_check_unavailable(&stderr) {
            eprintln!("SKIP: GitHub API unreachable/rate-limited on this runner");
            return;
        }
        panic!(
            "Unexpected failure.\nstderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"]["current_version"].as_str().is_some());
    assert!(json["data"]["latest_version"].as_str().is_some());
}
