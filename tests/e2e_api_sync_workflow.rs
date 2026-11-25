//! Hermetic contracts for API-sync concurrency, credentials and publication guards.

const SOURCE: &str = include_str!("../.github/workflows/sync-fabric-api-specs.yml");

fn workflow() -> serde_yaml::Value {
    serde_yaml::from_str(SOURCE).unwrap()
}

#[test]
fn api_sync_workflow_serializes_runs_and_checks_prior_days_before_generation() {
    let workflow = workflow();
    assert_eq!(workflow["concurrency"]["cancel-in-progress"], false);
    let steps = workflow["jobs"]["sync-specs"]["steps"]
        .as_sequence()
        .unwrap();
    let gate = steps
        .iter()
        .position(|step| step["id"] == "open-sync")
        .unwrap();
    assert!(
        steps[gate]["run"]
            .as_str()
            .unwrap()
            .contains("startswith(\"sync/fabric-api-specs-\")")
    );
    for name in [
        "Checkout fabric-rest-api-specs",
        "Check for spec changes since last cursor",
    ] {
        let index = steps.iter().position(|step| step["name"] == name).unwrap();
        assert!(gate < index);
        assert_eq!(
            steps[index]["if"],
            "steps.open-sync.outputs.exists != 'true'"
        );
    }
}

#[test]
fn api_sync_checkouts_do_not_persist_credentials_or_use_publishing_token() {
    let workflow = workflow();
    assert_eq!(workflow["permissions"]["contents"], "read");
    assert_eq!(workflow["permissions"]["pull-requests"], "read");
    let steps = workflow["jobs"]["sync-specs"]["steps"]
        .as_sequence()
        .unwrap();
    let checkouts: Vec<_> = steps
        .iter()
        .filter(|step| {
            step["uses"]
                .as_str()
                .is_some_and(|action| action.starts_with("actions/checkout@"))
        })
        .collect();
    assert_eq!(checkouts.len(), 2);
    for checkout in &checkouts {
        assert_eq!(checkout["with"]["persist-credentials"], false);
        assert!(checkout["with"]["token"].is_null());
    }
    assert_eq!(
        checkouts[0]["with"]["ref"],
        "${{ github.event.repository.default_branch }}"
    );
    assert!(workflow["jobs"]["sync-specs"]["env"].is_null());
}

#[test]
fn api_sync_records_and_checks_the_actual_base_before_generation() {
    let workflow = workflow();
    let steps = workflow["jobs"]["sync-specs"]["steps"]
        .as_sequence()
        .unwrap();
    let base = steps.iter().position(|step| step["id"] == "base").unwrap();
    let gate = steps
        .iter()
        .position(|step| step["name"] == "Check generation base before Copilot")
        .unwrap();
    let generate = steps
        .iter()
        .position(|step| step["name"] == "Generate improvements with Copilot")
        .unwrap();
    assert!(base < gate && gate < generate);
    assert!(
        steps[base]["run"]
            .as_str()
            .unwrap()
            .contains("git rev-parse HEAD")
    );
    assert_eq!(steps[gate]["env"]["GH_TOKEN"], "${{ github.token }}");
    assert_eq!(
        steps[gate]["env"]["BASE_SHA"],
        "${{ steps.base.outputs.sha }}"
    );
    assert_eq!(
        steps[gate]["if"],
        "steps.check-changes.outputs.has_changes == 'true'"
    );
}

#[test]
fn both_api_sync_publish_paths_verify_base_and_authenticate_git_before_push() {
    let workflow = workflow();
    let steps = workflow["jobs"]["sync-specs"]["steps"]
        .as_sequence()
        .unwrap();
    for name in [
        "Create pull request",
        "Create cursor-only PR (no code changes)",
    ] {
        let step = steps.iter().find(|step| step["name"] == name).unwrap();
        assert_eq!(step["env"]["GH_TOKEN"], "${{ secrets.COPILOT_PAT }}");
        assert_eq!(step["env"]["BASE_SHA"], "${{ steps.base.outputs.sha }}");
        assert_eq!(
            step["env"]["DEFAULT_BRANCH"],
            "${{ github.event.repository.default_branch }}"
        );
        let run = step["run"].as_str().unwrap();
        let commit = run.find("git commit").unwrap();
        let gate = run.find("python3 scripts/check-api-sync-base.py").unwrap();
        let auth = run.find("gh auth setup-git --hostname github.com").unwrap();
        let push = run.find("git push").unwrap();
        assert!(commit < gate && gate < auth && auth < push);
        assert!(run.contains("--base-sha \"$BASE_SHA\" --default-branch \"$DEFAULT_BRANCH\""));
        assert!(run.contains("if [ -z \"$GH_TOKEN\" ]; then"));
        assert!(run.contains("--base \"$DEFAULT_BRANCH\""));
        assert_ne!(step["continue-on-error"].as_bool(), Some(true));
    }
    let ci: serde_yaml::Value =
        serde_yaml::from_str(include_str!("../.github/workflows/ci.yml")).unwrap();
    assert!(
        ci["jobs"]["security-remediation-tests"]["steps"]
            .as_sequence()
            .unwrap()
            .iter()
            .any(|step| step["run"]
                .as_str()
                .is_some_and(|run| run.contains("test_api_sync_base.py")))
    );
}
