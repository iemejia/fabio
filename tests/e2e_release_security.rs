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
