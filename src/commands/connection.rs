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

        /// Connection parameters as JSON (e.g., '{"server":"host","database":"db"}')
        #[arg(long)]
        parameters: String,

        /// Gateway ID through which the connection is made (required when `--connectivity-type` is `VirtualNetworkGateway` or `StreamingVirtualNetworkGateway`; optional for other connectivity types)
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
            parameters,
            gateway_id,
            credential_type,
            credentials,
            privacy_level,
            skip_test_connection,
        } => {
            create(
                cli,
                client,
                name,
                connectivity_type,
                connection_type,
                parameters,
                gateway_id.as_deref(),
                credential_type,
                credentials.as_deref(),
                privacy_level,
                *skip_test_connection,
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
    let has_gateway_id = items
        .iter()
        .any(|item| item.get("gatewayId").is_some_and(|v| !v.is_null()));

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
    parameters: &str,
    gateway_id: Option<&str>,
    credential_type: &str,
    credentials: Option<&str>,
    privacy_level: &str,
    skip_test_connection: bool,
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

    let mut body = json!({
        "displayName": name,
        "connectivityType": connectivity_type,
        "connectionDetails": {
            "type": connection_type,
            "creationMethod": connection_type,
            "parameters": connection_params,
        },
        "credentialDetails": cred_details,
        "privacyLevel": privacy_level,
    });
    if let Some(gw_id) = gateway_id {
        body["gatewayId"] = json!(gw_id);
    }

    let data = client.post("/connections", &body, false).await?;
    output::render_object(cli, &data, "id");
    Ok(())
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

    let mut body = json!({});
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
}
