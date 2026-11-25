use std::collections::HashSet;

use anyhow::Result;
use clap::{Subcommand, ValueEnum};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, HintType, enrich_forbidden};
use crate::output;

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum PrincipalAccessRoot {
    #[value(name = "Tables")]
    Tables,
    #[value(name = "Files")]
    Files,
}

impl PrincipalAccessRoot {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Tables => "Tables",
            Self::Files => "Files",
        }
    }
}

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples onelake-security\nReturns response shapes, required parameters, and JMESPath queries as JSON."
)]
pub enum OnelakeSecurityCommand {
    /// List data access roles for an item
    #[command(display_order = 1)]
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID (e.g., lakehouse ID)
        #[arg(long)]
        id: String,
    },
    /// Show details of a data access role
    #[command(display_order = 2)]
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Data access role name
        #[arg(long)]
        role_name: String,
    },
    /// Create or update a single data access role
    #[command(display_order = 3)]
    Create {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Role definition as JSON or path to JSON file (prefix with @)
        #[arg(long)]
        role: String,

        /// Conflict policy when role already exists (Abort or Overwrite)
        #[arg(long, default_value = "Overwrite")]
        conflict_policy: String,
    },
    /// Replace all data access roles for an item (atomic PUT)
    #[command(display_order = 4)]
    Upsert {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Roles definition as JSON (array of role objects)
        /// or path to a JSON file (prefix with @)
        #[arg(long)]
        roles: String,
    },
    /// Delete a data access role
    #[command(display_order = 5)]
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Item ID
        #[arg(long)]
        id: String,

        /// Data access role name to delete
        #[arg(long)]
        role_name: String,
    },
    /// Get a principal's effective `OneLake` access, including row and column constraints
    #[command(name = "get-principal-access", display_order = 6)]
    GetPrincipalAccess {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Fabric item ID to evaluate
        #[arg(long)]
        id: String,

        /// Principal object ID; defaults to the caller
        #[arg(long)]
        principal_id: Option<String>,

        /// Restrict results to the Tables or Files root
        #[arg(long, value_enum)]
        root_path: Option<PrincipalAccessRoot>,

        /// Expand table grants into individual table entries; requires --root-path Tables
        #[arg(long)]
        expand_tables: bool,

        /// Server page size (1-1000; defaults to 500)
        #[arg(long, value_parser = clap::value_parser!(u16).range(1..=1000))]
        page_size: Option<u16>,

        /// Require the computed access state to match this quoted `ETag`
        #[arg(long)]
        if_match: Option<String>,

        /// Require the computed access state not to match this quoted `ETag`
        #[arg(long)]
        if_none_match: Option<String>,
    },
}

pub async fn execute(
    cli: &Cli,
    client: &FabricClient,
    command: &OnelakeSecurityCommand,
) -> Result<()> {
    match command {
        OnelakeSecurityCommand::List { workspace, id } => list(cli, client, workspace, id).await,
        OnelakeSecurityCommand::Show {
            workspace,
            id,
            role_name,
        } => show(cli, client, workspace, id, role_name).await,
        OnelakeSecurityCommand::Create {
            workspace,
            id,
            role,
            conflict_policy,
        } => create(cli, client, workspace, id, role, conflict_policy).await,
        OnelakeSecurityCommand::Upsert {
            workspace,
            id,
            roles,
        } => upsert(cli, client, workspace, id, roles).await,
        OnelakeSecurityCommand::Delete {
            workspace,
            id,
            role_name,
        } => delete(cli, client, workspace, id, role_name).await,
        OnelakeSecurityCommand::GetPrincipalAccess {
            workspace,
            id,
            principal_id,
            root_path,
            expand_tables,
            page_size,
            if_match,
            if_none_match,
        } => {
            get_principal_access(
                cli,
                client,
                workspace,
                id,
                principal_id.as_deref(),
                *root_path,
                *expand_tables,
                *page_size,
                if_match.as_deref(),
                if_none_match.as_deref(),
            )
            .await
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn get_principal_access(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    principal_id: Option<&str>,
    root_path: Option<PrincipalAccessRoot>,
    expand_tables: bool,
    page_size: Option<u16>,
    if_match: Option<&str>,
    if_none_match: Option<&str>,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(item_id, "--id")?;
    if let Some(principal_id) = principal_id {
        validate_uuid(principal_id, "--principal-id")?;
    }
    validate_principal_access_options(root_path, expand_tables)?;
    validate_quoted_etag("--if-match", if_match)?;
    validate_quoted_etag("--if-none-match", if_none_match)?;

    let preview = serde_json::json!({
        "workspace": workspace,
        "itemId": item_id,
        "principalId": principal_id,
        "rootPath": root_path.map(PrincipalAccessRoot::as_str),
        "expandTables": expand_tables,
        "pageSize": page_size,
        "continuationToken": cli.continuation_token,
        "ifMatch": if_match,
        "ifNoneMatch": if_none_match,
    });
    if output::dry_run_guard(cli, "onelake-security get-principal-access", &preview) {
        return Ok(());
    }

    let mut items = Vec::new();
    let mut token = cli.continuation_token.clone();
    let mut response_etag = None;
    let mut seen_tokens = HashSet::new();
    if let Some(token) = &token {
        seen_tokens.insert(token.clone());
    }
    loop {
        let path = build_principal_access_path(
            workspace,
            item_id,
            principal_id,
            root_path,
            expand_tables,
            page_size,
            token.as_deref(),
        );
        let data = client
            .get_with_conditions_and_etag(&path, if_match, if_none_match)
            .await?;
        let etag = data.get("etag").and_then(Value::as_str);
        if cli.all && etag.is_none() {
            return Err(FabioError::api_error(
                "Effective-access aggregation requires an ETag on every page; no combined access snapshot was returned.",
            )
            .into());
        }
        if let Some(previous) = response_etag.as_deref()
            && etag != Some(previous)
        {
            return Err(FabioError::api_error(
                "Effective access changed between pages; no combined access snapshot was returned.",
            )
            .into());
        }
        if response_etag.is_none() {
            response_etag = etag.map(str::to_owned);
        }
        let Some(page_items) = data.get("value").and_then(Value::as_array) else {
            return Err(FabioError::api_error(
                "Malformed effective-access response: expected a value array.",
            )
            .into());
        };
        items.extend(page_items.iter().cloned());
        token = data
            .get("continuationToken")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        if !cli.all || token.is_none() {
            break;
        }
        if let Some(token) = &token
            && !seen_tokens.insert(token.clone())
        {
            return Err(FabioError::api_error(
                "Effective-access pagination repeated a continuation token; refusing an incomplete result.",
            )
            .into());
        }
    }

    let mut metadata = serde_json::Map::new();
    if let Some(etag) = response_etag {
        metadata.insert("etag".to_string(), Value::from(etag));
    }
    output::render_list_with_token_and_metadata(
        cli,
        &items,
        &["type", "path", "access", "effect", "rowsConstraint"],
        &["TYPE", "PATH", "ACCESS", "EFFECT", "ROW CONSTRAINT"],
        "path",
        if cli.all { None } else { token.as_deref() },
        &metadata,
    );
    Ok(())
}

fn validate_principal_access_options(
    root_path: Option<PrincipalAccessRoot>,
    expand_tables: bool,
) -> Result<()> {
    if expand_tables && root_path != Some(PrincipalAccessRoot::Tables) {
        return Err(FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            "--expand-tables is only valid when --root-path Tables is specified",
            "Use --root-path Tables with --expand-tables, or omit --expand-tables.",
            HintType::SemanticCorrection,
        )
        .into());
    }
    Ok(())
}

fn validate_quoted_etag(flag: &str, etag: Option<&str>) -> Result<()> {
    if let Some(value) = etag
        && (value.len() < 2 || !value.starts_with('"') || !value.ends_with('"'))
    {
        return Err(FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            format!("{flag} must be a quoted ETag value"),
            format!("Pass the complete ETag verbatim, including quotes: {flag} '\"<etag>\"'."),
            HintType::SemanticCorrection,
        )
        .into());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn build_principal_access_path(
    workspace: &str,
    item_id: &str,
    principal_id: Option<&str>,
    root_path: Option<PrincipalAccessRoot>,
    expand_tables: bool,
    page_size: Option<u16>,
    continuation_token: Option<&str>,
) -> String {
    let mut query = Vec::new();
    if let Some(principal_id) = principal_id {
        query.push(format!("principalId={}", urlencoding::encode(principal_id)));
    }
    if let Some(root_path) = root_path {
        query.push(format!("rootPath={}", root_path.as_str()));
    }
    if expand_tables {
        query.push("expandTables=true".to_string());
    }
    if let Some(page_size) = page_size {
        query.push(format!("pageSize={page_size}"));
    }
    if let Some(token) = continuation_token {
        query.push(format!("continuationToken={}", urlencoding::encode(token)));
    }

    let mut path = format!("/workspaces/{workspace}/items/{item_id}/principalDataAccessRecords");
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }
    path
}

async fn list(cli: &Cli, client: &FabricClient, workspace: &str, item_id: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/items/{item_id}/dataAccessRoles"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    output::render_list_with_token(
        cli,
        &resp.items,
        &["name", "decisionRules", "members"],
        &["NAME", "DECISION RULES", "MEMBERS"],
        "name",
        resp.continuation_token.as_deref(),
    );
    Ok(())
}

async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    role_name: &str,
) -> Result<()> {
    let data = client
        .get(&format!(
            "/workspaces/{workspace}/items/{item_id}/dataAccessRoles/{role_name}"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "onelake-security show", "Member"))?;

    output::render_object(cli, &data, "name");
    Ok(())
}

async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    role: &str,
    conflict_policy: &str,
) -> Result<()> {
    let role_value: Value = if let Some(path) = role.strip_prefix('@') {
        let content = std::fs::read_to_string(path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{path}': {e}"),
                "Verify the file path is correct and the file is readable.",
            )
        })?;
        serde_json::from_str(&content).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in file '{path}': {e}"),
                "The file must contain valid JSON role definition.",
            )
        })?
    } else {
        serde_json::from_str(role).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --role JSON: {e}"),
                "Provide valid JSON or use @path/to/file.json prefix to read from a file.",
            )
        })?
    };

    if output::dry_run_guard(cli, "onelake-security create", &role_value) {
        return Ok(());
    }

    let data = client
        .post(
            &format!(
                "/workspaces/{workspace}/items/{item_id}/dataAccessRoles?dataAccessRoleConflictPolicy={conflict_policy}"
            ),
            &role_value,
            false,
        )
        .await
        .map_err(|e| enrich_role_change_error(e, "onelake-security create"))?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({
            "itemId": item_id,
            "status": "role_created"
        });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "name");
    }
    Ok(())
}

async fn upsert(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    roles: &str,
) -> Result<()> {
    let roles_value: Value = if let Some(path) = roles.strip_prefix('@') {
        let content = std::fs::read_to_string(path).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Failed to read file '{path}': {e}"),
                "Verify the file path is correct and the file is readable.",
            )
        })?;
        serde_json::from_str(&content).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid JSON in file '{path}': {e}"),
                "The file must contain valid JSON roles array.",
            )
        })?
    } else {
        serde_json::from_str(roles).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid --roles JSON: {e}"),
                "Provide valid JSON or use @path/to/file.json prefix to read from a file.",
            )
        })?
    };

    let body = serde_json::json!({ "value": roles_value });

    if output::dry_run_guard(cli, "onelake-security upsert", &body) {
        return Ok(());
    }

    client
        .put(
            &format!("/workspaces/{workspace}/items/{item_id}/dataAccessRoles"),
            &body,
        )
        .await
        .map_err(|e| enrich_role_change_error(e, "onelake-security upsert"))?;

    let obj = serde_json::json!({
        "itemId": item_id,
        "status": "roles_updated"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    item_id: &str,
    role_name: &str,
) -> Result<()> {
    if output::dry_run_guard(
        cli,
        "onelake-security delete",
        &serde_json::json!({
            "workspace": workspace,
            "itemId": item_id,
            "roleName": role_name
        }),
    ) {
        return Ok(());
    }

    client
        .delete(&format!(
            "/workspaces/{workspace}/items/{item_id}/dataAccessRoles/{role_name}"
        ))
        .await
        .map_err(|e| enrich_role_change_error(e, "onelake-security delete"))?;

    let obj = serde_json::json!({
        "roleName": role_name,
        "status": "deleted"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

fn enrich_role_change_error(err: anyhow::Error, operation: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };
    if fabio_err
        .message
        .contains("DataAccessRoleChangeRestrictedByPolicy")
    {
        return fabio_err
            .with_replaced_hint(
                "The tenant's 'Manage OneLake data access roles' policy blocks this change. \
                 Ask a Fabric administrator to review that tenant policy and the caller's \
                 eligibility; retrying the same request will not bypass it.",
                Some(HintType::SemanticCorrection),
            )
            .into();
    }
    enrich_forbidden(err, operation, "Admin")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_access_path_includes_all_query_parameters() {
        let path = build_principal_access_path(
            "workspace",
            "item",
            Some("principal"),
            Some(PrincipalAccessRoot::Tables),
            true,
            Some(1000),
            Some("next token=="),
        );
        assert_eq!(
            path,
            "/workspaces/workspace/items/item/principalDataAccessRecords?principalId=principal&rootPath=Tables&expandTables=true&pageSize=1000&continuationToken=next%20token%3D%3D"
        );
    }

    #[test]
    fn principal_access_path_omits_absent_options() {
        assert_eq!(
            build_principal_access_path("workspace", "item", None, None, false, None, None),
            "/workspaces/workspace/items/item/principalDataAccessRecords"
        );
    }

    #[test]
    fn expand_tables_requires_tables_root() {
        assert!(validate_principal_access_options(None, true).is_err());
        assert!(validate_principal_access_options(Some(PrincipalAccessRoot::Files), true).is_err());
        assert!(validate_principal_access_options(Some(PrincipalAccessRoot::Tables), true).is_ok());
    }

    #[test]
    fn etag_preconditions_require_quotes() {
        assert!(validate_quoted_etag("--if-match", None).is_ok());
        assert!(validate_quoted_etag("--if-match", Some("\"etag\"")).is_ok());
        assert!(validate_quoted_etag("--if-match", Some("etag")).is_err());
        assert!(validate_quoted_etag("--if-none-match", Some("\"")).is_err());
    }

    #[test]
    fn role_policy_error_gets_tenant_policy_hint() {
        let error = FabioError::api_error(
            "DataAccessRoleChangeRestrictedByPolicy: role changes are restricted",
        );
        let enriched = enrich_role_change_error(error.into(), "onelake-security upsert");
        let enriched = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(
            enriched
                .hint
                .as_deref()
                .is_some_and(|hint| hint.contains("Manage OneLake data access roles"))
        );
    }
}
