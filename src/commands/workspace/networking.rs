use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::read_json_body;

pub(super) async fn get_network_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-network-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_network_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let raw = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio workspace set-network-policy --workspace <WS> --file policy.json"
                    .to_string(),
            )
            .into());
        }
    };

    let body: Value =
        serde_json::from_str(&raw).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?;

    if output::dry_run_guard(cli, "workspace set-network-policy", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!("/workspaces/{workspace}/networking/communicationPolicy"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace set-network-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn get_firewall_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/inbound/firewall"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-firewall-rules", "Viewer"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_firewall_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace set-firewall-rules")?;

    if output::dry_run_guard(cli, "workspace set-firewall-rules", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!("/workspaces/{workspace}/networking/communicationPolicy/inbound/firewall"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace set-firewall-rules", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn get_git_outbound_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/outbound/git"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-git-outbound-policy", "Viewer"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_git_outbound_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace set-git-outbound-policy")?;

    if output::dry_run_guard(cli, "workspace set-git-outbound-policy", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!("/workspaces/{workspace}/networking/communicationPolicy/outbound/git"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace set-git-outbound-policy", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn get_inbound_azure_resource_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/inbound/azureResourceInstances"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-inbound-azure-resource-rules", "Viewer"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_inbound_azure_resource_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace set-inbound-azure-resource-rules")?;

    if output::dry_run_guard(cli, "workspace set-inbound-azure-resource-rules", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!(
                "/workspaces/{workspace}/networking/communicationPolicy/inbound/azureResourceInstances"
            ),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "workspace set-inbound-azure-resource-rules", "Admin")
        })?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn get_outbound_cloud_connection_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/outbound/cloudConnections"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(e, "workspace get-outbound-cloud-connection-rules", "Viewer")
        })?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_outbound_cloud_connection_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(
        file,
        content,
        "workspace set-outbound-cloud-connection-rules",
    )?;

    if output::dry_run_guard(cli, "workspace set-outbound-cloud-connection-rules", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!(
                "/workspaces/{workspace}/networking/communicationPolicy/outbound/cloudConnections"
            ),
            &body,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "workspace set-outbound-cloud-connection-rules", "Admin")
        })?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

/// Get the inbound External Data Shares bypass policy for a workspace.
/// Returns `defaultAction` (Allow/Deny) plus an `etag` field (from the response
/// `ETag` header) that can be passed to `set-inbound-external-data-shares-policy --if-match`.
pub(super) async fn get_inbound_external_data_shares_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get_with_etag(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/inbound/externalDataShares"
        ))
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "workspace get-inbound-external-data-shares-policy",
                "Viewer",
            )
        })?;
    output::render_object(cli, &data, "defaultAction");
    Ok(())
}

/// Set the inbound External Data Shares bypass policy for a workspace (preview API).
/// Requires *admin* workspace role. Supports optimistic concurrency via `--if-match`.
pub(super) async fn set_inbound_external_data_shares_policy(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    default_action: &str,
    if_match: Option<&str>,
) -> Result<()> {
    let body = serde_json::json!({ "defaultAction": default_action });

    if output::dry_run_guard(
        cli,
        "workspace set-inbound-external-data-shares-policy",
        &body,
    ) {
        return Ok(());
    }

    let data = client
        .put_with_if_match(
            &format!(
                "/workspaces/{workspace}/networking/communicationPolicy/inbound/externalDataShares"
            ),
            &body,
            if_match,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(
                e,
                "workspace set-inbound-external-data-shares-policy",
                "Admin",
            )
        })?;
    output::render_object(cli, &data, "etag");
    Ok(())
}

pub(super) async fn get_outbound_gateway_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/networking/communicationPolicy/outbound/gateways"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "workspace get-outbound-gateway-rules", "Viewer"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}

pub(super) async fn set_outbound_gateway_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_body(file, content, "workspace set-outbound-gateway-rules")?;

    if output::dry_run_guard(cli, "workspace set-outbound-gateway-rules", &body) {
        return Ok(());
    }

    let data = client
        .put(
            &format!("/workspaces/{workspace}/networking/communicationPolicy/outbound/gateways"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "workspace set-outbound-gateway-rules", "Admin"))?;
    output::render_object(cli, &data, "workspaceId");
    Ok(())
}
