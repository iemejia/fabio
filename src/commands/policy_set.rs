use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PolicyScope {
    #[value(name = "Tenant")]
    Tenant,
    #[value(name = "Capacity")]
    Capacity,
}

impl PolicyScope {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Tenant => "Tenant",
            Self::Capacity => "Capacity",
        }
    }
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PolicyType {
    #[value(name = "ItemCreation")]
    ItemCreation,
    #[value(name = "WorkspaceSettingsEditing")]
    WorkspaceSettingsEditing,
}

impl PolicyType {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ItemCreation => "ItemCreation",
            Self::WorkspaceSettingsEditing => "WorkspaceSettingsEditing",
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum PolicySetCommand {
    /// List policy sets in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list policy sets directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a policy set
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
    },
    /// Create a policy set
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// Description (maximum 256 characters)
        #[arg(long)]
        description: Option<String>,
        /// Folder ID (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,
        /// Policy scope for a creation-payload request
        #[arg(long, conflicts_with_all = ["definition", "definition_file"])]
        scope: Option<PolicyScope>,
        /// Full policy-set definition envelope as inline JSON
        #[arg(long, conflicts_with_all = ["scope", "definition_file"])]
        definition: Option<String>,
        /// Path to a full policy-set definition envelope
        #[arg(long, conflicts_with_all = ["scope", "definition"])]
        definition_file: Option<String>,
    },
    /// Update policy-set properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a policy set
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
    },
    /// Get a policy-set definition
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Definition format (currently Beta)
        #[arg(long, value_parser = ["Beta"])]
        format: Option<String>,
        /// Decode base64 definition parts
        #[arg(long)]
        decode: bool,
    },
    /// Replace a policy-set definition
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Path to a raw policySet.json part or full definition envelope
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline raw policySet.json content or full definition envelope
        #[arg(long)]
        content: Option<String>,
        /// Update item metadata from a supplied .platform definition part
        #[arg(long)]
        update_metadata: bool,
    },
    /// List policy rules (beta)
    ListRules {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
    },
    /// Show a policy rule (beta)
    ShowRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Policy rule ID
        #[arg(long)]
        rule_id: String,
    },
    /// Create a policy rule (beta)
    CreateRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Rule display name
        #[arg(long)]
        name: String,
        /// Rule description
        #[arg(long)]
        description: Option<String>,
        /// Policy governed by the rule
        #[arg(long)]
        policy: PolicyType,
        /// Conditions JSON array
        #[arg(long)]
        conditions: String,
        /// Effects JSON array
        #[arg(long)]
        effects: String,
    },
    /// Update a policy rule (beta)
    UpdateRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Policy rule ID
        #[arg(long)]
        rule_id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// Replacement conditions JSON array
        #[arg(long)]
        conditions: Option<String>,
    },
    /// Delete a policy rule (beta)
    DeleteRule {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Policy rule ID
        #[arg(long)]
        rule_id: String,
    },
    /// Replace every rule for one policy (beta)
    ReplaceRulesByPolicy {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Policy whose rules will be replaced
        #[arg(long)]
        policy: PolicyType,
        /// Complete replacement policyRules JSON array
        #[arg(long)]
        policy_rules: String,
    },
    /// Activate a capacity-scoped policy set
    Activate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
        /// Capacity ID on which to activate the policy set
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,
        /// Replace the currently active policy set on this capacity
        #[arg(long)]
        allow_replace: bool,
    },
    /// Deactivate a capacity-scoped policy set
    Deactivate {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Policy set ID
        #[arg(long)]
        id: String,
    },
    /// Show the active policy set on a capacity
    GetActiveForCapacity {
        /// Capacity ID
        #[arg(long, env = "FABIO_CAPACITY")]
        capacity_id: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &PolicySetCommand) -> Result<()> {
    match command {
        PolicySetCommand::List {
            workspace,
            no_recursive,
            root_folder_id,
        } => {
            list(
                cli,
                client,
                workspace,
                *no_recursive,
                root_folder_id.as_deref(),
            )
            .await
        }
        PolicySetCommand::Show { workspace, id } => {
            crate::commands::crud::show(cli, client, "policySets", workspace, id).await
        }
        PolicySetCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            scope,
            definition,
            definition_file,
        } => {
            create(
                cli,
                client,
                workspace,
                name,
                description.as_deref(),
                folder_id.as_deref(),
                *scope,
                definition.as_deref(),
                definition_file.as_deref(),
            )
            .await
        }
        PolicySetCommand::Update {
            workspace,
            id,
            name,
            description,
        } => {
            validate_description(description.as_deref())?;
            crate::commands::crud::update(
                cli,
                client,
                "policy-set",
                "policySets",
                "Contributor",
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
            )
            .await
        }
        PolicySetCommand::Delete { workspace, id } => {
            crate::commands::crud::delete(
                cli,
                client,
                "policy-set",
                "policySets",
                "Contributor",
                workspace,
                id,
                false,
            )
            .await
        }
        PolicySetCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        PolicySetCommand::UpdateDefinition {
            workspace,
            id,
            file,
            content,
            update_metadata,
        } => {
            update_definition(
                cli,
                client,
                workspace,
                id,
                file.as_deref(),
                content.as_deref(),
                *update_metadata,
            )
            .await
        }
        PolicySetCommand::ListRules { workspace, id } => {
            list_rules(cli, client, workspace, id).await
        }
        PolicySetCommand::ShowRule {
            workspace,
            id,
            rule_id,
        } => show_rule(cli, client, workspace, id, rule_id).await,
        PolicySetCommand::CreateRule {
            workspace,
            id,
            name,
            description,
            policy,
            conditions,
            effects,
        } => {
            let mut body = json!({
                "displayName": name,
                "policy": policy.as_str(),
                "conditions": parse_array(conditions, "--conditions")?,
                "effects": parse_array(effects, "--effects")?,
            });
            if let Some(description) = description {
                body["description"] = Value::from(description.as_str());
            }
            mutate_rule(cli, client, "create-rule", workspace, id, None, body, true).await
        }
        PolicySetCommand::UpdateRule {
            workspace,
            id,
            rule_id,
            name,
            description,
            conditions,
        } => {
            let mut body = json!({});
            if let Some(name) = name {
                body["displayName"] = Value::from(name.as_str());
            }
            if let Some(description) = description {
                body["description"] = Value::from(description.as_str());
            }
            if let Some(conditions) = conditions {
                body["conditions"] = parse_array(conditions, "--conditions")?;
            }
            if body.as_object().is_none_or(serde_json::Map::is_empty) {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "At least one of --name, --description, or --conditions is required",
                    "Example: fabio policy-set update-rule --workspace <WS> --id <SET> --rule-id <RULE> --name \"Updated rule\"",
                )
                .into());
            }
            mutate_rule(
                cli,
                client,
                "update-rule",
                workspace,
                id,
                Some(rule_id),
                body,
                false,
            )
            .await
        }
        PolicySetCommand::DeleteRule {
            workspace,
            id,
            rule_id,
        } => delete_rule(cli, client, workspace, id, rule_id).await,
        PolicySetCommand::ReplaceRulesByPolicy {
            workspace,
            id,
            policy,
            policy_rules,
        } => {
            let policy_rules = parse_array(policy_rules, "--policy-rules")?;
            validate_replacement_rules(cli.force, &policy_rules)?;
            replace_rules(cli, client, workspace, id, *policy, policy_rules).await
        }
        PolicySetCommand::Activate {
            workspace,
            id,
            capacity_id,
            allow_replace,
        } => activate(cli, client, workspace, id, capacity_id, *allow_replace).await,
        PolicySetCommand::Deactivate { workspace, id } => {
            deactivate(cli, client, workspace, id).await
        }
        PolicySetCommand::GetActiveForCapacity { capacity_id } => {
            get_active_for_capacity(cli, client, capacity_id).await
        }
    }
}

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    let mut url = format!("/workspaces/{workspace}/policySets");
    let mut params = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder_id) = root_folder_id {
        validate_uuid(folder_id, "--root-folder-id")?;
        params.push(format!("rootFolderId={folder_id}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let response = client
        .get_list(&url, "value", cli.all, cli.continuation_token.as_deref())
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set list", "Viewer"))?;
    output::render_item_list(
        cli,
        &response.items,
        &[
            "displayName",
            "id",
            "properties.status",
            "properties.scope.type",
        ],
        &["NAME", "ID", "STATUS", "SCOPE"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    folder_id: Option<&str>,
    scope: Option<PolicyScope>,
    definition: Option<&str>,
    definition_file: Option<&str>,
) -> Result<()> {
    validate_description(description)?;
    let mut body = json!({"displayName": name});
    if let Some(description) = description {
        body["description"] = Value::from(description);
    }
    if let Some(folder_id) = folder_id {
        validate_uuid(folder_id, "--folder-id")?;
        body["folderId"] = Value::from(folder_id);
    }
    match (scope, definition, definition_file) {
        (Some(scope), None, None) => {
            body["creationPayload"] = json!({"scope": {"type": scope.as_str()}});
        }
        (None, Some(value), None) => body["definition"] = parse_definition(value)?,
        (None, None, Some(path)) => {
            let value = std::fs::read_to_string(path).map_err(|error| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Failed to read definition file '{path}': {error}"),
                    "Verify the file exists and contains a policy-set definition envelope.",
                )
            })?;
            body["definition"] = parse_definition(&value)?;
        }
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Provide exactly one of --scope, --definition, or --definition-file",
                "Use --scope Tenant for a tenant policy set, --scope Capacity for a capacity policy set, or provide a Beta definition envelope.",
            )
            .into());
        }
    }
    if output::dry_run_guard(cli, "policy-set create", &body) {
        return Ok(());
    }
    let data = client
        .post(&format!("/workspaces/{workspace}/policySets"), &body, true)
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

fn validate_description(description: Option<&str>) -> Result<()> {
    if description.is_some_and(|value| value.chars().count() > 256) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--description must be at most 256 characters",
            "Shorten the description to 256 characters or fewer.",
        )
        .into());
    }
    Ok(())
}

fn parse_definition(input: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(input).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid definition JSON: {error}"),
            "Provide an object containing parts and format \"Beta\", or a full {\"definition\": ...} envelope.",
        )
    })?;
    let definition = value.get("definition").unwrap_or(&value).clone();
    if !definition.get("parts").is_some_and(Value::is_array)
        || definition.get("format").and_then(Value::as_str) != Some("Beta")
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Policy-set definitions require a parts array and format \"Beta\"",
            "Example: {\"parts\":[{\"path\":\"policySet.json\",\"payload\":\"<base64>\",\"payloadType\":\"InlineBase64\"}],\"format\":\"Beta\"}",
        )
        .into());
    }
    Ok(definition)
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let suffix = format.map_or_else(String::new, |value| format!("?format={value}"));
    let data = client
        .post(
            &format!("/workspaces/{workspace}/policySets/{id}/getDefinition{suffix}"),
            &json!({}),
            true,
        )
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set get-definition", "Contributor"))?;
    let value = if decode {
        output::decode_definition_parts(data)
    } else {
        data
    };
    output::render_object(cli, &value, "definition");
    Ok(())
}

async fn update_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    update_metadata: bool,
) -> Result<()> {
    let raw = match (file, content) {
        (Some(path), None) => std::fs::read_to_string(path).map_err(|error| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read definition file '{path}': {error}"),
                "Verify the file exists and is readable.",
            )
        })?,
        (None, Some(content)) => content.to_string(),
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Provide exactly one of --file or --content",
                "Example: fabio policy-set update-definition --workspace <WS> --id <ID> --file policySet.json",
            )
            .into());
        }
    };
    let mut body = crate::definition_spec::build_update_definition_body(&raw, "policySet.json");
    body["definition"]["format"] = Value::from("Beta");
    if output::dry_run_guard(
        cli,
        "policy-set update-definition",
        &json!({"workspace": workspace, "id": id, "updateMetadata": update_metadata, "request": body}),
    ) {
        return Ok(());
    }
    let suffix = if update_metadata {
        "?updateMetadata=true"
    } else {
        ""
    };
    let data = client
        .post(
            &format!("/workspaces/{workspace}/policySets/{id}/updateDefinition{suffix}"),
            &body,
            true,
        )
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set update-definition", "Contributor"))?;
    let result = if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        json!({"id": id, "status": "definition_updated"})
    } else {
        data
    };
    output::render_object(cli, &result, "status");
    Ok(())
}

fn parse_array(input: &str, flag: &str) -> Result<Value> {
    let value: Value = serde_json::from_str(input).map_err(|error| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON for {flag}: {error}"),
            format!("Provide {flag} as a JSON array."),
        )
    })?;
    if !value.is_array() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a JSON array"),
            format!("Example: {flag} '[{{\"type\":\"Allow\"}}]'"),
        )
        .into());
    }
    Ok(value)
}

fn validate_replacement_rules(force: bool, policy_rules: &Value) -> Result<()> {
    let rules = policy_rules
        .as_array()
        .expect("parse_array guarantees an array");
    if rules.len() > 50 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "--policy-rules contains {} rules; the API maximum is 50 per policy",
                rules.len()
            ),
            "Split rules across policy types or reduce the replacement set to 50 rules.",
        )
        .into());
    }
    if rules.is_empty() && !force {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "An empty --policy-rules array would delete every rule for the selected policy",
            "Re-run with --force only after confirming that all rules for this policy should be removed.",
        )
        .into());
    }
    Ok(())
}

fn rules_url(workspace: &str, id: &str) -> String {
    format!("/workspaces/{workspace}/policySets/{id}/policyRules?beta=true")
}

async fn list_rules(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let response = client
        .get_list(
            &rules_url(workspace, id),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set list-rules", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["displayName", "id", "policy", "description"],
        &["NAME", "ID", "POLICY", "DESCRIPTION"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show_rule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/policySets/{id}/policyRules/{rule_id}?beta=true"
        ))
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set show-rule", "Viewer"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn mutate_rule(
    cli: &Cli,
    client: &FabricClient,
    operation: &str,
    workspace: &str,
    id: &str,
    rule_id: Option<&str>,
    body: Value,
    poll: bool,
) -> Result<()> {
    let op = format!("policy-set {operation}");
    if output::dry_run_guard(cli, &op, &body) {
        return Ok(());
    }
    let data = if let Some(rule_id) = rule_id {
        client
            .patch(
                &format!("/workspaces/{workspace}/policySets/{id}/policyRules/{rule_id}?beta=true"),
                &body,
            )
            .await
    } else {
        client.post(&rules_url(workspace, id), &body, poll).await
    }
    .map_err(|error| enrich_forbidden(error, &op, "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_rule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    rule_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "policy-set delete-rule",
        &json!({"workspace": workspace, "policySetId": id, "policyRuleId": rule_id}),
    ) {
        return Ok(());
    }
    client
        .delete(&format!(
            "/workspaces/{workspace}/policySets/{id}/policyRules/{rule_id}?beta=true"
        ))
        .await
        .map_err(|error| enrich_forbidden(error, "policy-set delete-rule", "Contributor"))?;
    output::render_object(cli, &json!({"id": rule_id, "status": "deleted"}), "status");
    Ok(())
}

async fn replace_rules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    policy: PolicyType,
    policy_rules: Value,
) -> Result<()> {
    let body = json!({"policy": policy.as_str(), "policyRules": policy_rules});
    if output::dry_run_guard(cli, "policy-set replace-rules-by-policy", &body) {
        return Ok(());
    }
    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/policySets/{id}/policyRules/replaceByPolicy?beta=true"
            ),
            &body,
            true,
        )
        .await
        .map_err(|error| {
            enrich_forbidden(error, "policy-set replace-rules-by-policy", "Contributor")
        })?;
    output::render_object(cli, &data, "value");
    Ok(())
}

async fn activate(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    capacity_id: &str,
    allow_replace: bool,
) -> Result<()> {
    validate_uuid(capacity_id, "--capacity-id")?;
    let body = json!({"scopeType": "Capacity", "capacityId": capacity_id});
    if output::dry_run_guard_maybe_destructive(
        cli,
        "policy-set activate",
        &json!({"workspace": workspace, "id": id, "allowReplace": allow_replace, "request": body}),
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
            &format!("/workspaces/{workspace}/policySets/{id}/activate{suffix}"),
            &body,
            false,
        )
        .await
        .map_err(|error| {
            enrich_forbidden(error, "policy-set activate", "Capacity administrator")
        })?;
    output::render_object(cli, &json!({"id": id, "status": "active"}), "status");
    Ok(())
}

async fn deactivate(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let body = json!({"scopeType": "Capacity"});
    if output::dry_run_guard(
        cli,
        "policy-set deactivate",
        &json!({"workspace": workspace, "id": id, "request": body}),
    ) {
        return Ok(());
    }
    client
        .post(
            &format!("/workspaces/{workspace}/policySets/{id}/deactivate"),
            &body,
            false,
        )
        .await
        .map_err(|error| {
            enrich_forbidden(error, "policy-set deactivate", "Capacity administrator")
        })?;
    output::render_object(cli, &json!({"id": id, "status": "inactive"}), "status");
    Ok(())
}

async fn get_active_for_capacity(
    cli: &Cli,
    client: &FabricClient,
    capacity_id: &str,
) -> Result<()> {
    validate_uuid(capacity_id, "--capacity-id")?;
    let data = client
        .get(&format!("/capacities/{capacity_id}/policySets/active"))
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "policy-set get-active-for-capacity",
                "Capacity administrator",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy_set_definition_requires_beta_format() {
        let value = parse_definition(
            r#"{"parts":[{"path":"policySet.json","payload":"e30=","payloadType":"InlineBase64"}],"format":"Beta"}"#,
        )
        .unwrap();
        assert_eq!(value["format"], "Beta");
        assert!(parse_definition(r#"{"parts":[],"format":"V1"}"#).is_err());
    }

    #[test]
    fn rule_arrays_must_be_json_arrays() {
        assert!(parse_array(r#"[{"type":"Allow"}]"#, "--effects").is_ok());
        assert!(parse_array(r#"{"type":"Allow"}"#, "--effects").is_err());
    }

    #[test]
    fn policy_rule_url_always_enables_beta() {
        assert_eq!(
            rules_url("workspace", "set"),
            "/workspaces/workspace/policySets/set/policyRules?beta=true"
        );
    }

    #[test]
    fn policy_set_description_is_limited_to_256_characters() {
        assert!(validate_description(Some(&"x".repeat(256))).is_ok());
        assert!(validate_description(Some(&"x".repeat(257))).is_err());
    }

    #[test]
    fn replacement_rule_guard_limits_blast_radius() {
        assert!(validate_replacement_rules(false, &json!([])).is_err());
        assert!(validate_replacement_rules(true, &json!([])).is_ok());
        assert!(validate_replacement_rules(false, &Value::Array(vec![json!({}); 51])).is_err());
    }
}
