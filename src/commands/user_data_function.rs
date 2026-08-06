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
pub enum UserDataFunctionCommand {
    /// List user data functions in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a user data function
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// User data function ID
        #[arg(long)]
        id: String,
    },
    /// Create a new user data function
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
    /// Update user data function properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// User data function ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a user data function
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// User data function ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,
    },
    /// Get the definition of a user data function
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// User data function ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a user data function
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// User data function ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },
    /// Invoke a published function via its public REST endpoint
    ///
    /// The function URL is obtained from the Fabric portal (open the item in Run-only
    /// mode → Functions explorer → "Copy Function URL"; public access must be enabled).
    /// There is no public API to discover this URL, so it must be provided via --url.
    #[command(display_order = 8)]
    Invoke {
        /// Public function URL (from the portal; must be an *.fabric.microsoft.com HTTPS URL)
        #[arg(long)]
        url: String,

        /// Function input as name=value (repeatable). Values are sent as JSON strings.
        #[arg(long = "parameter")]
        parameters: Vec<String>,

        /// Raw JSON request body (overrides --parameter). E.g. `{"name":"John","count":3}`
        #[arg(long)]
        body: Option<String>,

        /// Maximum seconds to wait for the function to respond
        #[arg(long, default_value = "230")]
        timeout: u64,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &UserDataFunctionCommand,
) -> Result<()> {
    match command {
        UserDataFunctionCommand::List { workspace } => list(cli, client, workspace).await,
        UserDataFunctionCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        UserDataFunctionCommand::Create {
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
        UserDataFunctionCommand::Update {
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
        UserDataFunctionCommand::Delete {
            workspace,
            id,
            hard_delete,
        } => delete(cli, client, workspace, id, *hard_delete).await,
        UserDataFunctionCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        UserDataFunctionCommand::UpdateDefinition {
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
        UserDataFunctionCommand::Invoke {
            url,
            parameters,
            body,
            timeout,
        } => invoke(cli, client, url, parameters, body.as_deref(), *timeout).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/userDataFunctions"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
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
        .get(&format!("/workspaces/{workspace}/userDataFunctions/{id}"))
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
        "user-data-function create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description , "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/userDataFunctions"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "user-data-function create", "Contributor"))?;
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
            "Example: fabio user-data-function update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "user-data-function update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/userDataFunctions/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "user-data-function update", "Contributor"))?;
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
        "user-data-function delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete }),
    ) {
        return Ok(());
    }
    let url = if hard_delete {
        format!("/workspaces/{workspace}/userDataFunctions/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/userDataFunctions/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "user-data-function delete", "Contributor"))?;
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
            &format!("/workspaces/{workspace}/userDataFunctions/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "user-data-function get-definition", "Contributor"))?;
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
                "Example: fabio user-data-function update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, "definition.json");
    if output::dry_run_guard(
        cli,
        "user-data-function update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/userDataFunctions/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "user-data-function update-definition", "Contributor"))?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// Build the JSON request body for a function invocation. Pure — unit-tested.
///
/// Precedence: `--body` (raw JSON) wins; otherwise `name=value` parameters are
/// assembled into a JSON object (values are sent as strings); otherwise `{}`.
fn build_invoke_body(params: &[String], body: Option<&str>) -> Result<Value> {
    if let Some(raw) = body {
        return serde_json::from_str::<Value>(raw).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("--body is not valid JSON: {e}"),
                "Provide a JSON object, e.g. --body '{\"name\":\"John\"}'".to_string(),
            )
            .into()
        });
    }
    let mut obj = serde_json::Map::new();
    for p in params {
        let (k, v) = p
            .split_once('=')
            .filter(|(k, _)| !k.is_empty())
            .ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid parameter '{p}'"),
                    "Parameters must be name=value, e.g. --parameter name=John".to_string(),
                )
            })?;
        obj.insert(k.to_string(), Value::from(v));
    }
    Ok(Value::Object(obj))
}

/// Invoke a published user data function via its public REST endpoint.
///
/// Fabric exposes no public API to invoke a function or to discover its URL, so
/// the caller supplies the portal-provided `--url`. fabio attaches the Fabric
/// bearer token and POSTs the parameter body, then renders the standard
/// `{functionName, invocationId, status, output, errors}` response.
async fn invoke(
    cli: &Cli,
    client: &FabricClient,
    url: &str,
    parameters: &[String],
    body: Option<&str>,
    timeout: u64,
) -> Result<()> {
    // Validate the URL targets a trusted Microsoft domain before attaching a token.
    crate::client::validate_trusted_url(url, "--url")?;
    let request_body = build_invoke_body(parameters, body)?;

    if output::dry_run_guard(
        cli,
        "user-data-function invoke",
        &serde_json::json!({ "url": url, "body": request_body }),
    ) {
        return Ok(());
    }

    let token = client.require_auth().await?;
    let http = crate::client::http_client_builder()
        .timeout(std::time::Duration::from_secs(timeout))
        .build()
        .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

    let resp = http
        .post(url)
        .header("Authorization", &token)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .send()
        .await
        .map_err(|e| {
            FabioError::with_hint(
                ErrorCode::NetworkError,
                format!("Failed to reach the function endpoint: {e}"),
                "Verify the function URL from the portal (Run-only mode → Copy Function URL) and that public access is enabled.".to_string(),
            )
        })?;

    let status = resp.status();
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(FabioError::with_hint(
            crate::errors::FabioError::from_status(status.as_u16(), text.clone()).code,
            format!("Function invocation failed (HTTP {}): {text}", status.as_u16()),
            "422 = a UserThrownError in the function; 400 = bad/missing parameters or public access disabled; 401/403 = auth/permission.".to_string(),
        )
        .into());
    }

    // Successful HTTP: render the function's structured response. The `status`
    // field (Succeeded/Failed/BadRequest/Timeout/ResponseTooLarge) reports the
    // function-level outcome; agents inspect it.
    let parsed: Value =
        serde_json::from_str(&text).unwrap_or_else(|_| serde_json::json!({ "output": text }));
    output::render_object(cli, &parsed, "output");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_invoke_body_from_parameters() {
        let params = vec!["name=John".to_string(), "city=Paris".to_string()];
        let body = build_invoke_body(&params, None).unwrap();
        assert_eq!(body, serde_json::json!({ "name": "John", "city": "Paris" }));
    }

    #[test]
    fn build_invoke_body_value_may_contain_equals() {
        let params = vec!["expr=a=b".to_string()];
        let body = build_invoke_body(&params, None).unwrap();
        assert_eq!(body["expr"], "a=b");
    }

    #[test]
    fn build_invoke_body_empty_is_object() {
        let body = build_invoke_body(&[], None).unwrap();
        assert_eq!(body, serde_json::json!({}));
    }

    #[test]
    fn build_invoke_body_raw_json_overrides() {
        let params = vec!["name=Ignored".to_string()];
        let body = build_invoke_body(&params, Some(r#"{"name":"John","count":3}"#)).unwrap();
        assert_eq!(body, serde_json::json!({ "name": "John", "count": 3 }));
    }

    #[test]
    fn build_invoke_body_rejects_bad_json() {
        assert!(build_invoke_body(&[], Some("not json")).is_err());
    }

    #[test]
    fn build_invoke_body_rejects_bad_parameter() {
        assert!(build_invoke_body(&["noequals".to_string()], None).is_err());
        assert!(build_invoke_body(&["=value".to_string()], None).is_err());
    }
}
