use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::read_json_input;

// ─── Bulk Post (server-side LRO) ─────────────────────────────────────────────

pub(super) async fn bulk_post(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    operation: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_input(file, content, operation)?;

    if output::dry_run_guard(cli, &format!("item {operation}"), &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/items/{operation}"),
            &body,
            true,
        )
        .await?;

    output::render_object(cli, &data, "status");
    Ok(())
}

// ─── Bulk Create (client-side parallel) ──────────────────────────────────────

pub(super) async fn bulk_create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_json_input(file, content, "bulk-create")?;
    let items = body.as_array().ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Expected a JSON array of items".to_string(),
            "Example: [{\"displayName\":\"Item1\",\"type\":\"Lakehouse\"}, ...]".to_string(),
        )
    })?;

    if items.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Item array is empty".to_string(),
            "Provide at least one item to create.".to_string(),
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "item bulk-create",
        &serde_json::json!({
            "workspace": workspace,
            "count": items.len(),
            "items": items
        }),
    ) {
        return Ok(());
    }

    let workspace_owned = workspace.to_owned();
    let items_owned: Vec<Value> = items.clone();
    let items_ref = items_owned.clone(); // Keep a copy for result reporting
    let client_arc = std::sync::Arc::new(client.clone());
    let concurrency = crate::parallel::default_concurrency();

    let results = crate::parallel::execute_parallel(items_owned, concurrency, {
        let ws = workspace_owned.clone();
        let c = client_arc.clone();
        move |item| {
            let ws = ws.clone();
            let c = c.clone();
            async move {
                let resp = c
                    .post(&format!("/workspaces/{ws}/items"), &item, true)
                    .await?;
                Ok(resp)
            }
        }
    })
    .await;

    // Collect results
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for r in &results {
        let item_name = items_ref
            .get(r.index)
            .and_then(|v| v.get("displayName"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        match &r.result {
            Ok(data) => {
                succeeded.push(serde_json::json!({
                    "displayName": item_name,
                    "id": data.get("id").and_then(|v| v.as_str()).unwrap_or(""),
                    "type": data.get("type").and_then(|v| v.as_str()).unwrap_or(""),
                }));
            }
            Err(e) => {
                failed.push(serde_json::json!({
                    "displayName": item_name,
                    "error": e.message,
                }));
            }
        }
    }

    let result = serde_json::json!({
        "succeeded": succeeded.len(),
        "failed": failed.len(),
        "items": succeeded,
        "failures": failed,
    });
    output::render_object(cli, &result, "succeeded");
    Ok(())
}

// ─── Bulk Delete (client-side parallel) ──────────────────────────────────────

pub(super) async fn bulk_delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    ids: &[String],
) -> Result<()> {
    // Filter out empty strings (e.g., from `--ids ""`)
    let ids: Vec<&str> = ids
        .iter()
        .map(String::as_str)
        .filter(|s| !s.is_empty())
        .collect();

    if ids.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "No item IDs provided".to_string(),
            "Example: fabio item bulk-delete --workspace <WS> --ids id1,id2,id3".to_string(),
        )
        .into());
    }

    if output::dry_run_guard(
        cli,
        "item bulk-delete",
        &serde_json::json!({
            "workspace": workspace,
            "count": ids.len(),
            "ids": ids
        }),
    ) {
        return Ok(());
    }

    let workspace_owned = workspace.to_owned();
    let ids_owned: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
    let client_arc = std::sync::Arc::new(client.clone());
    let concurrency = crate::parallel::default_concurrency();

    let results = crate::parallel::execute_parallel(ids_owned.clone(), concurrency, {
        let ws = workspace_owned.clone();
        let c = client_arc.clone();
        move |id| {
            let ws = ws.clone();
            let c = c.clone();
            async move {
                c.delete(&format!("/workspaces/{ws}/items/{id}")).await?;
                Ok(id)
            }
        }
    })
    .await;

    // Collect results
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for r in &results {
        let id = &ids_owned[r.index];
        match &r.result {
            Ok(_) => {
                succeeded.push(serde_json::json!({"id": id, "status": "deleted"}));
            }
            Err(e) => {
                failed.push(serde_json::json!({"id": id, "error": e.message}));
            }
        }
    }

    let result = serde_json::json!({
        "succeeded": succeeded.len(),
        "failed": failed.len(),
        "items": succeeded,
        "failures": failed,
    });
    output::render_object(cli, &result, "succeeded");
    Ok(())
}

// ─── External Data Shares ────────────────────────────────────────────────────

pub(super) async fn list_external_data_shares(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{id}/externalDataShares"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["id", "status"],
        &["ID", "STATUS"],
        "id",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create_external_data_share(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    paths: &[String],
    recipient_type: &str,
    recipient_email: Option<&str>,
    recipient_id: Option<&str>,
    recipient_tenant_id: Option<&str>,
) -> Result<()> {
    let recipient = build_eds_recipient(
        recipient_type,
        recipient_email,
        recipient_id,
        recipient_tenant_id,
    )?;

    let body = serde_json::json!({
        "paths": paths,
        "recipient": recipient
    });

    if output::dry_run_guard(cli, "item create-external-data-share", &body) {
        return Ok(());
    }

    let data = client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/externalDataShares"),
            &body,
            false,
        )
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

/// Build the external-data-share `recipient` object. The API models the
/// recipient as a discriminated union on `type`: a `User` recipient is
/// identified by `userPrincipalName` (email, `tenantId` optional), a
/// `ServicePrincipal` recipient by `principalId` + `tenantId` (both required).
/// The older `{objectId, recipientType}` shape is rejected by the API (500).
fn build_eds_recipient(
    recipient_type: &str,
    recipient_email: Option<&str>,
    recipient_id: Option<&str>,
    recipient_tenant_id: Option<&str>,
) -> Result<Value> {
    if recipient_type.eq_ignore_ascii_case("User") {
        let upn = recipient_email.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--recipient-email is required for --recipient-type User",
                "Example: --recipient-type User --recipient-email alice@contoso.com",
            )
        })?;
        let mut r = serde_json::json!({ "type": "User", "userPrincipalName": upn });
        if let Some(tid) = recipient_tenant_id {
            r["tenantId"] = Value::from(tid);
        }
        Ok(r)
    } else if recipient_type.eq_ignore_ascii_case("ServicePrincipal") {
        let (Some(pid), Some(tid)) = (recipient_id, recipient_tenant_id) else {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--recipient-id and --recipient-tenant-id are both required for --recipient-type ServicePrincipal",
                "Example: --recipient-type ServicePrincipal --recipient-id <object-id> --recipient-tenant-id <tenant-id>",
            )
            .into());
        };
        Ok(serde_json::json!({
            "type": "ServicePrincipal",
            "principalId": pid,
            "tenantId": tid
        }))
    } else {
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "Invalid --recipient-type '{recipient_type}'. Valid values: User, ServicePrincipal"
            ),
            "Example: --recipient-type User --recipient-email alice@contoso.com",
        )
        .into())
    }
}

pub(super) async fn show_external_data_share(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    share_id: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{id}/externalDataShares/{share_id}"
        ))
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn revoke_external_data_share(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    share_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "item revoke-external-data-share",
        &serde_json::json!({ "workspace": workspace, "id": id, "share_id": share_id }),
    ) {
        return Ok(());
    }

    client
        .post(
            &format!("/workspaces/{workspace}/items/{id}/externalDataShares/{share_id}/revoke"),
            &serde_json::json!({}),
            false,
        )
        .await?;

    let obj = serde_json::json!({ "id": share_id, "status": "revoked" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

pub(super) async fn delete_external_data_share(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    share_id: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "item delete-external-data-share",
        &serde_json::json!({ "workspace": workspace, "id": id, "share_id": share_id }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/items/{id}/externalDataShares/{share_id}"
        ))
        .await?;

    let obj = serde_json::json!({ "id": share_id, "status": "deleted" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::build_eds_recipient;

    #[test]
    fn user_recipient_uses_upn() {
        let r = build_eds_recipient("User", Some("alice@contoso.com"), None, None).unwrap();
        assert_eq!(r["type"], "User");
        assert_eq!(r["userPrincipalName"], "alice@contoso.com");
        assert!(r.get("tenantId").is_none());
        assert!(r.get("objectId").is_none());
        assert!(r.get("recipientType").is_none());
    }

    #[test]
    fn user_recipient_includes_optional_tenant() {
        let r = build_eds_recipient("user", Some("a@b.com"), None, Some("tid-123")).unwrap();
        assert_eq!(r["tenantId"], "tid-123");
    }

    #[test]
    fn user_recipient_requires_email() {
        assert!(build_eds_recipient("User", None, None, None).is_err());
    }

    #[test]
    fn service_principal_uses_principal_id() {
        let r =
            build_eds_recipient("ServicePrincipal", None, Some("obj-1"), Some("tid-1")).unwrap();
        assert_eq!(r["type"], "ServicePrincipal");
        assert_eq!(r["principalId"], "obj-1");
        assert_eq!(r["tenantId"], "tid-1");
    }

    #[test]
    fn service_principal_requires_id_and_tenant() {
        assert!(build_eds_recipient("ServicePrincipal", None, Some("obj-1"), None).is_err());
        assert!(build_eds_recipient("ServicePrincipal", None, None, Some("tid-1")).is_err());
    }

    #[test]
    fn unknown_type_rejected() {
        assert!(build_eds_recipient("Group", None, None, None).is_err());
    }
}
