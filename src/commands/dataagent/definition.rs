use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// Get the definition of a data agent (data sources, instructions, etc.).
pub(super) async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/dataAgents/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

/// Update the definition of a data agent (configure data sources, instructions, etc.).
pub(super) async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let definition_json = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(inline)) => inline.to_string(),
        (None, None) => {
            return Err(
                FabioError::invalid_input("Either --file or --content must be provided").into(),
            );
        }
    };

    let body: Value = serde_json::from_str(&definition_json).map_err(|e| {
        FabioError::new(
            ErrorCode::InvalidInput,
            format!("Invalid JSON definition: {e}"),
        )
    })?;

    // If the body already has a "definition" wrapper, use as-is; otherwise wrap it
    let request_body = if body.get("definition").is_some() {
        body
    } else {
        serde_json::json!({ "definition": body })
    };

    if output::dry_run_guard(
        cli,
        "data-agent update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "updateMetadata": update_metadata,
        }),
    ) {
        return Ok(());
    }

    let path = if update_metadata {
        format!("/workspaces/{workspace}/dataAgents/{id}/updateDefinition?updateMetadata=True")
    } else {
        format!("/workspaces/{workspace}/dataAgents/{id}/updateDefinition")
    };

    let data = client.post(&path, &request_body, true).await?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// Publish a data agent using the dedicated staging publish endpoint.
///
/// Uses: `POST /workspaces/{ws}/dataAgents/{id}/staging/publish`
///
/// This promotes the staging (draft) configuration to published state in a single
/// API call — replacing the previous approach of manually copying definition parts.
pub(super) async fn publish(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    description: Option<&str>,
    to_m365: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent publish",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "description": description,
            "toM365": to_m365,
        }),
    ) {
        return Ok(());
    }

    // Build publish request body
    let mut body = serde_json::json!({});
    if let Some(desc) = description {
        body["publishedDescription"] = Value::from(desc);
    }

    // POST to the staging publish endpoint
    client
        .post(
            &format!("/workspaces/{workspace}/dataAgents/{id}/staging/publish"),
            &body,
            false,
        )
        .await?;

    // Fetch published settings to get the published URL (now official endpoint)
    let settings_path = format!("/workspaces/{workspace}/dataAgents/{id}/settings");
    let published_url = client.get(&settings_path).await.ok().and_then(|s| {
        s.get("publishedUrl")
            .or_else(|| s.get("aiInstructions").and(None)) // ensure we only pick publishedUrl
            .and_then(Value::as_str)
            .filter(|u| !u.is_empty())
            .map(String::from)
    });

    let mut obj = serde_json::json!({
        "id": id,
        "status": "published",
        "description": description.unwrap_or(""),
    });

    if let Some(url) = published_url {
        obj["publishedUrl"] = Value::from(url);
    }

    // M365 Copilot Agent Store publishing is not available via the public REST API.
    // The SDK implements it against the internal workload host
    // (`{wl_host}/webapi/capacities/{cap}/workloads/ML/DataAgent/Automatic/v1/.../metaosapppackage`),
    // which is discovered via `synapse.ml.fabric.service_discovery` inside a Fabric
    // notebook and has no `api.fabric.microsoft.com` equivalent (every candidate
    // public path returns 404 — verified live). Report as unsupported if requested.
    if to_m365 {
        obj["m365Status"] = Value::from("unsupported");
        obj["m365Error"] = Value::from(
            "M365 Copilot Agent Store publishing is not available via the public Fabric REST API \
             (the SDK uses an internal workload-host endpoint only reachable from a Fabric \
             notebook runtime). Publish to M365 from the Fabric portal, or use the \
             fabric-data-agent-sdk Python package inside a notebook.",
        );
    }

    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Reset staging configuration (discard all draft changes, revert to published state).
///
/// Uses: `POST /workspaces/{ws}/dataAgents/{id}/staging/reset`
pub(super) async fn reset(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "data-agent reset",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
        }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/dataAgents/{id}/staging/reset"),
            &serde_json::json!({}),
            false,
        )
        .await?;

    let result = serde_json::json!({
        "id": id,
        "status": "staging_reset",
    });
    output::render_object(cli, &result, "status");
    Ok(())
}
