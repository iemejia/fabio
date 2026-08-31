use anyhow::Result;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use clap::Subcommand;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::commands::tds_utils;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum DigitalTwinBuilderCommand {
    /// List Digital Twin Builders in a workspace
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
    },
    /// Show details of a Digital Twin Builder
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
    },
    /// Create a new Digital Twin Builder
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
    /// Update Digital Twin Builder properties
    #[command(display_order = 4)]
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
    },
    /// Delete a Digital Twin Builder
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,

        /// Permanently delete (cannot be recovered)
        #[arg(long)]
        hard_delete: bool,

        /// Also delete the associated `<name>dtdm` data lakehouse (NOT removed
        /// automatically). Resolves it from the definition before deleting.
        #[arg(long)]
        delete_lakehouse: bool,
    },
    /// Get the definition of a Digital Twin Builder
    #[command(display_order = 6)]
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline (adds decodedPayload field)
        #[arg(long)]
        decode: bool,
    },
    /// Update the definition of a Digital Twin Builder
    #[command(display_order = 7)]
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
        /// Path to definition file
        #[arg(long)]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
    },

    /// Resolve the associated data lakehouse (the `<name>dtdm` lakehouse where the
    /// twin's ontology/instance data lives) and its SQL analytics endpoint.
    #[command(name = "show-lakehouse", display_order = 8)]
    ShowLakehouse {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
    },

    /// Run a T-SQL query against the twin's data (the associated `dtdm` lakehouse
    /// SQL endpoint). Query the `dom` domain views (recommended) or `dbo` base tables.
    #[command(display_order = 9)]
    Query {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Digital Twin Builder ID
        #[arg(long)]
        id: String,
        /// SQL query text (use `@file.sql` to read from a file, or omit to pipe via stdin)
        #[arg(long)]
        sql: Option<String>,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &DigitalTwinBuilderCommand,
) -> Result<()> {
    match command {
        DigitalTwinBuilderCommand::List { workspace } => list(cli, client, workspace).await,
        DigitalTwinBuilderCommand::Show { workspace, id } => show(cli, client, workspace, id).await,
        DigitalTwinBuilderCommand::Create {
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
        DigitalTwinBuilderCommand::Update {
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
        DigitalTwinBuilderCommand::Delete {
            workspace,
            id,
            hard_delete,
            delete_lakehouse,
        } => delete(cli, client, workspace, id, *hard_delete, *delete_lakehouse).await,
        DigitalTwinBuilderCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        DigitalTwinBuilderCommand::UpdateDefinition {
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
        DigitalTwinBuilderCommand::ShowLakehouse { workspace, id } => {
            show_lakehouse(cli, client, workspace, id).await
        }
        DigitalTwinBuilderCommand::Query { workspace, id, sql } => {
            query(cli, client, workspace, id, sql.as_deref()).await
        }
    }
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/digitalTwinBuilders"),
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
        .get(&format!("/workspaces/{workspace}/digitalTwinBuilders/{id}"))
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
        "digital-twin-builder create",
        &serde_json::json!({ "workspace": workspace, "displayName": name, "description": description , "sensitivityLabel": sensitivity_label }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilders"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder create", "Contributor"))?;
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
            "Example: fabio digital-twin-builder update --workspace <WS> --id <ID> --name \"New Name\"".to_string(),
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
    if output::dry_run_guard(cli, "digital-twin-builder update", &body) {
        return Ok(());
    }
    let data = client
        .patch(
            &format!("/workspaces/{workspace}/digitalTwinBuilders/{id}"),
            &body,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard_delete: bool,
    delete_lakehouse: bool,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "digital-twin-builder delete",
        &serde_json::json!({ "workspace": workspace, "id": id, "hardDelete": hard_delete, "deleteLakehouse": delete_lakehouse }),
    ) {
        return Ok(());
    }

    // Resolve the linked data lakehouse BEFORE deleting the DTB (getDefinition is
    // unavailable once the item is gone). Best-effort: an unmodeled/odd DTB may
    // not expose a LakehouseId, in which case we skip the cascade.
    let lakehouse_id = if delete_lakehouse {
        resolve_lakehouse_id(client, workspace, id).await.ok()
    } else {
        None
    };

    let url = if hard_delete {
        format!("/workspaces/{workspace}/digitalTwinBuilders/{id}?hardDelete=true")
    } else {
        format!("/workspaces/{workspace}/digitalTwinBuilders/{id}")
    };

    client
        .delete(&url)
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder delete", "Contributor"))?;

    let mut obj = serde_json::json!({ "id": id, "status": "deleted" });
    if delete_lakehouse {
        if let Some(lh) = &lakehouse_id {
            // Best-effort cleanup of the orphaned data lakehouse.
            let deleted = client
                .delete(&format!("/workspaces/{workspace}/lakehouses/{lh}"))
                .await
                .is_ok();
            obj["dataLakehouseId"] = Value::from(lh.as_str());
            obj["dataLakehouseDeleted"] = Value::from(deleted);
            if !deleted {
                obj["note"] = Value::from(format!(
                    "Could not delete the data lakehouse '{lh}' automatically; remove it with: \
                     fabio lakehouse delete --workspace {workspace} --id {lh}"
                ));
            }
        } else {
            obj["note"] = Value::from(
                "Could not resolve the associated data lakehouse from the definition; \
                 delete the '<name>dtdm' lakehouse manually if it remains.",
            );
        }
    } else {
        obj["note"] = Value::from(
            "The associated data lakehouse ('<name>dtdm', where the twin's ontology/instance \
             data lives) is NOT deleted automatically. Re-run with --delete-lakehouse to remove \
             it, or delete it with: fabio lakehouse delete.",
        );
    }
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
            &format!("/workspaces/{workspace}/digitalTwinBuilders/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder get-definition", "Contributor"))?;
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
                "Example: fabio digital-twin-builder update-definition --workspace <WS> --id <ID> --file definition.json".to_string(),
            )
            .into());
        }
    };
    let body = crate::definition_spec::build_update_definition_body(&script, "definition.json");
    if output::dry_run_guard(
        cli,
        "digital-twin-builder update-definition",
        &serde_json::json!({ "workspace": workspace, "id": id, "contentLength": script.len() }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilders/{id}/updateDefinition"),
            &body,
            true,
        )
        .await
        .map_err(|e| {
            enrich_forbidden(e, "digital-twin-builder update-definition", "Contributor")
        })?;
    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "definition_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

/// Resolve the `LakehouseId` from a Digital Twin Builder's definition. The DTB
/// item stores its associated `<name>dtdm` data lakehouse in the single
/// `definition.json` part as `{"LakehouseId": "<uuid>"}` (auto-provisioned at
/// create time).
async fn resolve_lakehouse_id(client: &FabricClient, workspace: &str, id: &str) -> Result<String> {
    let data = client
        .post(
            &format!("/workspaces/{workspace}/digitalTwinBuilders/{id}/getDefinition"),
            &serde_json::json!({}),
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "digital-twin-builder show-lakehouse", "Contributor"))?;
    lakehouse_id_from_definition(&data)
}

/// Extract the linked `LakehouseId` from a `getDefinition` response (pure).
fn lakehouse_id_from_definition(data: &Value) -> Result<String> {
    let parts = data
        .pointer("/definition/parts")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FabioError::api_error("Unexpected getDefinition response (no definition.parts)")
        })?;
    let payload = parts
        .iter()
        .find(|p| p.get("path").and_then(Value::as_str) == Some("definition.json"))
        .and_then(|p| p.get("payload").and_then(Value::as_str))
        .ok_or_else(|| FabioError::not_found("Digital Twin Builder has no definition.json part"))?;
    let decoded = BASE64
        .decode(payload)
        .map_err(|e| FabioError::api_error(format!("Failed to decode definition.json: {e}")))?;
    let def: Value = serde_json::from_slice(&decoded)
        .map_err(|e| FabioError::api_error(format!("Invalid definition.json: {e}")))?;
    def.get("LakehouseId")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                "This Digital Twin Builder has no linked data lakehouse (LakehouseId) yet.",
                "The data lakehouse is provisioned when the item is created and modeled. \
                 If this is a brand-new or unmodeled item, retry after modeling in the portal.",
            )
            .into()
        })
}

async fn show_lakehouse(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    let lakehouse_id = resolve_lakehouse_id(client, workspace, id).await?;
    let lh = client
        .get(&format!(
            "/workspaces/{workspace}/lakehouses/{lakehouse_id}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "lakehouse show", "Viewer"))?;

    let name = lh.get("displayName").and_then(Value::as_str).unwrap_or("");
    let sql = lh.pointer("/properties/sqlEndpointProperties");
    let connection_string = sql
        .and_then(|s| s.get("connectionString"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let (server, _db) = tds_utils::parse_connection_string(connection_string);

    let out = serde_json::json!({
        "digitalTwinBuilderId": id,
        "lakehouseId": lakehouse_id,
        "lakehouseName": name,
        "sqlEndpoint": {
            "id": sql.and_then(|s| s.get("id")).and_then(Value::as_str),
            "connectionString": connection_string,
            "server": server,
            "database": name,
        },
        "note": "Query the twin's data via this lakehouse's SQL endpoint: the 'dom' schema \
                 holds the domain views (recommended for analytics) and 'dbo' holds the base-layer \
                 tables. Use: fabio digital-twin-builder query --id <DTB> --sql \"SELECT * FROM dom.<View>\".",
    });
    output::render_object(cli, &out, "lakehouseId");
    Ok(())
}

async fn query(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    sql: Option<&str>,
) -> Result<()> {
    let sql_text = tds_utils::resolve_sql_input(sql)?;
    let lakehouse_id = resolve_lakehouse_id(client, workspace, id).await?;
    let (server, database) =
        tds_utils::resolve_lakehouse_sql(client, workspace, &lakehouse_id).await?;
    tds_utils::execute_and_render_sql(cli, client, &server, &database, &sql_text).await
}

#[cfg(test)]
mod tests {
    use super::lakehouse_id_from_definition;
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;
    use serde_json::json;

    fn get_definition_response(def_json: &str) -> serde_json::Value {
        let payload = BASE64.encode(def_json.as_bytes());
        json!({
            "definition": {
                "parts": [
                    { "path": ".platform", "payload": "e30=", "payloadType": "InlineBase64" },
                    { "path": "definition.json", "payload": payload, "payloadType": "InlineBase64" }
                ]
            }
        })
    }

    #[test]
    fn extracts_lakehouse_id_from_definition() {
        let data =
            get_definition_response(r#"{"LakehouseId":"de93a958-5ae6-4721-a791-81c2d07ed59e"}"#);
        assert_eq!(
            lakehouse_id_from_definition(&data).unwrap(),
            "de93a958-5ae6-4721-a791-81c2d07ed59e"
        );
    }

    #[test]
    fn missing_lakehouse_id_is_a_helpful_error() {
        // Definition present but no LakehouseId (unmodeled item).
        let data = get_definition_response(r"{}");
        let err = lakehouse_id_from_definition(&data).unwrap_err().to_string();
        assert!(err.contains("no linked data lakehouse"), "got: {err}");
    }

    #[test]
    fn missing_definition_part_errors() {
        let data = json!({"definition": {"parts": [{"path": ".platform", "payload": "e30="}]}});
        assert!(lakehouse_id_from_definition(&data).is_err());
    }

    #[test]
    fn malformed_response_errors() {
        assert!(lakehouse_id_from_definition(&json!({})).is_err());
    }
}
