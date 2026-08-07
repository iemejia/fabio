use anyhow::{Result, bail};
use clap::Subcommand;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples connection\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum ConnectionCommand {
    /// List all connections you have permission to access
    #[command(display_order = 1)]
    List,
    /// Show details of a specific connection
    #[command(display_order = 2)]
    Show {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// Create a new connection
    #[command(display_order = 3)]
    Create {
        /// Display name for the connection
        #[arg(long)]
        name: String,

        /// Connectivity type
        #[arg(long, value_name = "TYPE", value_parser = ["ShareableCloud", "OnPremises", "VirtualNetworkGateway", "StreamingVirtualNetworkGateway", "PersonalCloud"])]
        connectivity_type: String,

        /// Connection type path (e.g., Web, SQL, `GitHubSourceControl`)
        #[arg(long, visible_alias = "type", value_name = "TYPE")]
        connection_type: String,

        /// Creation method (`connectionDetails.creationMethod`). If omitted, fabio auto-resolves it from the connection type via `supportedConnectionTypes` (most types differ, e.g. `SQL`→`Sql`, `EventHub`→`EventHub.Contents`). Specify explicitly only for types with multiple methods (e.g. `AzureDataExplorer`, `Spark`).
        #[arg(long, value_name = "METHOD")]
        creation_method: Option<String>,

        /// Connection parameters as JSON (e.g., '{"server":"host","database":"db"}')
        #[arg(long)]
        parameters: String,

        /// Gateway ID through which the connection is made (required when `--connectivity-type` is `VirtualNetworkGateway` or `StreamingVirtualNetworkGateway`)
        #[arg(long, value_name = "GATEWAY_ID")]
        gateway_id: Option<String>,

        /// Credential type
        #[arg(long, value_parser = ["Basic", "OAuth2", "Key", "Anonymous", "ServicePrincipal", "SharedAccessSignature", "WorkspaceIdentity", "KeyPair"])]
        credential_type: String,

        /// Credentials as JSON (format depends on credential type)
        #[arg(long)]
        credentials: Option<String>,

        /// Privacy level
        #[arg(long, default_value = "Organizational", value_parser = ["None", "Public", "Organizational", "Private"])]
        privacy_level: String,

        /// Skip connection test during creation
        #[arg(long)]
        skip_test_connection: bool,

        /// Allow this connection to be used by code-first artifacts such as Notebooks (`allowUsageInUserControlledCode`). Can ONLY be set at creation time; cannot be changed later.
        #[arg(long, visible_alias = "allow-usage-in-user-controlled-code")]
        allow_code_first_artifacts: bool,

        /// Allow this connection to be used with on-premises or virtual-network data gateways (`allowConnectionUsageInGateway`).
        #[arg(long)]
        allow_gateway_usage: bool,
    },
    /// Update a connection's name, credentials, or privacy level
    #[command(display_order = 4)]
    Update {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// New display name
        #[arg(long)]
        name: Option<String>,

        /// New privacy level
        #[arg(long, value_parser = ["None", "Public", "Organizational", "Private"])]
        privacy_level: Option<String>,

        /// New credential type
        #[arg(long, value_parser = ["Basic", "OAuth2", "Key", "Anonymous", "ServicePrincipal", "SharedAccessSignature", "WorkspaceIdentity", "KeyPair"])]
        credential_type: Option<String>,

        /// New credentials as JSON
        #[arg(long)]
        credentials: Option<String>,
    },
    /// Delete a connection
    #[command(display_order = 5)]
    Delete {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// List supported connection types (gateway types catalog)
    #[command(display_order = 10)]
    ListSupportedTypes,
    /// List role assignments for a connection
    #[command(display_order = 20)]
    ListRoleAssignments {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
    /// Add a role assignment to a connection
    #[command(display_order = 21)]
    AddRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Principal ID (user, group, or service principal)
        #[arg(long)]
        principal_id: String,

        /// Principal type
        #[arg(long, value_parser = ["User", "Group", "ServicePrincipal"])]
        principal_type: String,

        /// Role to assign
        #[arg(long, value_parser = ["Owner", "User", "UserWithReshare"])]
        role: String,
    },
    /// Show a specific role assignment for a connection
    #[command(display_order = 22)]
    ShowRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// Update a role assignment for a connection
    #[command(display_order = 23)]
    UpdateRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,

        /// New role
        #[arg(long, value_parser = ["Owner", "User", "UserWithReshare"])]
        role: String,
    },
    /// Delete a role assignment from a connection
    #[command(display_order = 24)]
    DeleteRoleAssignment {
        /// Connection ID
        #[arg(long)]
        id: String,

        /// Role assignment ID
        #[arg(long)]
        assignment_id: String,
    },
    /// Test a connection (not supported for `StreamingVirtualNetworkGateway` connections)
    #[command(display_order = 30)]
    TestConnection {
        /// Connection ID
        #[arg(long)]
        id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &ConnectionCommand) -> Result<()> {
    match command {
        ConnectionCommand::List => list(cli, client).await,
        ConnectionCommand::Show { id } => show(cli, client, id).await,
        ConnectionCommand::Create {
            name,
            connectivity_type,
            connection_type,
            creation_method,
            parameters,
            gateway_id,
            credential_type,
            credentials,
            privacy_level,
            skip_test_connection,
            allow_code_first_artifacts,
            allow_gateway_usage,
        } => {
            create(
                cli,
                client,
                name,
                connectivity_type,
                connection_type,
                creation_method.as_deref(),
                parameters,
                gateway_id.as_deref(),
                credential_type,
                credentials.as_deref(),
                privacy_level,
                *skip_test_connection,
                *allow_code_first_artifacts,
                *allow_gateway_usage,
            )
            .await
        }
        ConnectionCommand::Update {
            id,
            name,
            privacy_level,
            credential_type,
            credentials,
        } => {
            update(
                cli,
                client,
                id,
                name.as_deref(),
                privacy_level.as_deref(),
                credential_type.as_deref(),
                credentials.as_deref(),
            )
            .await
        }
        ConnectionCommand::Delete { id } => delete(cli, client, id).await,
        ConnectionCommand::ListSupportedTypes => list_supported_types(cli, client).await,
        ConnectionCommand::ListRoleAssignments { id } => {
            list_role_assignments(cli, client, id).await
        }
        ConnectionCommand::AddRoleAssignment {
            id,
            principal_id,
            principal_type,
            role,
        } => add_role_assignment(cli, client, id, principal_id, principal_type, role).await,
        ConnectionCommand::ShowRoleAssignment { id, assignment_id } => {
            show_role_assignment(cli, client, id, assignment_id).await
        }
        ConnectionCommand::UpdateRoleAssignment {
            id,
            assignment_id,
            role,
        } => update_role_assignment(cli, client, id, assignment_id, role).await,
        ConnectionCommand::DeleteRoleAssignment { id, assignment_id } => {
            delete_role_assignment(cli, client, id, assignment_id).await
        }
        ConnectionCommand::TestConnection { id } => test_connection(cli, client, id).await,
    }
}

async fn list(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/connections",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let (columns, headers) = list_table_columns(&resp.items);
    output::render_list_with_token(
        cli,
        &resp.items,
        columns,
        headers,
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

fn list_table_columns(items: &[Value]) -> (&'static [&'static str], &'static [&'static str]) {
    let has_gateway_id = items.iter().any(|item| {
        item.get("gatewayId")
            .and_then(Value::as_str)
            .is_some_and(|s| !s.is_empty())
    });

    if has_gateway_id {
        (
            &["displayName", "id", "connectivityType", "gatewayId"],
            &["NAME", "ID", "CONNECTIVITY TYPE", "GATEWAY ID"],
        )
    } else {
        (
            &["displayName", "id", "connectivityType"],
            &["NAME", "ID", "CONNECTIVITY TYPE"],
        )
    }
}

async fn show(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let data = client.get(&format!("/connections/{id}")).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Connectivity types whose create request requires a `gatewayId`.
fn connectivity_type_requires_gateway_id(connectivity_type: &str) -> bool {
    matches!(
        connectivity_type,
        "VirtualNetworkGateway" | "StreamingVirtualNetworkGateway"
    )
}

#[allow(clippy::too_many_arguments)]
async fn create(
    cli: &Cli,
    client: &FabricClient,
    name: &str,
    connectivity_type: &str,
    connection_type: &str,
    creation_method: Option<&str>,
    parameters: &str,
    gateway_id: Option<&str>,
    credential_type: &str,
    credentials: Option<&str>,
    privacy_level: &str,
    skip_test_connection: bool,
    allow_code_first_artifacts: bool,
    allow_gateway_usage: bool,
) -> Result<()> {
    if connectivity_type_requires_gateway_id(connectivity_type) && gateway_id.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("--gateway-id is required when --connectivity-type is '{connectivity_type}'"),
            "Example: --gateway-id <GATEWAY_ID>",
        )
        .into());
    }

    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would create connection '{name}' ({connectivity_type})"),
            "displayName": name,
            "connectivityType": connectivity_type,
            "connectionType": connection_type,
            "creationMethod": creation_method.unwrap_or(connection_type),
            "creationMethodResolution": if creation_method.is_some() { "explicit" } else { "auto (resolved from supportedConnectionTypes at execution)" },
            "allowUsageInUserControlledCode": allow_code_first_artifacts,
            "allowConnectionUsageInGateway": allow_gateway_usage,
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let params: Value = serde_json::from_str(parameters).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --parameters JSON: {e}"),
            "Expected JSON object, e.g.: --parameters '{\"server\":\"host\",\"database\":\"db\"}'",
        )
    })?;

    let cred_details = if let Some(creds) = credentials {
        let cred_value: Value = serde_json::from_str(creds).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --credentials JSON: {e}"),
                "Expected JSON object with credential fields, e.g.: '{\"credentialType\":\"Basic\",\"username\":\"u\",\"password\":\"p\"}'",
            )
        })?;
        let mut details = json!({
            "singleSignOnType": "None",
            "connectionEncryption": "NotEncrypted",
            "skipTestConnection": skip_test_connection,
            "credentials": cred_value,
        });
        // Ensure credentialType is set inside credentials
        if details["credentials"]["credentialType"].is_null() {
            details["credentials"]["credentialType"] = json!(credential_type);
        }
        details
    } else {
        json!({
            "singleSignOnType": "None",
            "connectionEncryption": "NotEncrypted",
            "skipTestConnection": skip_test_connection,
            "credentials": {
                "credentialType": credential_type,
            },
        })
    };

    // Build connection parameters in the API array format
    let connection_params: Vec<Value> = if let Some(obj) = params.as_object() {
        obj.iter()
            .map(|(k, v)| {
                json!({
                    "dataType": "Text",
                    "name": k,
                    "value": v.as_str().unwrap_or(&v.to_string()),
                })
            })
            .collect()
    } else {
        bail!("--parameters must be a JSON object (e.g., '{{\"server\":\"host\"}}')");
    };

    // When --creation-method is omitted, auto-resolve it from the connection type
    // via supportedConnectionTypes (most types differ from the type name, e.g.
    // SQL -> Sql, EventHub -> EventHub.Contents). Falls back to the type name if the
    // catalog is unreachable or the type isn't found; errors (teaching the valid
    // values) for the few types with multiple creation methods.
    let resolved_method: String = match creation_method {
        Some(m) => m.to_string(),
        None => resolve_creation_method_via_api(client, connection_type).await?,
    };

    let body = build_connection_body(
        name,
        connectivity_type,
        connection_type,
        Some(&resolved_method),
        &connection_params,
        &cred_details,
        privacy_level,
        gateway_id,
        allow_code_first_artifacts,
        allow_gateway_usage,
    );

    let data = client.post("/connections", &body, false).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Outcome of resolving a connection type's canonical `creationMethod` from the
/// `supportedConnectionTypes` catalog.
#[derive(Debug, PartialEq)]
enum MethodResolution {
    /// Exactly one canonical method (or one matching the type name).
    Resolved(String),
    /// The type advertises multiple creation methods — the caller must choose one.
    Ambiguous(Vec<String>),
    /// The type is not present in the catalog (caller falls back to the type name).
    Unknown,
}

/// Resolve a connection type's `creationMethod` from the supportedConnectionTypes
/// catalog items. Pure function for testing.
fn resolve_creation_method(types: &[Value], connection_type: &str) -> MethodResolution {
    let entry = types
        .iter()
        .find(|t| t.get("type").and_then(Value::as_str) == Some(connection_type))
        .or_else(|| {
            types.iter().find(|t| {
                t.get("type")
                    .and_then(Value::as_str)
                    .is_some_and(|s| s.eq_ignore_ascii_case(connection_type))
            })
        });
    let Some(entry) = entry else {
        return MethodResolution::Unknown;
    };
    let methods: Vec<String> = entry
        .get("creationMethods")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("name").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    match methods.as_slice() {
        [] => MethodResolution::Unknown,
        [only] => MethodResolution::Resolved(only.clone()),
        many => {
            // Prefer a method that exactly matches the type name if present.
            many.iter()
                .find(|m| m.as_str() == connection_type)
                .map_or_else(
                    || MethodResolution::Ambiguous(many.to_vec()),
                    |exact| MethodResolution::Resolved(exact.clone()),
                )
        }
    }
}

/// Fetch the supportedConnectionTypes catalog and resolve the canonical creation
/// method for `connection_type`. Non-blocking: falls back to the type name if the
/// catalog can't be fetched or the type is unknown; errors (with the valid values)
/// only when the type genuinely has multiple creation methods.
async fn resolve_creation_method_via_api(
    client: &FabricClient,
    connection_type: &str,
) -> Result<String> {
    let Ok(resp) = client
        .get_list("/connections/supportedConnectionTypes", "value", true, None)
        .await
    else {
        // Don't block connection creation on a catalog-fetch failure.
        return Ok(connection_type.to_string());
    };

    match resolve_creation_method(&resp.items, connection_type) {
        MethodResolution::Resolved(m) => Ok(m),
        MethodResolution::Unknown => Ok(connection_type.to_string()),
        MethodResolution::Ambiguous(methods) => Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Connection type '{connection_type}' has multiple creation methods; specify one with --creation-method"
            ),
            format!(
                "Valid creation methods: {}. Example: --creation-method {}",
                methods.join(", "),
                methods.first().map_or("", String::as_str)
            ),
        )
        .into()),
    }
}

/// Build the `POST /connections` request body. `creation_method` defaults to
/// `connection_type` when `None` (callers normally pass the auto-resolved method).
/// Pure function for testing.
#[allow(clippy::too_many_arguments)]
fn build_connection_body(
    name: &str,
    connectivity_type: &str,
    connection_type: &str,
    creation_method: Option<&str>,
    connection_params: &[Value],
    cred_details: &Value,
    privacy_level: &str,
    gateway_id: Option<&str>,
    allow_code_first_artifacts: bool,
    allow_gateway_usage: bool,
) -> Value {
    let method = creation_method.unwrap_or(connection_type);
    let mut body = json!({
        "displayName": name,
        "connectivityType": connectivity_type,
        "connectionDetails": {
            "type": connection_type,
            "creationMethod": method,
            "parameters": connection_params,
        },
        "credentialDetails": cred_details,
        "privacyLevel": privacy_level,
    });
    if let Some(gw_id) = gateway_id {
        body["gatewayId"] = json!(gw_id);
    }
    // Top-level booleans (default false server-side); only send when enabled.
    if allow_code_first_artifacts {
        body["allowUsageInUserControlledCode"] = json!(true);
    }
    if allow_gateway_usage {
        body["allowConnectionUsageInGateway"] = json!(true);
    }
    body
}

async fn delete(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would delete connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    client.delete(&format!("/connections/{id}")).await?;

    let result = json!({
        "status": "deleted",
        "id": id,
    });
    output::render_object(cli, &result, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    name: Option<&str>,
    privacy_level: Option<&str>,
    credential_type: Option<&str>,
    credentials: Option<&str>,
) -> Result<()> {
    if name.is_none() && privacy_level.is_none() && credential_type.is_none() {
        bail!(
            "At least one of --name, --privacy-level, or --credential-type must be provided. Example: fabio connection update --id <ID> --name \"New Name\""
        );
    }

    // `UpdateConnectionRequest` is a discriminated union on `connectivityType`
    // (a REQUIRED field). Fetch the current connection so the PATCH body carries
    // the correct discriminator — omitting it fails with `InvalidInput`.
    let current = client.get(&format!("/connections/{id}")).await?;
    let connectivity_type = current["connectivityType"]
        .as_str()
        .unwrap_or("ShareableCloud")
        .to_string();

    let body = build_connection_update_body(
        &connectivity_type,
        name,
        privacy_level,
        credential_type,
        credentials,
    )?;

    if cli.dry_run {
        // Redact credential values from the dry-run preview
        let mut safe_body = body.clone();
        if let Some(cred) = safe_body.get_mut("credentialDetails")
            && let Some(creds) = cred.get_mut("credentials")
        {
            *creds = serde_json::json!("[REDACTED]");
        }
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would update connection '{id}'"),
            "updates": safe_body,
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let data = client.patch(&format!("/connections/{id}"), &body).await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the `PATCH /connections/{id}` body. The request is a discriminated
/// union on `connectivityType` (a REQUIRED field), so the caller must pass the
/// connection's current type; omitting it fails with `InvalidInput`.
fn build_connection_update_body(
    connectivity_type: &str,
    name: Option<&str>,
    privacy_level: Option<&str>,
    credential_type: Option<&str>,
    credentials: Option<&str>,
) -> Result<Value> {
    let mut body = json!({ "connectivityType": connectivity_type });
    if let Some(n) = name {
        body["displayName"] = json!(n);
    }
    if let Some(pl) = privacy_level {
        body["privacyLevel"] = json!(pl);
    }
    if credential_type.is_some() || credentials.is_some() {
        let mut cred_details = json!({});
        if let Some(ct) = credential_type {
            cred_details["credentials"] = json!({ "credentialType": ct });
        }
        if let Some(creds) = credentials {
            let cred_value: Value = serde_json::from_str(creds).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid --credentials JSON: {e}"),
                    "Expected JSON object with credential fields.",
                )
            })?;
            if cred_details["credentials"].is_null() {
                cred_details["credentials"] = cred_value;
            } else if let Some(obj) = cred_details["credentials"].as_object_mut()
                && let Some(cred_obj) = cred_value.as_object()
            {
                for (k, v) in cred_obj {
                    obj.insert(k.clone(), v.clone());
                }
            }
        }
        body["credentialDetails"] = cred_details;
    }
    Ok(body)
}

async fn list_supported_types(cli: &Cli, client: &FabricClient) -> Result<()> {
    let resp = client
        .get_list(
            "/connections/supportedConnectionTypes",
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "displayName"],
        &["TYPE", "DISPLAY NAME"],
        "name",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn list_role_assignments(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/connections/{id}/roleAssignments"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "role", "principal.id", "principal.type"],
        &["ID", "ROLE", "PRINCIPAL ID", "PRINCIPAL TYPE"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn add_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    principal_id: &str,
    principal_type: &str,
    role: &str,
) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would add role assignment '{role}' for principal '{principal_id}' on connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({
        "principal": {
            "id": principal_id,
            "type": principal_type,
        },
        "role": role,
    });

    let data = client
        .post(&format!("/connections/{id}/roleAssignments"), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "connection add-role-assignment", "Owner"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn show_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn update_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
    role: &str,
) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would update role assignment '{assignment_id}' to role '{role}' on connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({ "role": role });

    let data = client
        .patch(
            &format!("/connections/{id}/roleAssignments/{assignment_id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "connection update-role-assignment", "Owner"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete_role_assignment(
    cli: &Cli,
    client: &FabricClient,
    id: &str,
    assignment_id: &str,
) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would delete role assignment '{assignment_id}' from connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    client
        .delete(&format!(
            "/connections/{id}/roleAssignments/{assignment_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "connection delete-role-assignment", "Owner"))?;

    let result = json!({
        "status": "deleted",
        "id": assignment_id,
        "connectionId": id,
    });
    output::render_object(cli, &result, "id");
    Ok(())
}

async fn test_connection(cli: &Cli, client: &FabricClient, id: &str) -> Result<()> {
    if cli.dry_run {
        let preview = json!({
            "status": "dry_run",
            "message": format!("Would test connection '{id}'"),
        });
        output::render_object(cli, &preview, "status");
        return Ok(());
    }

    let body = json!({});
    let data = client
        .post(&format!("/connections/{id}/testConnection"), &body, false)
        .await
        .map_err(|e| enrich_forbidden(e, "connection test-connection", "User"))?;
    output::render_object(cli, &data, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_body_always_includes_connectivity_type() {
        // The PATCH body is a discriminated union on connectivityType — it must
        // always be present, even when only unrelated fields change.
        let body =
            build_connection_update_body("ShareableCloud", Some("New Name"), None, None, None)
                .unwrap();
        assert_eq!(body["connectivityType"], "ShareableCloud");
        assert_eq!(body["displayName"], "New Name");
    }

    #[test]
    fn update_body_sets_privacy_level() {
        let body = build_connection_update_body(
            "OnPremisesGateway",
            None,
            Some("Organizational"),
            None,
            None,
        )
        .unwrap();
        assert_eq!(body["connectivityType"], "OnPremisesGateway");
        assert_eq!(body["privacyLevel"], "Organizational");
        assert!(body.get("displayName").is_none());
    }

    #[test]
    fn update_body_wraps_credential_type() {
        let body = build_connection_update_body("ShareableCloud", None, None, Some("Basic"), None)
            .unwrap();
        assert_eq!(
            body["credentialDetails"]["credentials"]["credentialType"],
            "Basic"
        );
    }

    #[test]
    fn update_body_rejects_invalid_credentials_json() {
        assert!(
            build_connection_update_body("ShareableCloud", None, None, None, Some("{bad")).is_err()
        );
    }

    #[test]
    fn gateway_id_required_for_virtual_network_gateway() {
        assert!(connectivity_type_requires_gateway_id(
            "VirtualNetworkGateway"
        ));
    }

    #[test]
    fn gateway_id_required_for_streaming_virtual_network_gateway() {
        assert!(connectivity_type_requires_gateway_id(
            "StreamingVirtualNetworkGateway"
        ));
    }

    #[test]
    fn gateway_id_not_required_for_other_types() {
        assert!(!connectivity_type_requires_gateway_id("ShareableCloud"));
        assert!(!connectivity_type_requires_gateway_id("OnPremises"));
        assert!(!connectivity_type_requires_gateway_id("PersonalCloud"));
    }

    #[test]
    fn list_table_columns_includes_gateway_id_when_present() {
        let items = vec![json!({
            "id": "conn-1",
            "displayName": "Conn",
            "connectivityType": "OnPremises",
            "gatewayId": "gw-1"
        })];
        let (columns, headers) = list_table_columns(&items);
        assert_eq!(
            columns,
            ["displayName", "id", "connectivityType", "gatewayId"]
        );
        assert_eq!(headers, ["NAME", "ID", "CONNECTIVITY TYPE", "GATEWAY ID"]);
    }

    #[test]
    fn list_table_columns_omits_gateway_id_when_missing_or_null() {
        let items = vec![
            json!({
                "id": "conn-1",
                "displayName": "Conn 1",
                "connectivityType": "OnPremises"
            }),
            json!({
                "id": "conn-2",
                "displayName": "Conn 2",
                "connectivityType": "ShareableCloud",
                "gatewayId": null
            }),
        ];
        let (columns, headers) = list_table_columns(&items);
        assert_eq!(columns, ["displayName", "id", "connectivityType"]);
        assert_eq!(headers, ["NAME", "ID", "CONNECTIVITY TYPE"]);
    }

    #[test]
    fn list_table_columns_omits_gateway_id_when_empty_string() {
        let items = vec![json!({
            "id": "conn-1",
            "displayName": "Conn",
            "connectivityType": "OnPremises",
            "gatewayId": ""
        })];
        let (columns, headers) = list_table_columns(&items);
        assert_eq!(columns, ["displayName", "id", "connectivityType"]);
        assert_eq!(headers, ["NAME", "ID", "CONNECTIVITY TYPE"]);
    }

    #[test]
    fn creation_method_defaults_to_connection_type() {
        let creds = json!({"credentialType": "WorkspaceIdentity"});
        let body = build_connection_body(
            "c1",
            "ShareableCloud",
            "SQL",
            None,
            &[],
            &creds,
            "Organizational",
            None,
            false,
            false,
        );
        assert_eq!(body["connectionDetails"]["type"], "SQL");
        assert_eq!(body["connectionDetails"]["creationMethod"], "SQL");
        assert!(body.get("allowUsageInUserControlledCode").is_none());
        assert!(body.get("allowConnectionUsageInGateway").is_none());
        assert!(body.get("gatewayId").is_none());
    }

    #[test]
    fn creation_method_override_is_used() {
        // EventHub's creation method is EventHub.Contents, not EventHub.
        let creds = json!({"credentialType": "WorkspaceIdentity"});
        let params = vec![json!({"dataType": "Text", "name": "endpoint", "value": "sb://x"})];
        let body = build_connection_body(
            "eh",
            "ShareableCloud",
            "EventHub",
            Some("EventHub.Contents"),
            &params,
            &creds,
            "Organizational",
            None,
            true,
            false,
        );
        assert_eq!(body["connectionDetails"]["type"], "EventHub");
        assert_eq!(body["allowUsageInUserControlledCode"], true);
        assert_eq!(
            body["connectionDetails"]["creationMethod"],
            "EventHub.Contents"
        );
        assert_eq!(
            body["connectionDetails"]["parameters"][0]["name"],
            "endpoint"
        );
    }

    #[test]
    fn build_body_includes_gateway_when_present() {
        let creds = json!({"credentialType": "Basic"});
        let body = build_connection_body(
            "vnet",
            "VirtualNetworkGateway",
            "SQL",
            None,
            &[],
            &creds,
            "Organizational",
            Some("gw-123"),
            false,
            true,
        );
        assert_eq!(body["gatewayId"], "gw-123");
        assert_eq!(body["allowConnectionUsageInGateway"], true);
    }

    fn catalog() -> Vec<Value> {
        vec![
            json!({"type": "SQL", "creationMethods": [{"name": "Sql"}]}),
            json!({"type": "EventHub", "creationMethods": [{"name": "EventHub.Contents"}]}),
            json!({"type": "Web", "creationMethods": [{"name": "Web"}]}),
            json!({"type": "AzureDataExplorer", "creationMethods": [
                {"name": "AzureDataExplorer.Contents"}, {"name": "AzureDataExplorer.KqlDatabase"}
            ]}),
            json!({"type": "NoMethods", "creationMethods": []}),
        ]
    }

    #[test]
    fn resolve_single_method_differing_from_type() {
        assert_eq!(
            resolve_creation_method(&catalog(), "SQL"),
            MethodResolution::Resolved("Sql".to_string())
        );
        assert_eq!(
            resolve_creation_method(&catalog(), "EventHub"),
            MethodResolution::Resolved("EventHub.Contents".to_string())
        );
    }

    #[test]
    fn resolve_method_matching_type_name() {
        assert_eq!(
            resolve_creation_method(&catalog(), "Web"),
            MethodResolution::Resolved("Web".to_string())
        );
    }

    #[test]
    fn resolve_case_insensitive_type_match() {
        assert_eq!(
            resolve_creation_method(&catalog(), "eventhub"),
            MethodResolution::Resolved("EventHub.Contents".to_string())
        );
    }

    #[test]
    fn resolve_ambiguous_multiple_methods() {
        assert_eq!(
            resolve_creation_method(&catalog(), "AzureDataExplorer"),
            MethodResolution::Ambiguous(vec![
                "AzureDataExplorer.Contents".to_string(),
                "AzureDataExplorer.KqlDatabase".to_string(),
            ])
        );
    }

    #[test]
    fn resolve_unknown_type_or_no_methods() {
        assert_eq!(
            resolve_creation_method(&catalog(), "NotInCatalog"),
            MethodResolution::Unknown
        );
        assert_eq!(
            resolve_creation_method(&catalog(), "NoMethods"),
            MethodResolution::Unknown
        );
    }
}
