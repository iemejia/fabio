use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[path = "reflex_mcp.rs"]
mod mcp;

#[derive(Debug, Subcommand)]
// The `create-rule` variant carries the full typed rule-authoring surface, so it
// is much larger than the CRUD variants; boxing clap-derive arg fields fights the
// derive, so allow the size disparity here.
#[allow(clippy::large_enum_variant)]
#[command(
    after_help = "Before using this command, run: fabio context examples reflex\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum ReflexCommand {
    /// List reflexes in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a reflex
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,
    },
    /// Create a new reflex
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex display name
        #[arg(long)]
        name: String,

        /// Optional description
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,
    },
    /// Update reflex properties (name and/or description)
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a reflex
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a reflex
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a reflex
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,

        /// Path to definition file
        #[arg(long)]
        file: Option<String>,

        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Create a trigger with auto-generated Reflex definition (KQL source + email/Teams alert)
    #[command(name = "create-trigger", display_order = 10)]
    CreateTrigger {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex display name
        #[arg(long)]
        name: String,

        /// Eventhouse item ID (the Eventhouse containing the KQL database)
        #[arg(long)]
        eventhouse_id: String,

        /// KQL database name
        #[arg(long)]
        database: String,

        /// KQL table name to monitor
        #[arg(long)]
        table: String,

        /// KQL condition expression (e.g., `EventType == 'Flood'`)
        #[arg(long)]
        condition: String,

        /// Alert action type: `email` or `teams`
        #[arg(long, value_parser = ["email", "teams"])]
        action: String,

        /// Comma-separated recipient email addresses
        #[arg(long)]
        recipients: String,

        /// Optional custom alert message
        #[arg(long)]
        message: Option<String>,

        /// Query execution interval in seconds (default: 60)
        #[arg(long, default_value = "60")]
        interval: u32,
    },

    /// Configure a KQL data source (portal-only operation)
    #[command(name = "configure-kql-source", display_order = 20)]
    ConfigureKqlSource {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex ID
        #[arg(long)]
        id: String,
    },

    /// Create a monitoring rule in an existing reflex (via the Activator MCP server).
    ///
    /// Provide `--rule <json|@file>` for full control (the createRuleParams payload),
    /// or the typed flags for the common single-condition KQL case.
    #[command(name = "create-rule", display_order = 29)]
    CreateRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,

        /// Full createRuleParams JSON (inline, @file, or stdin). Takes precedence
        /// over the typed flags; fabio injects artifactId/workspaceId.
        #[arg(long, conflicts_with_all = ["kql", "column"])]
        rule: Option<String>,

        /// Rule name (typed mode)
        #[arg(long)]
        name: Option<String>,

        /// Rule description (typed mode; defaults to the name)
        #[arg(long)]
        description: Option<String>,

        /// KQL query text defining the monitored stream (typed mode)
        #[arg(long)]
        kql: Option<String>,

        /// Query execution interval in seconds (typed mode)
        #[arg(long, default_value_t = 300)]
        interval: u32,

        /// KQL DATABASE item id to monitor (NOT the eventhouse id — pass the KQL
        /// database inside the eventhouse). Mutually exclusive with --cluster.
        #[arg(long, visible_alias = "kql-database-id", conflicts_with = "cluster")]
        eventhouse_id: Option<String>,

        /// Workspace of the KQL database (defaults to --workspace)
        #[arg(long, visible_alias = "kql-database-workspace")]
        eventhouse_workspace: Option<String>,

        /// Azure Data Explorer cluster host (source; use with --database)
        #[arg(long)]
        cluster: Option<String>,

        /// Azure Data Explorer database name (use with --cluster)
        #[arg(long)]
        database: Option<String>,

        /// Column to group by for per-entity monitoring (required by CHANGE conditions)
        #[arg(long)]
        split_column: Option<String>,

        /// Column to monitor (typed mode)
        #[arg(long)]
        column: Option<String>,

        /// Condition function, e.g. isGreaterThan / increasesAbove / decreasesBelow / changesTo (typed mode)
        #[arg(long)]
        condition: Option<String>,

        /// Threshold value for the condition (number, boolean, or string) (typed mode)
        #[arg(long)]
        value: Option<String>,

        /// Action to trigger: email or teams (typed mode)
        #[arg(long, default_value = "email")]
        action: String,

        /// Recipient email(s), comma-separated (typed mode)
        #[arg(long, value_delimiter = ',')]
        recipients: Vec<String>,

        /// Email subject (email action; defaults to the name)
        #[arg(long)]
        subject: Option<String>,

        /// Message body (email body / Teams message; supports {columnName} placeholders)
        #[arg(long)]
        message: Option<String>,

        /// Headline shown in the alert (defaults to the name)
        #[arg(long)]
        headline: Option<String>,

        /// Message locale (typed mode)
        #[arg(long, default_value = "en-US")]
        locale: String,
    },

    /// Print the Activator remote MCP server URL for this reflex
    #[command(name = "mcp-url", display_order = 30)]
    McpUrl {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,
    },

    /// List monitoring rules in a reflex (via the Activator MCP server)
    #[command(name = "list-rules", display_order = 31)]
    ListRules {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,
    },

    /// Start (enable) a monitoring rule (via the Activator MCP server)
    #[command(name = "start-rule", display_order = 32)]
    StartRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,

        /// Rule ID to start (from `reflex list-rules`)
        #[arg(long)]
        rule_id: String,
    },

    /// Stop (disable) a monitoring rule (via the Activator MCP server)
    #[command(name = "stop-rule", display_order = 33)]
    StopRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,

        /// Rule ID to stop (from `reflex list-rules`)
        #[arg(long)]
        rule_id: String,
    },

    /// Delete a monitoring rule (via the Activator MCP server) — irreversible
    #[command(name = "delete-rule", display_order = 34)]
    DeleteRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,

        /// Rule ID to delete (from `reflex list-rules`)
        #[arg(long)]
        rule_id: String,
    },

    /// Show the activation (fired-alert) history for a rule (via the Activator MCP server)
    #[command(name = "rule-activations", display_order = 35)]
    RuleActivations {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Reflex (Activator artifact) ID
        #[arg(long)]
        id: String,

        /// Rule ID to inspect (from `reflex list-rules`)
        #[arg(long)]
        rule_id: String,

        /// Start of the window (ISO 8601; default: 24 hours ago)
        #[arg(long)]
        start_time: Option<String>,

        /// End of the window (ISO 8601; default: now)
        #[arg(long)]
        end_time: Option<String>,

        /// Maximum activations to return (default 100, max 2000)
        #[arg(long)]
        max_results: Option<u32>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &ReflexCommand) -> Result<()> {
    match command {
        ReflexCommand::List { workspace } => list(cli, client, workspace).await,
        ReflexCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        ReflexCommand::Create {
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
        ReflexCommand::Update {
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
        ReflexCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        ReflexCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        ReflexCommand::UpdateDefinition {
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
        ReflexCommand::CreateTrigger {
            workspace,
            name,
            eventhouse_id,
            database,
            table,
            condition,
            action,
            recipients,
            message,
            interval,
        } => {
            create_trigger(
                cli,
                client,
                workspace,
                name,
                eventhouse_id,
                database,
                table,
                condition,
                action,
                recipients,
                message.as_deref(),
                *interval,
            )
            .await
        }
        ReflexCommand::ConfigureKqlSource { .. } => Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "KQL source configuration is a portal-only operation.",
            "KQL sources always fail via REST API with 'importArtifactRequest field is required'. \
                 Configure the KQL source through the Fabric portal, then manage the definition \
                 programmatically with: fabio reflex get-definition / update-definition",
        )
        .into()),
        ReflexCommand::CreateRule {
            workspace,
            id,
            rule,
            name,
            description,
            kql,
            interval,
            eventhouse_id,
            eventhouse_workspace,
            cluster,
            database,
            split_column,
            column,
            condition,
            value,
            action,
            recipients,
            subject,
            message,
            headline,
            locale,
        } => {
            let spec = match (name, kql, column, condition, value) {
                (Some(name), Some(kql), Some(column), Some(condition), Some(value)) => {
                    Some(mcp::TypedRuleSpec {
                        name,
                        description: description.as_deref(),
                        kql,
                        interval: *interval,
                        eventhouse_id: eventhouse_id.as_deref(),
                        eventhouse_workspace: eventhouse_workspace.as_deref(),
                        cluster: cluster.as_deref(),
                        database: database.as_deref(),
                        split_column: split_column.as_deref(),
                        column,
                        condition,
                        value,
                        action,
                        recipients,
                        subject: subject.as_deref(),
                        message: message.as_deref(),
                        headline: headline.as_deref(),
                        locale,
                    })
                }
                _ => None,
            };
            mcp::create_rule(cli, client, workspace, id, rule.as_deref(), spec).await
        }
        ReflexCommand::McpUrl { workspace, id } => mcp::mcp_url(cli, client, workspace, id).await,
        ReflexCommand::ListRules { workspace, id } => {
            mcp::list_rules(cli, client, workspace, id).await
        }
        ReflexCommand::StartRule {
            workspace,
            id,
            rule_id,
        } => mcp::set_rule_state(cli, client, workspace, id, rule_id, true).await,
        ReflexCommand::StopRule {
            workspace,
            id,
            rule_id,
        } => mcp::set_rule_state(cli, client, workspace, id, rule_id, false).await,
        ReflexCommand::DeleteRule {
            workspace,
            id,
            rule_id,
        } => mcp::delete_rule(cli, client, workspace, id, rule_id).await,
        ReflexCommand::RuleActivations {
            workspace,
            id,
            rule_id,
            start_time,
            end_time,
            max_results,
        } => {
            mcp::rule_activations(
                cli,
                client,
                workspace,
                id,
                rule_id,
                start_time.as_deref(),
                end_time.as_deref(),
                *max_results,
            )
            .await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    crate::commands::crud::list(
        cli,
        client,
        "reflexes",
        workspace,
        &["displayName", "id", "description"],
        &["NAME", "ID", "DESCRIPTION"],
    )
    .await
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    crate::commands::crud::show(cli, client, "reflexes", workspace, id).await
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });
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
        "reflex create",
        &serde_json::json!({
            "workspace": workspace,
            "displayName": name,
            "description": description,
            "sensitivityLabel": sensitivity_label
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/reflexes"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "reflex create", "Member"))?;
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
            "Example: fabio reflex update --workspace <WS> --id <ID> --name \"New Name\""
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

    if output::dry_run_guard(cli, "reflex update", &body) {
        return Ok(());
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/reflexes/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "reflex update", "Contributor"))?;
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
        "reflex delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }

    let url = if hard_delete {
        format!("/workspaces/{workspace}/reflexes/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/reflexes/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "reflex delete", "Member"))?;

    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

// ─── Definitions ─────────────────────────────────────────────────────────────

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    decode: bool,
) -> Result<()> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/reflexes/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "reflex get-definition", "Contributor"))?;
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
                "Example: fabio reflex update-definition --workspace <WS> --id <ID> --file entities.json".to_string(),
            ).into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(&script, "ReflexEntities.json");

    if output::dry_run_guard(
        cli,
        "reflex update-definition",
        &serde_json::json!({
            "workspace": workspace,
            "id": id,
            "contentLength": script.len()
        }),
    ) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/reflexes/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "reflex update-definition", "Contributor"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

// ─── Create Trigger ──────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn create_trigger(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    eventhouse_id: &str,
    database: &str,
    table: &str,
    condition: &str,
    action: &str,
    recipients: &str,
    message: Option<&str>,
    interval: u32,
) -> Result<()> {
    let recipient_list: Vec<&str> = recipients.split(',').map(str::trim).collect();
    if recipient_list.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one recipient is required.".to_string(),
            "Example: --recipients \"user@example.com,team@example.com\"".to_string(),
        )
        .into());
    }

    // Generate UUIDs for all entities
    let container_id = Uuid::new_v4().to_string();
    let source_id = Uuid::new_v4().to_string();
    let event_id = Uuid::new_v4().to_string();
    let object_id = Uuid::new_v4().to_string();
    let attribute_id = Uuid::new_v4().to_string();
    let rule_id = Uuid::new_v4().to_string();

    // Build the alert message
    let alert_msg = message.unwrap_or("Condition triggered by fabio");

    // Build the full ReflexEntities.json with the entity hierarchy:
    // Container → Source → Event → Object → Attribute → Rule (with action)
    let entities = build_trigger_entities(
        &container_id,
        &source_id,
        &event_id,
        &object_id,
        &attribute_id,
        &rule_id,
        eventhouse_id,
        database,
        table,
        condition,
        action,
        &recipient_list,
        alert_msg,
        interval,
    );

    let entities_json = serde_json::to_string(&entities)?;

    if output::dry_run_guard(
        cli,
        "reflex create-trigger",
        &serde_json::json!({
            "workspace": workspace,
            "name": name,
            "eventhouse_id": eventhouse_id,
            "database": database,
            "table": table,
            "condition": condition,
            "action": action,
            "recipients": recipient_list,
            "interval_seconds": interval,
            "entity_count": entities.as_array().map_or(0, Vec::len),
        }),
    ) {
        return Ok(());
    }

    // 1. Create the Reflex item
    let create_body = serde_json::json!({ "displayName": name });
    let item = client
        .post(
            &format!("/workspaces/{workspace}/reflexes"),
            &create_body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "reflex create-trigger", "Member"))?;

    let reflex_id = item
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Failed to extract reflex ID from creation response"))?;

    // 2. Push the definition with the trigger entities
    let encoded = BASE64.encode(entities_json.as_bytes());
    let def_body = serde_json::json!({
        "definition": {
            "parts": [{
                "path": "ReflexEntities.json",
                "payload": encoded,
                "payloadType": "InlineBase64"
            }]
        }
    });

    let update_result = client
        .post(
            &format!("/workspaces/{workspace}/reflexes/{reflex_id}/updateDefinition"),
            &def_body,
            true,
        )
        .await;

    match update_result {
        Ok(_) => {
            let obj = serde_json::json!({
                "id": reflex_id,
                "name": name,
                "status": "trigger_created",
                "action": action,
                "table": table,
                "condition": condition,
                "recipients": recipient_list,
            });
            output::render_object(cli, &obj, "id");
        }
        Err(e) => {
            // Trigger was created but definition update failed — report both
            let obj = serde_json::json!({
                "id": reflex_id,
                "name": name,
                "status": "created_but_definition_failed",
                "error": e.to_string(),
                "hint": "The Reflex item was created but the trigger definition failed to apply. \
                         This is a known limitation — KQL-based triggers may require portal initialization. \
                         Use 'fabio reflex update-definition' to retry or configure via portal."
            });
            output::render_object(cli, &obj, "status");
        }
    }

    Ok(())
}

/// Build the complete `ReflexEntities.json` array for an `AttributeTrigger`-based alert.
///
/// Entity hierarchy: Container -> Event -> Object -> Attribute -> Rule
/// The rule uses an `AttributeTrigger` template with a `NumberBecomes` condition.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn build_trigger_entities(
    container_id: &str,
    _source_id: &str,
    event_id: &str,
    object_id: &str,
    attribute_id: &str,
    rule_id: &str,
    eventhouse_id: &str,
    database: &str,
    table: &str,
    condition: &str,
    action: &str,
    recipients: &[&str],
    message: &str,
    interval: u32,
) -> Value {
    // Build action step arguments based on action type
    let act_step = if action == "teams" {
        let recipient_args: Vec<Value> = recipients
            .iter()
            .map(|r| serde_json::json!({"type": "string", "value": *r}))
            .collect();
        serde_json::json!({
            "name": "ActStep",
            "id": Uuid::new_v4().to_string(),
            "rows": [{
                "name": "TeamsMessage",
                "kind": "TeamsMessage",
                "arguments": [
                    {"name": "messageLocale", "values": [{"type": "string", "value": "en-US"}]},
                    {"name": "recipients", "values": recipient_args},
                    {"name": "headline", "values": [{"type": "string", "value": message}]},
                    {"name": "optionalMessage", "values": [{"type": "string", "value": format!("Condition: {condition} on table {table}")}]}
                ]
            }]
        })
    } else {
        // Default: email
        let recipient_args: Vec<Value> = recipients
            .iter()
            .map(|r| serde_json::json!({"type": "string", "value": *r}))
            .collect();
        serde_json::json!({
            "name": "ActStep",
            "id": Uuid::new_v4().to_string(),
            "rows": [{
                "name": "EmailMessage",
                "kind": "EmailMessage",
                "arguments": [
                    {"name": "messageLocale", "values": [{"type": "string", "value": "en-US"}]},
                    {"name": "sentTo", "values": recipient_args},
                    {"name": "subject", "values": [{"type": "string", "value": format!("Alert: {table}")}]},
                    {"name": "headline", "values": [{"type": "string", "value": message}]},
                    {"name": "optionalMessage", "values": [{"type": "string", "value": format!("Condition: {condition}")}]}
                ]
            }]
        })
    };

    // Build the rule template (AttributeTrigger with NumberBecomes pattern)
    let rule_template = serde_json::json!({
        "templateId": "AttributeTrigger",
        "templateVersion": "1.1",
        "steps": [
            {
                "name": "ScalarSelectStep",
                "id": Uuid::new_v4().to_string(),
                "rows": [{
                    "name": "SelectAttribute",
                    "kind": "SelectAttribute",
                    "arguments": [
                        {"name": "attributeId", "values": [{"type": "string", "value": attribute_id}]}
                    ]
                }]
            },
            {
                "name": "ScalarDetectStep",
                "id": Uuid::new_v4().to_string(),
                "rows": [{
                    "name": "NumberBecomes",
                    "kind": "NumberBecomes",
                    "arguments": [
                        {"name": "operator", "values": [{"type": "string", "value": "BecomesGreaterThan"}]},
                        {"name": "value", "values": [{"type": "string", "value": "0"}]},
                        {"name": "summary", "values": [{"type": "string", "value": "Count"}]},
                        {"name": "timeDrivenWindowSpec", "values": [
                            {"type": "string", "value": (u64::from(interval) * 1000).to_string()}
                        ]}
                    ]
                }]
            },
            act_step
        ]
    });

    let rule_instance = serde_json::to_string(&rule_template).unwrap_or_default();

    // Build entity array
    serde_json::json!([
        // Container
        {
            "uniqueIdentifier": container_id,
            "type": "container-v1",
            "payload": {
                "type": "kqlQueries",
                "displayName": table
            }
        },
        // Event (timeSeriesView: Event type)
        {
            "uniqueIdentifier": event_id,
            "type": "timeSeriesView-v1",
            "payload": {
                "parentContainer": {"targetUniqueIdentifier": container_id},
                "definition": {
                    "type": "Event",
                    "displayName": format!("{table} events"),
                    "instance": "",
                    "settings": {}
                },
                "kqlQueryConfiguration": {
                    "query": format!("{table} | where {condition}"),
                    "databaseName": database,
                    "eventhouseItemId": eventhouse_id,
                    "executionIntervalInSeconds": interval
                }
            }
        },
        // Object (timeSeriesView: Object type)
        {
            "uniqueIdentifier": object_id,
            "type": "timeSeriesView-v1",
            "payload": {
                "parentContainer": {"targetUniqueIdentifier": container_id},
                "parentEvent": {"targetUniqueIdentifier": event_id},
                "definition": {
                    "type": "Object",
                    "displayName": format!("{table} object"),
                    "instance": "",
                    "settings": {}
                }
            }
        },
        // Attribute (timeSeriesView: Attribute type — monitors condition count)
        {
            "uniqueIdentifier": attribute_id,
            "type": "timeSeriesView-v1",
            "payload": {
                "parentContainer": {"targetUniqueIdentifier": container_id},
                "parentObject": {"targetUniqueIdentifier": object_id},
                "definition": {
                    "type": "Attribute",
                    "displayName": format!("count({condition})"),
                    "instance": "",
                    "settings": {}
                }
            }
        },
        // Rule (timeSeriesView: Rule type — AttributeTrigger)
        {
            "uniqueIdentifier": rule_id,
            "type": "timeSeriesView-v1",
            "payload": {
                "parentContainer": {"targetUniqueIdentifier": container_id},
                "parentObject": {"targetUniqueIdentifier": object_id},
                "definition": {
                    "type": "Rule",
                    "displayName": format!("Alert on {condition}"),
                    "instance": rule_instance,
                    "settings": {
                        "shouldRun": true,
                        "shouldApplyRuleOnUpdate": false
                    }
                }
            }
        }
    ])
}
