use anyhow::Result;
use serde_json::json;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::enrich_admin;
use crate::output;

pub(super) async fn activate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    policy_set_id: &str,
    allow_replace: bool,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(policy_set_id, "--policy-set-id")?;
    let details = json!({
        "workspace": workspace,
        "policySetId": policy_set_id,
        "allowReplace": allow_replace,
        "scope": "Tenant",
    });
    if output::dry_run_guard_maybe_destructive(
        cli,
        "admin activate-policy-set",
        &details,
        allow_replace,
    ) {
        return Ok(());
    }
    let suffix = if allow_replace {
        "?allowReplace=true"
    } else {
        ""
    };
    client
        .post(
            &format!("/workspaces/{workspace}/policySets/{policy_set_id}/activate{suffix}"),
            &json!({}),
            false,
        )
        .await
        .map_err(|error| enrich_admin(error, "admin activate-policy-set"))?;
    output::render_object(
        cli,
        &json!({"id": policy_set_id, "status": "active", "scope": "Tenant"}),
        "status",
    );
    Ok(())
}

pub(super) async fn deactivate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    policy_set_id: &str,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(policy_set_id, "--policy-set-id")?;
    if output::dry_run_guard(
        cli,
        "admin deactivate-policy-set",
        &json!({"workspace": workspace, "policySetId": policy_set_id, "scope": "Tenant"}),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/policySets/{policy_set_id}/deactivate"),
            &json!({}),
            false,
        )
        .await
        .map_err(|error| enrich_admin(error, "admin deactivate-policy-set"))?;
    output::render_object(
        cli,
        &json!({"id": policy_set_id, "status": "inactive", "scope": "Tenant"}),
        "status",
    );
    Ok(())
}

pub(super) async fn get_active(cli: &Cli, client: &FabricClient) -> Result<()> {
    let data = client
        .get("/policySets/active?beta=true")
        .await
        .map_err(|error| enrich_admin(error, "admin get-active-policy-set"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}
