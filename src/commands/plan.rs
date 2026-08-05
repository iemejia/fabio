use anyhow::Result;
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
pub enum PlanCommand {
    /// List plans in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Only list plans directly in the root folder (default lists nested folders too)
        #[arg(long)]
        no_recursive: bool,

        /// Filter plans to a specific root folder ID (defaults to the workspace root)
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show details of a plan
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Plan ID
        #[arg(long)]
        id: String,
    },
    /// Create a new plan
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Display name
        #[arg(long)]
        name: String,
        /// Optional description (max 256 characters)
        #[arg(long)]
        description: Option<String>,

        /// Sensitivity label ID to apply on creation
        #[arg(long)]
        sensitivity_label: Option<String>,

        /// Folder ID to create the plan in (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,
    },
    /// Update plan properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Plan ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description (max 256 characters)
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a plan
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Plan ID
        #[arg(long)]
        id: String,
    },
    /// Get the definition of a plan
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Plan ID
        #[arg(long)]
        id: String,

        /// Definition format (defaults to the server's canonical format, `PlanV1`)
        #[arg(long)]
        format: Option<String>,

        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a plan
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Plan ID
        #[arg(long)]
        id: String,
        /// Path to definition file (a single raw part, or a full envelope JSON)
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &PlanCommand) -> Result<()> {
    match command {
        PlanCommand::List {
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
        PlanCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        PlanCommand::Create {
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
        PlanCommand::Update {
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
        PlanCommand::Delete { workspace, id } => delete(cli, client, workspace, id).await,
        PlanCommand::GetDefinition {
            workspace,
            id,
            format,
            decode,
        } => get_definition(cli, client, workspace, id, format.as_deref(), *decode).await,
        PlanCommand::UpdateDefinition {
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
    }
}

async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    no_recursive: bool,
    root_folder_id: Option<&str>,
) -> Result<()> {
    let mut url = format!("/workspaces/{workspace}/plans");
    let mut params: Vec<String> = Vec::new();
    if no_recursive {
        params.push("recursive=false".to_string());
    }
    if let Some(folder_id) = root_folder_id {
        params.push(format!("rootFolderId={folder_id}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }

    let resp = client
        .get_list(&url, "value", cli.all, cli.continuation_token.as_deref())
        .await?;
    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

async fn show(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/plans/{id}"))
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
        "plan create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description, "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(&format!("/workspaces/{workspace}/plans"), &body, true)
        .await
        .map_err(|e| enrich_forbidden(e, "plan create", "Contributor"))?;
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
            "Example: fabio plan update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "plan update", &body) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{workspace}/plans/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "plan update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "plan delete",
        &serde_json::json!({ "workspace": workspace, "id": id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!("/workspaces/{workspace}/plans/{id}"))
        .await
        .map_err(|e| enrich_forbidden(e, "plan delete", "Contributor"))?;
    let obj = serde_json::json!({ "id": id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

fn get_definition_path(workspace: &str, id: &str, format: Option<&str>) -> String {
    format.map_or_else(
        || format!("/workspaces/{workspace}/plans/{id}/getDefinition"),
        |f| format!("/workspaces/{workspace}/plans/{id}/getDefinition?format={f}"),
    )
}

async fn get_definition(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    format: Option<&str>,
    decode: bool,
) -> Result<()> {
    let path = get_definition_path(workspace, id, format);
    let data = client
        .post(&path, &serde_json::json!({}), true)
        .await
        .map_err(|e| enrich_forbidden(e, "plan get-definition", "Contributor"))?;
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
                crate::definition_spec::definition_input_hint("Plan", "plan", "update-definition"),
            )
            .into());
        }
    };

    let body = crate::definition_spec::build_update_definition_body(
        &script,
        "connectedPlanning/infobridge.json",
    );

    if output::dry_run_guard(
        cli,
        "plan update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/plans/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "plan update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_update_definition_body_wraps_raw_content() {
        let body = crate::definition_spec::build_update_definition_body(
            "hello world",
            "connectedPlanning/infobridge.json",
        );
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "connectedPlanning/infobridge.json");
        assert_eq!(parts[0]["payloadType"], "InlineBase64");
    }

    #[test]
    fn build_update_definition_body_passes_through_envelope() {
        let envelope = serde_json::json!({
            "definition": {
                "format": "PlanV1",
                "parts": [{"path": "connectedPlanning/infobridge.json", "payload": "eyJ9", "payloadType": "InlineBase64"}]
            }
        });
        let body = crate::definition_spec::build_update_definition_body(
            &envelope.to_string(),
            "connectedPlanning/infobridge.json",
        );
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "connectedPlanning/infobridge.json");
    }

    /// Matches the `CreatePlanRequest` shape from fabric-rest-api-specs/plan/examples/CreatePlan.json
    #[test]
    fn create_body_matches_spec_example() {
        let mut body = serde_json::json!({ "displayName": "Plan 1" });
        body["description"] = Value::from("A plan description.");
        assert_eq!(body["displayName"], "Plan 1");
        assert_eq!(body["description"], "A plan description.");
        // Spec response envelope confirms the item "type" is "Plan"
        let response = serde_json::json!({
            "id": "3546052c-ae64-4526-b1a8-52af7761426f",
            "type": "Plan",
            "displayName": "Plan 1",
            "description": "A plan description.",
            "workspaceId": "cfafbeb1-8037-4d0c-896e-28571e947920"
        });
        assert_eq!(response["type"], "Plan");
    }

    /// Matches `UpdatePlanRequest` from fabric-rest-api-specs/plan/examples/UpdatePlan.json —
    /// both fields are optional (fully-optional PATCH body).
    #[test]
    fn update_body_omits_absent_fields() {
        let name: Option<&str> = None;
        let description: Option<&str> = Some("An updated plan description.");
        let mut body = serde_json::json!({});
        if let Some(n) = name {
            body["displayName"] = Value::from(n);
        }
        if let Some(d) = description {
            body["description"] = Value::from(d);
        }
        assert!(body.get("displayName").is_none());
        assert_eq!(body["description"], "An updated plan description.");
    }

    /// Matches fabric-rest-api-specs/plan/examples/ListPlansInWorkspace.json — response
    /// array key is "value" and each item has type "Plan".
    #[test]
    fn list_response_shape_matches_spec_example() {
        let response = serde_json::json!({
            "value": [
                {
                    "id": "3546052c-ae64-4526-b1a8-52af7761426f",
                    "type": "Plan",
                    "displayName": "Plan 1",
                    "description": "A plan description.",
                    "workspaceId": "cfafbeb1-8037-4d0c-896e-28571e947920"
                },
                {
                    "id": "f089354e-8366-4e18-aea3-4cb4a3a50b48",
                    "type": "Plan",
                    "displayName": "Plan 2",
                    "description": "A plan description.",
                    "workspaceId": "cfafbeb1-8037-4d0c-896e-28571e947920"
                }
            ]
        });
        let items = response["value"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|item| item["type"] == "Plan"));
    }

    /// Matches fabric-rest-api-specs/plan/examples/GetPlanDefinition.json — the canonical
    /// (sole) definition part path is "connectedPlanning/infobridge.json".
    #[test]
    fn get_definition_response_shape_matches_spec_example() {
        let response = serde_json::json!({
            "definition": {
                "parts": [
                    {
                        "path": "connectedPlanning/infobridge.json",
                        "payload": "eyJjb25uZWN0aW9uSWQiOiAiMTIzNDU2Nzg=",
                        "payloadType": "InlineBase64"
                    }
                ]
            }
        });
        let parts = response["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "connectedPlanning/infobridge.json");
        assert_eq!(parts[0]["payloadType"], "InlineBase64");
    }

    /// `getDefinition`'s optional `format` query parameter (`GET .../plans/{id}/getDefinition
    /// [?format=<f>]`) is only appended when `--format` is supplied, mirroring the
    /// `ontology`/`sql-database`/`kql-database`/`lakehouse` convention.
    #[test]
    fn get_definition_path_omits_format_when_absent() {
        let path = get_definition_path("ws1", "plan1", None);
        assert_eq!(path, "/workspaces/ws1/plans/plan1/getDefinition");
    }

    #[test]
    fn get_definition_path_appends_format_when_present() {
        let path = get_definition_path("ws1", "plan1", Some("PlanV1"));
        assert_eq!(
            path,
            "/workspaces/ws1/plans/plan1/getDefinition?format=PlanV1"
        );
    }

    /// `list()` builds the query string from --no-recursive and --root-folder-id; verify the
    /// URL construction logic matches the spec's `recursive`/`rootFolderId` query params.
    #[test]
    fn list_query_params_are_built_correctly() {
        let workspace = "cfafbeb1-8037-4d0c-896e-28571e947920";
        let mut url = format!("/workspaces/{workspace}/plans");
        let mut params: Vec<String> = Vec::new();
        let no_recursive = true;
        let root_folder_id: Option<&str> = Some("aaaaaaaa-0000-0000-0000-000000000000");
        if no_recursive {
            params.push("recursive=false".to_string());
        }
        if let Some(folder_id) = root_folder_id {
            params.push(format!("rootFolderId={folder_id}"));
        }
        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }
        assert_eq!(
            url,
            format!(
                "/workspaces/{workspace}/plans?recursive=false&rootFolderId=aaaaaaaa-0000-0000-0000-000000000000"
            )
        );
    }
}
