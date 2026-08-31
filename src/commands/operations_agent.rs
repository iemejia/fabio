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
pub enum OperationsAgentCommand {
    /// List operations agents in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a operations agent
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
    },
    /// Create a new operations agent
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
    /// Update operations agent properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a operations agent
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a operations agent
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a operations agent
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Start the operations agent so it evaluates its rules (sets shouldRun=true)
    #[command(display_order = 8)]
    Start {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
    },
    /// Stop the operations agent so it stops evaluating its rules (sets shouldRun=false)
    #[command(display_order = 9)]
    Stop {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
    },
    /// Show whether the operations agent is running (reads shouldRun from its definition)
    #[command(display_order = 10)]
    Status {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Operations agent ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &OperationsAgentCommand,
) -> Result<()> {
    match command {
        OperationsAgentCommand::List { workspace } => list(cli, client, workspace).await,
        OperationsAgentCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        OperationsAgentCommand::Create {
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
        OperationsAgentCommand::Update {
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
        OperationsAgentCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        OperationsAgentCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        OperationsAgentCommand::UpdateDefinition {
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
        OperationsAgentCommand::Start { workspace, id } => {
            set_running(cli, client, workspace, id, true).await
        }
        OperationsAgentCommand::Stop { workspace, id } => {
            set_running(cli, client, workspace, id, false).await
        }
        OperationsAgentCommand::Status { workspace, id } => {
            status(cli, client, workspace, id).await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "operationsAgents",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "operationsAgents", workspace, id).await
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
        "operations-agent create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description , "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "operations-agent create", "Contributor"))?;
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
            "Example: fabio operations-agent update --workspace <WS> --id <ID> --name \"New Name\""
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
    if output::dry_run_guard(cli, "operations-agent update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/operationsAgents/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "operations-agent update", "Contributor"))?;
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
    crate::commands::crud::delete(
        cli,
        client,
        "operations-agent",
        "operationsAgents",
        "Contributor",
        workspace,
        id,
        hard_delete,
    )
    .await
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    crate::commands::crud::get_definition(
        cli,
        client,
        "operations-agent",
        "operationsAgents",
        "Contributor",
        workspace,
        id,
        decode,
    )
    .await
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
                "Example: fabio operations-agent update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, "Configurations.json");
    if output::dry_run_guard(
        cli,
        "operations-agent update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "operations-agent update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// Locate and decode the `Configurations.json` part from a `getDefinition` response.
///
/// The operations-agent definition is a single `Configurations.json` part whose
/// base64 payload contains `{"$schema":..,"configuration":{..},"shouldRun":bool}`.
/// Returns the parsed configuration JSON so callers can inspect or flip `shouldRun`.
fn extract_configuration(def: &Value) -> Result<Value> {
    let parts = def
        .get("definition")
        .and_then(|d| d.get("parts"))
        .and_then(|p| p.as_array())
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "getDefinition response has no definition.parts array".to_string(),
            )
        })?;
    let payload = parts
        .iter()
        .find(|p| p.get("path").and_then(Value::as_str) == Some("Configurations.json"))
        .and_then(|p| p.get("payload"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::new(
                ErrorCode::ApiError,
                "operations-agent definition has no Configurations.json part".to_string(),
            )
        })?;
    let decoded = BASE64.decode(payload).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("failed to base64-decode Configurations.json: {e}"),
        )
    })?;
    let config: Value = serde_json::from_slice(&decoded).map_err(|e| {
        FabioError::new(
            ErrorCode::ApiError,
            format!("Configurations.json is not valid JSON: {e}"),
        )
    })?;
    Ok(config)
}

/// Read the current `shouldRun` activation flag from a decoded configuration.
/// Absent/invalid values are treated as `false` (stopped).
fn read_should_run(config: &Value) -> bool {
    config
        .get("shouldRun")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

/// Set `shouldRun` on a decoded operations-agent configuration, returning the
/// previous value (if any). Pure so it can be unit-tested without a network.
fn set_should_run(config: &mut Value, run: bool) -> Option<bool> {
    let previous = config.get("shouldRun").and_then(Value::as_bool);
    config["shouldRun"] = Value::Bool(run);
    previous
}

/// Actionable hint that suggests the exact command to activate a stopped agent.
fn start_hint(workspace: &str, id: &str) -> String {
    format!("Activate it with: fabio operations-agent start --workspace {workspace} --id {id}")
}

/// Actionable hint shown when Fabric refuses to activate an agent because it has
/// no configured data source / playbook: configure it, then start.
fn configure_then_start_hint(workspace: &str, id: &str) -> String {
    format!(
        "Configure a data source and playbook first (fabio operations-agent update-definition --workspace {workspace} --id {id} --file <Configurations.json>), then re-run: fabio operations-agent start --workspace {workspace} --id {id}"
    )
}

/// Start or stop the agent by flipping `shouldRun` in its `Configurations.json`
/// definition. Fabric has no dedicated start/stop endpoint — activation is a
/// property of the definition — so this reads the definition, edits it, and
/// writes it back via `updateDefinition`.
async fn set_running(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    run: bool,
) -> Result<()> {
    let action = if run {
        "operations-agent start"
    } else {
        "operations-agent stop"
    };
    if output::dry_run_guard(
        cli,
        action,
        &serde_json::json!({ "workspace": workspace, "id": id, "shouldRun": run }),
    ) {
        return Ok(());
    }

    let def = client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, action, "Contributor"))?;
    let mut config = extract_configuration(&def)?;
    set_should_run(&mut config, run);

    let serialized = serde_json::to_string(&config)?;
    let encoded = BASE64.encode(serialized.as_bytes());
    let body = serde_json::json!({
        "definition": { "parts": [{ "path": "Configurations.json", "payload": encoded, "payloadType": "InlineBase64" }] }
    });
    client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, action, "Contributor"))?;

    // Re-read the persisted definition: Fabric coerces `shouldRun` back to
    // false for an agent that has no configured data source / playbook, so the
    // requested value is not necessarily what was persisted. Report the truth.
    let after = client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, action, "Contributor"))?;
    let actual = read_should_run(&extract_configuration(&after)?);

    let status = if actual { "started" } else { "stopped" };
    let mut obj = serde_json::json!({
        "id": id,
        "requestedShouldRun": run,
        "shouldRun": actual,
        "status": status,
    });
    if run && !actual {
        obj["note"] = Value::from(
            "Fabric did not activate the agent. Operations agents only start once they have a configured data source (eventhouse or ontology) and a generated playbook. Configure the agent (operations-agent update-definition) before starting.",
        );
        obj["hint"] = Value::from(configure_then_start_hint(workspace, id));
    }
    output::render_object(cli, &obj, "status");
    Ok(())
}

/// Report whether the operations agent is currently running by reading the
/// `shouldRun` flag from its definition.
async fn status(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let def = client
        .post(
            &format!("/workspaces/{workspace}/operationsAgents/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "operations-agent status", "Viewer"))?;
    let config = extract_configuration(&def)?;
    let should_run = read_should_run(&config);
    let mut obj = serde_json::json!({
        "id": id,
        "shouldRun": should_run,
        "state": if should_run { "running" } else { "stopped" },
    });
    if !should_run {
        obj["hint"] = Value::from(start_hint(workspace, id));
    }
    output::render_object(cli, &obj, "state");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded_definition(should_run: bool) -> Value {
        let config = serde_json::json!({
            "$schema": "https://developer.microsoft.com/json-schemas/fabric/item/operationsAgents/definition/1.0.0/schema.json",
            "configuration": { "goals": "", "instructions": "", "dataSources": {}, "actions": {} },
            "shouldRun": should_run,
        });
        let payload = BASE64.encode(serde_json::to_string(&config).unwrap().as_bytes());
        serde_json::json!({
            "definition": {
                "parts": [
                    { "path": "Configurations.json", "payload": payload, "payloadType": "InlineBase64" },
                    { "path": ".platform", "payload": "e30=", "payloadType": "InlineBase64" }
                ]
            }
        })
    }

    #[test]
    fn extract_configuration_reads_shouldrun() {
        let def = encoded_definition(true);
        let config = extract_configuration(&def).unwrap();
        assert!(read_should_run(&config));

        let def = encoded_definition(false);
        let config = extract_configuration(&def).unwrap();
        assert!(!read_should_run(&config));
    }

    #[test]
    fn extract_configuration_errors_without_part() {
        let def = serde_json::json!({ "definition": { "parts": [
            { "path": ".platform", "payload": "e30=", "payloadType": "InlineBase64" }
        ] } });
        let err = extract_configuration(&def).unwrap_err();
        assert!(err.to_string().contains("Configurations.json"));
    }

    #[test]
    fn extract_configuration_errors_without_definition() {
        let def = serde_json::json!({ "id": "abc" });
        assert!(extract_configuration(&def).is_err());
    }

    #[test]
    fn set_should_run_flips_and_returns_previous() {
        let mut config = serde_json::json!({ "shouldRun": false, "configuration": {} });
        let previous = set_should_run(&mut config, true);
        assert_eq!(previous, Some(false));
        assert_eq!(config["shouldRun"], Value::Bool(true));
        assert!(read_should_run(&config));
    }

    #[test]
    fn set_should_run_inserts_when_absent() {
        let mut config = serde_json::json!({ "configuration": {} });
        let previous = set_should_run(&mut config, true);
        assert_eq!(previous, None);
        assert_eq!(config["shouldRun"], Value::Bool(true));
    }

    #[test]
    fn read_should_run_defaults_to_false() {
        assert!(!read_should_run(&serde_json::json!({})));
        assert!(!read_should_run(&serde_json::json!({ "shouldRun": "yes" })));
    }

    #[test]
    fn start_hint_suggests_start_command() {
        let hint = start_hint("ws-1", "id-2");
        assert!(hint.contains("fabio operations-agent start"));
        assert!(hint.contains("--workspace ws-1"));
        assert!(hint.contains("--id id-2"));
    }

    #[test]
    fn configure_then_start_hint_suggests_configure_and_start() {
        let hint = configure_then_start_hint("ws-1", "id-2");
        assert!(hint.contains("fabio operations-agent update-definition"));
        assert!(hint.contains("fabio operations-agent start"));
        assert!(hint.contains("--workspace ws-1"));
        assert!(hint.contains("--id id-2"));
    }

    #[test]
    fn set_running_roundtrip_preserves_configuration() {
        // Simulate the read-modify-write payload path used by start/stop.
        let def = encoded_definition(false);
        let mut config = extract_configuration(&def).unwrap();
        set_should_run(&mut config, true);
        let encoded = BASE64.encode(serde_json::to_string(&config).unwrap().as_bytes());
        let decoded: Value = serde_json::from_slice(&BASE64.decode(encoded).unwrap()).unwrap();
        assert!(read_should_run(&decoded));
        // Original configuration payload survives the flip.
        assert!(decoded["configuration"].is_object());
        assert!(decoded["$schema"].is_string());
    }
}
