use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum VariableLibraryCommand {
    /// List variable librarys in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a variable library
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID
        #[arg(long)]
        id: String,
    },
    /// Create a new variable library
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update variable library properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a variable library
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a variable library
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a variable library
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// List value sets defined in a variable library
    #[command(display_order = 8)]
    ListValueSets {
        /// Workspace ID or name
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID or name
        #[arg(long)]
        id: String,
    },
    /// Activate a value set for a variable library in a workspace
    ///
    /// Sets the active value set so that items reading variables will get
    /// the values from the specified set. This is a workspace-level setting:
    /// the same variable library definition can have different active value
    /// sets in different workspaces (e.g., "dev", "test", "prod").
    #[command(display_order = 9)]
    ActivateValueSet {
        /// Workspace ID or name
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Variable library ID or name
        #[arg(long)]
        id: String,
        /// Name of the value set to activate
        #[arg(long)]
        value_set: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &VariableLibraryCommand,
) -> Result<()> {
    match command {
        VariableLibraryCommand::List { workspace } => list(cli, client, workspace).await,
        VariableLibraryCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        VariableLibraryCommand::Create {
            workspace,
            name,
            description,
            sensitivity_label,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                sensitivity_label.as_deref(),
            )
            .await
        }
        VariableLibraryCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        VariableLibraryCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        VariableLibraryCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        VariableLibraryCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
            )
            .await
        }
        VariableLibraryCommand::ListValueSets { workspace, id } => {
            list_value_sets(cli, client, workspace, id).await
        }
        VariableLibraryCommand::ActivateValueSet {
            workspace,
            id,
            value_set,
        } => activate_value_set(cli, client, workspace, id, value_set).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/variableLibraries"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_item_list(
        cli,
        &resp.items,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/variableLibraries/{id}"))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({ "displayName": name });
    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }
    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    if output::dry_run_guard(
        cli,
        "variable-library create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description , "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/variableLibraries"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one of --name or --description must be provided".to_string(),
            "Example: fabio variable-library update --workspace <WS> --id <ID> --name \"New Name\""
                .to_string(),
        )
        .into());
    }
    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }
    if output::dry_run_guard(cli, "variable-library update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/variableLibraries/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "variable-library delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }
    let url = if hard_delete {
        format!("/workspaces/{workspace}/variableLibraries/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/variableLibraries/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library delete", "Contributor"))?;
    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/variableLibraries/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library get-definition", "Contributor"))?;
    if decode {
        let decoded = output::decode_definition_parts(data);
        output::render_object(cli, &decoded, "definition");
    } else {
        output::render_object(cli, &data, "definition");
    }
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let script = match (file, content) {
        (Some(path), _) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (_, Some(c)) => c.to_string(),
        (None, None) => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Either --file or --content must be provided".to_string(),
                "Example: fabio variable-library update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    // Accept EITHER a full definition envelope (JSON with definition.parts —
    // needed to author settings.json + valueSets/* alongside variables.json) OR
    // a raw variables.json (wrapped as the single variables.json part).
    let body = crate::definition_spec::build_update_definition_body(&script, "variables.json");
    if output::dry_run_guard(
        cli,
        "variable-library update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/variableLibraries/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// List value sets defined in a variable library by inspecting its definition.
///
/// The Fabric API does not expose a dedicated list-value-sets endpoint;
/// value sets are stored as definition parts (one JSON file per set in
/// the `valueSets/` path). We fetch the definition, decode the parts,
/// and extract value set names along with their override counts.
async fn list_value_sets(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    // First, get the variable library properties to find the active value set
    let vl_info = client
        .get(&format!("/workspaces/{workspace}/variableLibraries/{id}"))
        .await?;
    let active_set = vl_info
        .pointer("/properties/activeValueSetName")
        .and_then(|v| v.as_str())
        .unwrap_or("Default");

    // Fetch the definition to enumerate value sets
    let data = client
        .post(
            &format!("/workspaces/{workspace}/variableLibraries/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library list-value-sets", "Contributor"))?;

    let mut value_sets: Vec<Value> = Vec::new();

    // Always include "Default" as the base value set.
    // The API returns activeValueSetName as "Default value set" for the default.
    let is_default_active =
        active_set.eq_ignore_ascii_case("Default value set") || active_set.is_empty();
    value_sets.push(serde_json::json!({
        "name": "Default",
        "active": is_default_active,
        "type": "default"
    }));

    // Parse definition parts to find value set files (path: "valueSets/<name>.json")
    if let Some(parts) = data.pointer("/definition/parts").and_then(|v| v.as_array()) {
        for part in parts {
            let path = part.get("path").and_then(|v| v.as_str()).unwrap_or("");
            if let Some(set_name) = path
                .strip_prefix("valueSets/")
                .and_then(|s| s.strip_suffix(".json"))
            {
                // Decode payload to count overrides
                let override_count = part
                    .get("payload")
                    .and_then(|v| v.as_str())
                    .and_then(|encoded| BASE64.decode(encoded).ok())
                    .and_then(|bytes| String::from_utf8(bytes).ok())
                    .and_then(|json_str| serde_json::from_str::<Value>(&json_str).ok())
                    .and_then(|val| {
                        // Value set format: {"variableOverrides": [...]} or legacy {"values": [...]}
                        val.get("variableOverrides")
                            .or_else(|| val.get("values"))
                            .and_then(|v| v.as_array())
                            .map(Vec::len)
                            .or_else(|| val.as_array().map(Vec::len))
                    })
                    .unwrap_or(0);

                value_sets.push(serde_json::json!({
                    "name": set_name,
                    "active": set_name == active_set,
                    "type": "alternative",
                    "overrides": override_count
                }));
            }
        }
    }

    output::render_list_with_token(
        cli,
        &value_sets,
        &["name", "active", "type", "overrides"],
        &["NAME", "ACTIVE", "TYPE", "OVERRIDES"],
        "name",
        None,
    );
    Ok(())
}

/// Activate a value set for a variable library.
///
/// Uses PATCH /workspaces/{ws}/variableLibraries/{id} with
/// `properties.activeValueSetName` to switch the active value set.
/// This is a workspace-level setting — deploying the same definition
/// to different workspaces allows each to have a different active set.
async fn activate_value_set(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    value_set: &str,
) -> Result<()> {
    let body = serde_json::json!({
        "properties": {
            "activeValueSetName": value_set
        }
    });

    if output::dry_run_guard(
        cli,
        "variable-library activate-value-set",
        &serde_json::json!({ "workspace": workspace, "id": id, "valueSet": value_set }),
    ) {
        return Ok(());
    }

    let data = client
        .patch(
            &format!("/workspaces/{workspace}/variableLibraries/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "variable-library activate-value-set", "Contributor"))?;

    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn update_definition_wraps_raw_variables_json() {
        // A raw variables.json is wrapped as a single variables.json part.
        let raw = r#"{"variables":[{"name":"x","type":"String","value":"1"}]}"#;
        let body = crate::definition_spec::build_update_definition_body(raw, "variables.json");
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "variables.json");
    }

    #[test]
    fn update_definition_passes_through_multipart_envelope() {
        // A full envelope (variables.json + settings.json + valueSets/*) needed to
        // author alternate value sets is passed through verbatim.
        let env = r#"{"definition":{"parts":[
            {"path":"variables.json","payload":"e30=","payloadType":"InlineBase64"},
            {"path":"settings.json","payload":"e30=","payloadType":"InlineBase64"},
            {"path":"valueSets/Prod.json","payload":"e30=","payloadType":"InlineBase64"}
        ]}}"#;
        let body = crate::definition_spec::build_update_definition_body(env, "variables.json");
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 3);
        let paths: Vec<&str> = parts.iter().filter_map(|p| p["path"].as_str()).collect();
        assert!(paths.contains(&"settings.json"));
        assert!(paths.contains(&"valueSets/Prod.json"));
    }
}
