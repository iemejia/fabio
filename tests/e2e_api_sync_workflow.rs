//! Hermetic contract for preventing overlapping generated API-sync PRs.

#[test]
fn api_sync_workflow_serializes_runs_and_checks_prior_days_before_generation() {
    let workflow: serde_yaml::Value = serde_yaml::from_str(include_str!(
        "../.github/workflows/sync-fabric-api-specs.yml"
    ))
    .unwrap();
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
