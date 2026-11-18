use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

const COLLECTION: &str = "shares";
const PART: &str = "share.json";

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum ShareCommand {
    /// List shares in a workspace
    List {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Only list shares directly in the root folder
        #[arg(long)]
        no_recursive: bool,
        /// Filter to a specific root folder ID
        #[arg(long)]
        root_folder_id: Option<String>,
    },
    /// Show a share
    Show {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Create a share
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
        /// Folder ID (defaults to the workspace root)
        #[arg(long)]
        folder_id: Option<String>,
        /// Sensitivity label ID
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Sensitivity label application strategy
        #[arg(
            long,
            requires = "sensitivity_label",
            value_parser = ["ApplyOrFail", "Ignore"]
        )]
        sensitivity_label_apply_strategy: Option<String>,
        /// Definition file (raw share.json or a definition envelope)
        #[arg(long, conflicts_with = "definition_content")]
        definition_file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        definition_content: Option<String>,
    },
    /// Update share properties
    Update {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// New display name
        #[arg(long)]
        name: Option<String>,
        /// New description
        #[arg(long)]
        description: Option<String>,
        /// New sensitivity label ID
        #[arg(long)]
        sensitivity_label: Option<String>,
        /// Sensitivity label application strategy
        #[arg(
            long,
            requires = "sensitivity_label",
            value_parser = ["ApplyOrFail", "Ignore"]
        )]
        sensitivity_label_apply_strategy: Option<String>,
    },
    /// Delete a share
    Delete {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Get a share definition
    GetDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// Decode base64 payloads inline
        #[arg(long)]
        decode: bool,
    },
    /// Replace a share definition
    UpdateDefinition {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// Definition file (raw share.json or a definition envelope)
        #[arg(long, conflicts_with = "content")]
        file: Option<String>,
        /// Inline definition content
        #[arg(long)]
        content: Option<String>,
        /// Update item metadata from the .platform definition part
        #[arg(long)]
        update_metadata: bool,
    },
    /// Initialize a share with assets, recipients, and optional policies
    Initialize {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// List assets in a share
    ListAssets {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Add assets to a share
    BulkAddAssets {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// Remove assets from a share
    BulkRemoveAssets {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// List recipients of a share
    ListRecipients {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Add recipients to a share
    BulkAddRecipients {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// Remove recipients from a share
    BulkRemoveRecipients {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// List policies attached to a share
    ListPolicies {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
    },
    /// Add policies to a share
    BulkAddPolicies {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// Replace the mutable fields of a share policy
    UpdatePolicy {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// Share policy ID
        #[arg(long)]
        policy_id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// Remove policies from a share
    BulkRemovePolicies {
        /// Workspace ID
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,
        /// Share ID
        #[arg(long)]
        id: String,
        /// JSON request file
        #[arg(long, conflicts_with = "content", required_unless_present = "content")]
        file: Option<String>,
        /// Inline JSON request content
        #[arg(long)]
        content: Option<String>,
    },
    /// Preview a producer share before accepting it
    Preview {
        /// Producer tenant ID
        #[arg(long)]
        producer_tenant: String,
        /// Producer workspace ID
        #[arg(long)]
        producer_workspace: String,
        /// Producer share ID
        #[arg(long)]
        producer_share: String,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, command: &ShareCommand) -> Result<()> {
    match command {
        ShareCommand::List {
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
        ShareCommand::Show { workspace, id } => {
            validate_share_ids(workspace, id)?;
            crate::commands::crud::show(cli, client, COLLECTION, workspace, id).await
        }
        ShareCommand::Create {
            workspace,
            name,
            description,
            folder_id,
            sensitivity_label,
            sensitivity_label_apply_strategy,
            definition_file,
            definition_content,
        } => {
            let args = CreateArgs {
                workspace,
                name,
                description: description.as_deref(),
                folder_id: folder_id.as_deref(),
                sensitivity_label: sensitivity_label.as_deref(),
                sensitivity_label_apply_strategy: sensitivity_label_apply_strategy.as_deref(),
                definition_file: definition_file.as_deref(),
                definition_content: definition_content.as_deref(),
            };
            create(cli, client, &args).await
        }
        ShareCommand::Update {
            workspace,
            id,
            name,
            description,
            sensitivity_label,
            sensitivity_label_apply_strategy,
        } => {
            update(
                cli,
                client,
                workspace,
                id,
                name.as_deref(),
                description.as_deref(),
                sensitivity_label.as_deref(),
                sensitivity_label_apply_strategy.as_deref(),
            )
            .await
        }
        ShareCommand::Delete { workspace, id } => delete(cli, client, workspace, id).await,
        ShareCommand::GetDefinition {
            workspace,
            id,
            decode,
        } => get_definition(cli, client, workspace, id, *decode).await,
        ShareCommand::UpdateDefinition {
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
        ShareCommand::Initialize {
            workspace,
            id,
            file,
            content,
        } => {
            let request: InitializeShareRequest =
                read_json_request(file.as_deref(), content.as_deref(), "share initialize")?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share initialize",
                workspace,
                id,
                "initializeShare",
                &request,
            )
            .await
        }
        ShareCommand::ListAssets { workspace, id } => {
            list_share_collection(
                cli,
                client,
                workspace,
                id,
                "assets",
                &["displayName", "item.itemId", "itemType", "shareAs"],
                &["NAME", "ITEM ID", "ITEM TYPE", "SHARE AS"],
                "item.itemId",
            )
            .await
        }
        ShareCommand::BulkAddAssets {
            workspace,
            id,
            file,
            content,
        } => {
            let request: AddShareAssetsRequest =
                read_json_request(file.as_deref(), content.as_deref(), "share bulk-add-assets")?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-add-assets",
                workspace,
                id,
                "assets/bulkAdd",
                &request,
            )
            .await
        }
        ShareCommand::BulkRemoveAssets {
            workspace,
            id,
            file,
            content,
        } => {
            let request: RemoveShareAssetsRequest = read_json_request(
                file.as_deref(),
                content.as_deref(),
                "share bulk-remove-assets",
            )?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-remove-assets",
                workspace,
                id,
                "assets/bulkRemove",
                &request,
            )
            .await
        }
        ShareCommand::ListRecipients { workspace, id } => {
            list_share_collection(
                cli,
                client,
                workspace,
                id,
                "recipients",
                &[
                    "recipient.type",
                    "recipient.userPrincipalName",
                    "recipient.principalId",
                    "invitationAcceptanceStatus",
                ],
                &["TYPE", "USER", "PRINCIPAL ID", "STATUS"],
                "recipient.userPrincipalName",
            )
            .await
        }
        ShareCommand::BulkAddRecipients {
            workspace,
            id,
            file,
            content,
        } => {
            let request: ShareRecipientsRequest = read_json_request(
                file.as_deref(),
                content.as_deref(),
                "share bulk-add-recipients",
            )?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-add-recipients",
                workspace,
                id,
                "recipients/bulkAdd",
                &request,
            )
            .await
        }
        ShareCommand::BulkRemoveRecipients {
            workspace,
            id,
            file,
            content,
        } => {
            let request: ShareRecipientsRequest = read_json_request(
                file.as_deref(),
                content.as_deref(),
                "share bulk-remove-recipients",
            )?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-remove-recipients",
                workspace,
                id,
                "recipients/bulkRemove",
                &request,
            )
            .await
        }
        ShareCommand::ListPolicies { workspace, id } => {
            list_share_collection(
                cli,
                client,
                workspace,
                id,
                "sharePolicies",
                &["id", "type", "properties.expirationDateTime"],
                &["ID", "TYPE", "EXPIRATION"],
                "id",
            )
            .await
        }
        ShareCommand::BulkAddPolicies {
            workspace,
            id,
            file,
            content,
        } => {
            let request: AddSharePoliciesRequest = read_json_request(
                file.as_deref(),
                content.as_deref(),
                "share bulk-add-policies",
            )?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-add-policies",
                workspace,
                id,
                "sharePolicies/bulkAdd",
                &request,
            )
            .await
        }
        ShareCommand::UpdatePolicy {
            workspace,
            id,
            policy_id,
            file,
            content,
        } => {
            let request: UpdateSharePolicyRequest =
                read_json_request(file.as_deref(), content.as_deref(), "share update-policy")?;
            validate_uuid(policy_id, "--policy-id")?;
            if request.id() != policy_id {
                return Err(FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "The policy ID in the request must match --policy-id",
                    "Set the request id field to the same UUID passed with --policy-id.",
                )
                .into());
            }
            mutate_share(
                cli,
                client,
                "share update-policy",
                workspace,
                id,
                &format!("sharePolicies/{policy_id}/update"),
                &request,
            )
            .await
        }
        ShareCommand::BulkRemovePolicies {
            workspace,
            id,
            file,
            content,
        } => {
            let request: RemoveSharePoliciesRequest = read_json_request(
                file.as_deref(),
                content.as_deref(),
                "share bulk-remove-policies",
            )?;
            request.validate()?;
            mutate_share(
                cli,
                client,
                "share bulk-remove-policies",
                workspace,
                id,
                "sharePolicies/bulkRemove",
                &request,
            )
            .await
        }
        ShareCommand::Preview {
            producer_tenant,
            producer_workspace,
            producer_share,
        } => {
            preview(
                cli,
                client,
                producer_tenant,
                producer_workspace,
                producer_share,
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
    crate::commands::crud::list_with_options(
        cli,
        client,
        COLLECTION,
        workspace,
        no_recursive,
        root_folder_id,
        &["displayName", "id", "description", "properties.shareStatus"],
        &["NAME", "ID", "DESCRIPTION", "STATUS"],
    )
    .await
}

struct CreateArgs<'a> {
    workspace: &'a str,
    name: &'a str,
    description: Option<&'a str>,
    folder_id: Option<&'a str>,
    sensitivity_label: Option<&'a str>,
    sensitivity_label_apply_strategy: Option<&'a str>,
    definition_file: Option<&'a str>,
    definition_content: Option<&'a str>,
}

fn read_definition(file: Option<&str>, content: Option<&str>) -> Result<Option<Value>> {
    let raw = match (file, content) {
        (Some(path), _) => Some(
            std::fs::read_to_string(path)
                .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        ),
        (_, Some(value)) => Some(value.to_string()),
        (None, None) => None,
    };
    Ok(raw.map(|value| {
        crate::definition_spec::build_update_definition_body(&value, PART)["definition"].clone()
    }))
}

fn build_create_body(args: &CreateArgs<'_>, definition: Option<Value>) -> Value {
    let mut body = json!({ "displayName": args.name });
    if let Some(value) = args.description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = args.folder_id {
        body["folderId"] = Value::from(value);
    }
    if let Some(value) = args.sensitivity_label {
        let mut settings = json!({ "labelId": value });
        if let Some(strategy) = args.sensitivity_label_apply_strategy {
            settings["sensitivityLabelApplyStrategy"] = Value::from(strategy);
        }
        body["sensitivityLabelSettings"] = settings;
    }
    if let Some(value) = definition {
        body["definition"] = value;
    }
    body
}

async fn create(cli: &Cli, client: &FabricClient, args: &CreateArgs<'_>) -> Result<()> {
    validate_uuid(args.workspace, "--workspace")?;
    if let Some(folder) = args.folder_id {
        validate_uuid(folder, "--folder-id")?;
    }
    let body = build_create_body(
        args,
        read_definition(args.definition_file, args.definition_content)?,
    );
    if output::dry_run_guard(
        cli,
        "share create",
        &json!({ "workspace": args.workspace, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{}/{COLLECTION}", args.workspace),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, "share create", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
    sensitivity_label: Option<&str>,
    sensitivity_label_apply_strategy: Option<&str>,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
    if name.is_none() && description.is_none() && sensitivity_label.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "At least one update field must be provided",
            "Provide --name, --description, or --sensitivity-label.",
        )
        .into());
    }
    let mut body = json!({});
    if let Some(value) = name {
        body["displayName"] = Value::from(value);
    }
    if let Some(value) = description {
        body["description"] = Value::from(value);
    }
    if let Some(value) = sensitivity_label {
        let mut settings = json!({ "labelId": value });
        if let Some(strategy) = sensitivity_label_apply_strategy {
            settings["sensitivityLabelApplyStrategy"] = Value::from(strategy);
        }
        body["sensitivityLabelSettings"] = settings;
    }
    if output::dry_run_guard(
        cli,
        "share update",
        &json!({ "workspace": workspace, "id": id, "request": body }),
    ) {
        return Ok(());
    }
    let data = client
        .patch(&format!("/workspaces/{workspace}/{COLLECTION}/{id}"), &body)
        .await
        .map_err(|e| enrich_forbidden(e, "share update", "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn delete(cli: &Cli, client: &FabricClient, workspace: &str, id: &str) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
    crate::commands::crud::delete(
        cli,
        client,
        "share",
        COLLECTION,
        "Contributor",
        workspace,
        id,
        false,
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
        "share",
        COLLECTION,
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
    update_metadata: bool,
) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")?;
    crate::commands::crud::update_definition_with_options(
        cli,
        client,
        "share",
        COLLECTION,
        "Contributor",
        PART,
        workspace,
        id,
        file,
        content,
        None,
        update_metadata,
    )
    .await
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ItemReferenceById {
    reference_type: ItemReferenceType,
    item_id: String,
    workspace_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
enum ItemReferenceType {
    ById,
}

#[derive(Debug, Deserialize, Serialize)]
enum ShareAsType {
    MirroredOneLake,
    #[serde(rename = "Self")]
    Native,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShareAssetForCreation {
    item: ItemReferenceById,
    #[serde(skip_serializing_if = "Option::is_none")]
    share_as: Option<ShareAsType>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum ShareRecipientIdentity {
    User {
        #[serde(rename = "userPrincipalName", skip_serializing_if = "Option::is_none")]
        user_principal_name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        email: Option<String>,
        #[serde(rename = "tenantId", skip_serializing_if = "Option::is_none")]
        tenant_id: Option<String>,
    },
    ServicePrincipal {
        #[serde(rename = "principalId")]
        principal_id: String,
        #[serde(rename = "tenantId")]
        tenant_id: String,
    },
}

impl ShareRecipientIdentity {
    fn validate(&self) -> Result<()> {
        match self {
            Self::User {
                user_principal_name,
                email,
                tenant_id,
            } => {
                if user_principal_name.is_some() == email.is_some() {
                    return Err(FabioError::with_hint(
                        ErrorCode::InvalidInput,
                        "A User recipient requires exactly one of userPrincipalName or email",
                        "Provide one identity field and omit the other.",
                    )
                    .into());
                }
                if let Some(value) = tenant_id {
                    validate_uuid(value, "recipient.tenantId")?;
                }
            }
            Self::ServicePrincipal {
                principal_id,
                tenant_id,
            } => {
                validate_uuid(principal_id, "recipient.principalId")?;
                validate_uuid(tenant_id, "recipient.tenantId")?;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum SharePolicyForCreation {
    ShareExpiration {
        properties: ShareExpirationPolicyProperties,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ShareExpirationPolicyProperties {
    expiration_date_time: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct InitializeShareRequest {
    assets: Vec<ShareAssetForCreation>,
    recipients: Vec<ShareRecipientIdentity>,
    #[serde(skip_serializing_if = "Option::is_none")]
    share_policies: Option<Vec<SharePolicyForCreation>>,
}

impl InitializeShareRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.assets, "assets")?;
        validate_assets(&self.assets)?;
        validate_nonempty(&self.recipients, "recipients")?;
        validate_recipients(&self.recipients)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct AddShareAssetsRequest {
    assets: Vec<ShareAssetForCreation>,
}

impl AddShareAssetsRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.assets, "assets")?;
        validate_assets(&self.assets)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RemoveShareAssetsRequest {
    items: Vec<ItemReferenceById>,
}

impl RemoveShareAssetsRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.items, "items")?;
        for item in &self.items {
            validate_item_reference(item)?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ShareRecipientsRequest {
    recipients: Vec<ShareRecipientIdentity>,
}

impl ShareRecipientsRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.recipients, "recipients")?;
        validate_recipients(&self.recipients)
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddSharePoliciesRequest {
    share_policies: Vec<SharePolicyForCreation>,
}

impl AddSharePoliciesRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.share_policies, "sharePolicies")
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum UpdateSharePolicyRequest {
    ShareExpiration {
        id: String,
        properties: ShareExpirationPolicyProperties,
    },
}

impl UpdateSharePolicyRequest {
    fn id(&self) -> &str {
        match self {
            Self::ShareExpiration { id, .. } => id,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RemoveSharePoliciesRequest {
    share_policy_ids: Vec<String>,
}

impl RemoveSharePoliciesRequest {
    fn validate(&self) -> Result<()> {
        validate_nonempty(&self.share_policy_ids, "sharePolicyIds")?;
        for id in &self.share_policy_ids {
            validate_uuid(id, "sharePolicyIds[]")?;
        }
        Ok(())
    }
}

fn validate_nonempty<T>(values: &[T], field: &str) -> Result<()> {
    if values.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{field} must contain at least one entry"),
            format!("Add at least one object to the {field} array."),
        )
        .into());
    }
    Ok(())
}

fn validate_item_reference(item: &ItemReferenceById) -> Result<()> {
    validate_uuid(&item.item_id, "item.itemId")?;
    validate_uuid(&item.workspace_id, "item.workspaceId")
}

fn validate_assets(assets: &[ShareAssetForCreation]) -> Result<()> {
    for asset in assets {
        validate_item_reference(&asset.item)?;
    }
    Ok(())
}

fn validate_recipients(recipients: &[ShareRecipientIdentity]) -> Result<()> {
    for recipient in recipients {
        recipient.validate()?;
    }
    Ok(())
}

fn validate_share_ids(workspace: &str, id: &str) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(id, "--id")
}

fn read_json_request<T: DeserializeOwned>(
    file: Option<&str>,
    content: Option<&str>,
    operation: &str,
) -> Result<T> {
    let raw = match (file, content) {
        (Some(path), None) => std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("Failed to read file '{path}': {e}"))?,
        (None, Some(value)) => value.to_string(),
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("{operation} requires exactly one JSON input"),
                "Provide either --file <path> or --content '<json>'.",
            )
            .into());
        }
    };
    serde_json::from_str(&raw).map_err(|e| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid JSON request for {operation}: {e}"),
            "Match the request schema in the Fabric Share REST API specification.",
        )
        .into()
    })
}

#[allow(clippy::too_many_arguments)]
async fn list_share_collection(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    resource: &str,
    columns: &[&str],
    headers: &[&str],
    plain_key: &str,
) -> Result<()> {
    validate_share_ids(workspace, id)?;
    let response = client
        .get_list(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/{resource}?beta=true"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|e| enrich_forbidden(e, "share list content", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        columns,
        headers,
        plain_key,
        response.continuation_token.as_deref(),
    );
    Ok(())
}

async fn mutate_share<T: Serialize + Sync>(
    cli: &Cli,
    client: &FabricClient,
    operation: &str,
    workspace: &str,
    id: &str,
    resource: &str,
    request: &T,
) -> Result<()> {
    validate_share_ids(workspace, id)?;
    let body = serde_json::to_value(request)?;
    let destructive = matches!(
        operation,
        "share bulk-remove-assets"
            | "share bulk-remove-recipients"
            | "share update-policy"
            | "share bulk-remove-policies"
    );
    if output::dry_run_guard_maybe_destructive(
        cli,
        operation,
        &json!({ "workspace": workspace, "id": id, "request": body }),
        destructive,
    ) {
        return Ok(());
    }
    let data = client
        .post(
            &format!("/workspaces/{workspace}/{COLLECTION}/{id}/{resource}?beta=true"),
            &body,
            true,
        )
        .await
        .map_err(|e| enrich_forbidden(e, operation, "Contributor"))?;
    output::render_object(cli, &data, "id");
    Ok(())
}

async fn preview(
    cli: &Cli,
    client: &FabricClient,
    producer_tenant: &str,
    producer_workspace: &str,
    producer_share: &str,
) -> Result<()> {
    validate_uuid(producer_tenant, "--producer-tenant")?;
    validate_uuid(producer_workspace, "--producer-workspace")?;
    validate_uuid(producer_share, "--producer-share")?;
    let data = client
        .get(&format!(
            "/tenants/{producer_tenant}/workspaces/{producer_workspace}/shares/{producer_share}/preview?beta=true"
        ))
        .await
        .map_err(|e| enrich_forbidden(e, "share preview", "Recipient"))?;
    output::render_object(cli, &data, "shareId");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_body_serializes_all_optional_fields() {
        let args = CreateArgs {
            workspace: "workspace",
            name: "Share",
            description: Some("Description"),
            folder_id: Some("folder"),
            sensitivity_label: Some("label"),
            sensitivity_label_apply_strategy: Some("ApplyOrFail"),
            definition_file: None,
            definition_content: None,
        };
        let definition = json!({ "parts": [{ "path": PART, "payload": "e30=", "payloadType": "InlineBase64" }] });
        let body = build_create_body(&args, Some(definition));
        assert_eq!(body["folderId"], "folder");
        assert_eq!(body["sensitivityLabelSettings"]["labelId"], "label");
        assert_eq!(
            body["sensitivityLabelSettings"]["sensitivityLabelApplyStrategy"],
            "ApplyOrFail"
        );
        assert_eq!(body["definition"]["parts"][0]["path"], PART);
    }

    #[test]
    fn list_path_serializes_folder_query_parameters() {
        assert_eq!(
            crate::commands::crud::item_list_path(
                COLLECTION,
                "ws",
                true,
                Some("00000000-0000-0000-0000-000000000001")
            )
            .unwrap(),
            "/workspaces/ws/shares?recursive=false&rootFolderId=00000000-0000-0000-0000-000000000001"
        );
    }

    #[test]
    fn initialize_request_serializes_discriminated_unions() {
        let request = InitializeShareRequest {
            assets: vec![ShareAssetForCreation {
                item: ItemReferenceById {
                    reference_type: ItemReferenceType::ById,
                    item_id: "00000000-0000-0000-0000-000000000002".to_string(),
                    workspace_id: "00000000-0000-0000-0000-000000000001".to_string(),
                },
                share_as: Some(ShareAsType::MirroredOneLake),
            }],
            recipients: vec![
                ShareRecipientIdentity::User {
                    user_principal_name: Some("user@example.com".to_string()),
                    email: None,
                    tenant_id: Some("00000000-0000-0000-0000-000000000003".to_string()),
                },
                ShareRecipientIdentity::ServicePrincipal {
                    principal_id: "00000000-0000-0000-0000-000000000004".to_string(),
                    tenant_id: "00000000-0000-0000-0000-000000000003".to_string(),
                },
            ],
            share_policies: Some(vec![SharePolicyForCreation::ShareExpiration {
                properties: ShareExpirationPolicyProperties {
                    expiration_date_time: "2027-03-31T23:59:59Z".to_string(),
                },
            }]),
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "assets": [{
                    "item": {
                        "referenceType": "ById",
                        "itemId": "00000000-0000-0000-0000-000000000002",
                        "workspaceId": "00000000-0000-0000-0000-000000000001"
                    },
                    "shareAs": "MirroredOneLake"
                }],
                "recipients": [
                    {
                        "type": "User",
                        "userPrincipalName": "user@example.com",
                        "tenantId": "00000000-0000-0000-0000-000000000003"
                    },
                    {
                        "type": "ServicePrincipal",
                        "principalId": "00000000-0000-0000-0000-000000000004",
                        "tenantId": "00000000-0000-0000-0000-000000000003"
                    }
                ],
                "sharePolicies": [{
                    "type": "ShareExpiration",
                    "properties": {
                        "expirationDateTime": "2027-03-31T23:59:59Z"
                    }
                }]
            })
        );
    }

    #[test]
    fn update_policy_request_serializes_id_type_and_properties() {
        let request = UpdateSharePolicyRequest::ShareExpiration {
            id: "00000000-0000-0000-0000-000000000005".to_string(),
            properties: ShareExpirationPolicyProperties {
                expiration_date_time: "2027-03-31T23:59:59Z".to_string(),
            },
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "type": "ShareExpiration",
                "id": "00000000-0000-0000-0000-000000000005",
                "properties": {
                    "expirationDateTime": "2027-03-31T23:59:59Z"
                }
            })
        );
    }

    #[test]
    fn user_recipient_rejects_both_identity_fields() {
        let recipient = ShareRecipientIdentity::User {
            user_principal_name: Some("user@example.com".to_string()),
            email: Some("other@example.com".to_string()),
            tenant_id: None,
        };
        assert!(recipient.validate().is_err());
    }
}
