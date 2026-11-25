//! Deterministic guard against publishing before the security gates pass.

use serde_yaml::Value;

fn workflow(source: &str) -> Value {
    serde_yaml::from_str(source).expect("valid workflow YAML")
}

fn commands(job: &Value) -> Vec<&str> {
    job["steps"]
        .as_sequence()
        .expect("job steps")
        .iter()
        .filter_map(|step| step["run"].as_str())
        .collect()
}

#[test]
fn release_requires_strict_audits_and_security_regressions_before_artifacts() {
    let release = workflow(include_str!("../.github/workflows/release.yml"));
    let steps = commands(&release["jobs"]["validate"]);
    for required in [
        "cargo audit --deny warnings",
        "npm --prefix docs ci --ignore-scripts",
        "npm --prefix docs audit --audit-level=low",
        "npm --prefix docs test",
        "cargo test --locked",
    ] {
        assert!(
            steps.contains(&required),
            "missing mandatory gate: {required}"
        );
    }
    assert_eq!(release["jobs"]["build"]["needs"], "validate");
    assert_eq!(release["jobs"]["release"]["needs"], "build");
    assert_eq!(release["jobs"]["docker"]["needs"], "build");
    let build_steps = commands(&release["jobs"]["build"]);
    assert!(build_steps.contains(&"cargo build --locked --release --target ${{ matrix.target }}"));
    for step in release["jobs"]["validate"]["steps"].as_sequence().unwrap() {
        assert_ne!(step["continue-on-error"].as_bool(), Some(true));
    }
}

#[test]
fn normal_ci_and_docs_also_fail_on_dependency_findings() {
    let ci = workflow(include_str!("../.github/workflows/ci.yml"));
    assert!(commands(&ci["jobs"]["audit"]).contains(&"cargo audit --deny warnings"));
    let docs = workflow(include_str!("../.github/workflows/docs.yml"));
    assert!(commands(&docs["jobs"]["build"]).contains(&"npm audit --audit-level=low"));
}

#[test]
fn dependabot_groups_all_security_updates_per_ecosystem() {
    let dependabot = workflow(include_str!("../.github/dependabot.yml"));
    for ecosystem in ["cargo", "npm"] {
        let entry = dependabot["updates"]
            .as_sequence()
            .unwrap()
            .iter()
            .find(|entry| entry["package-ecosystem"] == ecosystem)
            .unwrap();
        let group = &entry["groups"][format!("{ecosystem}-security")];
        assert_eq!(group["applies-to"], "security-updates");
        assert_eq!(group["patterns"][0], "*");
        assert!(entry["target-branch"].is_null());
    }
}

#[test]
fn cargo_remediation_only_publishes_validated_lockfile_changes_from_default_branch() {
    let source = include_str!("../.github/workflows/cargo-security-updates.yml");
    let config = workflow(source);
    assert!(config["on"].get("workflow_dispatch").is_some());
    assert_eq!(config["on"]["schedule"][0]["cron"], "0 7 * * *");
    assert!(config["on"].get("pull_request_target").is_none());
    assert!(config["on"].get("pull_request").is_none());
    assert_eq!(config["permissions"]["contents"], "read");
    assert_eq!(config["concurrency"]["cancel-in-progress"], false);
    let steps = config["jobs"]["remediate"]["steps"].as_sequence().unwrap();
    let checkout = &steps[0];
    assert_eq!(
        checkout["with"]["ref"],
        "${{ github.event.repository.default_branch }}"
    );
    assert_eq!(checkout["with"]["persist-credentials"], false);
    let validation = steps
        .iter()
        .position(|step| step["name"] == "Validate remediation")
        .unwrap();
    let upload = steps
        .iter()
        .position(|step| step["name"] == "Upload validated remediation")
        .unwrap();
    assert!(validation < upload);
    let run = steps[validation]["run"].as_str().unwrap();
    for gate in [
        "cargo audit --deny warnings",
        "cargo fmt -- --check",
        "cargo clippy --tests --locked -- -D warnings",
        "cargo test --locked",
    ] {
        assert!(run.contains(gate), "missing gate: {gate}");
    }
    assert!(
        !serde_yaml::to_string(&config["jobs"]["remediate"])
            .unwrap()
            .contains("SECURITY_UPDATES_TOKEN")
    );
    let publisher = &config["jobs"]["publish"];
    assert_eq!(publisher["needs"], "remediate");
    assert_eq!(publisher["if"], "needs.remediate.outputs.changed == 'true'");
    let publish_steps = publisher["steps"].as_sequence().unwrap();
    let publish = publish_steps.last().unwrap();
    assert_eq!(publish["with"]["add-paths"], "Cargo.lock");
    assert_eq!(
        publish["with"]["branch"],
        "automation/cargo-security-updates"
    );
    assert_eq!(
        publish["with"]["token"],
        "${{ secrets.SECURITY_UPDATES_TOKEN }}"
    );
    assert!(
        commands(publisher)
            .iter()
            .any(|command| command.contains("$AUDITED_SHA"))
    );
    assert!(
        commands(publisher)
            .iter()
            .all(|command| !command.contains("cargo "))
    );
    for step in steps.iter().chain(publish_steps) {
        assert_ne!(step["continue-on-error"].as_bool(), Some(true));
        if let Some(action) = step["uses"].as_str() {
            let (_, revision) = action.split_once('@').unwrap();
            assert_eq!(revision.len(), 40, "action must be SHA-pinned: {action}");
            assert!(revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
    }
    assert!(!source.contains("--ignore "));
    assert!(!source.contains("gh pr merge"));
    let ci = workflow(include_str!("../.github/workflows/ci.yml"));
    assert!(
        commands(&ci["jobs"]["security-remediation-tests"])
            .iter()
            .any(|command| command.contains("test_cargo_security_updates.py"))
    );
}

#[test]
fn local_release_script_audits_before_committing_or_tagging() {
    let script = include_str!("../scripts/release.sh");
    let commit = script.find("git commit").expect("commit step");
    for gate in [
        "cargo audit --deny warnings",
        "npm --prefix docs audit --audit-level=low",
        "npm --prefix docs test",
    ] {
        assert!(script.find(gate).expect("mandatory gate") < commit);
    }
    assert!(!script.contains("cargo check --quiet 2>/dev/null || true"));
}
