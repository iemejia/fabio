use std::fmt;

use thiserror::Error;

/// Machine-readable error codes for structured error output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    AuthRequired,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    CapacityInactive,
    InvalidInput,
    ApiError,
    Timeout,
    NetworkError,
    ReadonlyMode,
    Unknown,
}

impl ErrorCode {
    /// Returns the machine-readable string representation of the error code.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthRequired => "AUTH_REQUIRED",
            Self::Forbidden => "FORBIDDEN",
            Self::NotFound => "NOT_FOUND",
            Self::Conflict => "CONFLICT",
            Self::RateLimited => "RATE_LIMITED",
            Self::CapacityInactive => "CAPACITY_INACTIVE",
            Self::InvalidInput => "INVALID_INPUT",
            Self::ApiError => "API_ERROR",
            Self::Timeout => "TIMEOUT",
            Self::NetworkError => "NETWORK_ERROR",
            Self::ReadonlyMode => "READONLY_MODE",
            Self::Unknown => "UNKNOWN",
        }
    }

    /// Returns the stable process exit code for this error.
    ///
    /// These are stable and documented — agents can branch on `$?` without
    /// parsing JSON. New codes may be added but existing ones never change.
    ///
    /// | Code | Name | Meaning |
    /// |------|------|---------|
    /// | 0 | ok | Success |
    /// | 1 | error | Generic or unclassified failure |
    /// | 2 | usage | Invalid command syntax (clap) |
    /// | 3 | auth_required | Not authenticated |
    /// | 4 | forbidden | Permission denied or command blocked |
    /// | 5 | not_found | Resource does not exist |
    /// | 6 | conflict | Resource already exists |
    /// | 7 | rate_limited | API quota or rate limit reached |
    /// | 8 | timeout | Operation timed out |
    /// | 9 | network | Network connectivity failure |
    /// | 10 | readonly | Mutation blocked by --readonly |
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::AuthRequired => 3,
            Self::Forbidden | Self::ReadonlyMode => 4,
            Self::NotFound => 5,
            Self::Conflict => 6,
            Self::RateLimited | Self::CapacityInactive => 7,
            Self::Timeout => 8,
            Self::NetworkError => 9,
            Self::InvalidInput | Self::ApiError | Self::Unknown => 1,
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detail entry from the API's `error.moreDetails` array.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ErrorDetail {
    #[serde(rename = "errorCode")]
    pub error_code: String,
    pub message: String,
}

/// Related resource information from the API's `error.relatedResource` object.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RelatedResource {
    #[serde(rename = "resourceId")]
    pub resource_id: String,
    #[serde(rename = "resourceType")]
    pub resource_type: String,
}

/// Classification of a hint's semantic impact on the operation.
///
/// AI agents use this to decide whether a hint-driven retry is safe to execute
/// automatically or requires user confirmation/post-action verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HintType {
    /// Auth/connectivity/infra fix — no semantic change to the operation.
    /// Safe to auto-retry after applying the fix.
    AuthFix,
    /// The hint suggests retrying the same command as-is (transient failure).
    /// Idempotent and safe.
    RetrySafe,
    /// The hint corrects syntax or casing but preserves the user's original intent.
    /// E.g., `"Overwrite"` instead of `"overwrite"` — same meaning, just `PascalCase`.
    SyntaxFix,
    /// The hint corrects the command in a way that CHANGES the operation's meaning.
    /// E.g., different mode, different scope, different target. Agent should verify
    /// with the user that the semantic change is intentional, or run the `verify_after`
    /// command to confirm the result matches expectations.
    SemanticCorrection,
    /// The hint suggests a safety-bypass flag (e.g., `--force`, `--hard-delete`).
    /// Already triggers `agentNotice`. Agent must NOT retry without explicit user approval.
    SafetyBypass,
}

/// Structured error type for the fabio CLI.
#[derive(Debug, Error)]
#[error("{code}: {message}")]
pub struct FabioError {
    pub code: ErrorCode,
    pub message: String,
    /// Optional hint with valid values or corrected command for agent self-correction.
    pub hint: Option<String>,
    /// Classification of the hint's semantic impact. When `None`, the output layer
    /// infers the type from error code and hint content (conservative default:
    /// `SemanticCorrection`). Explicit classification is preferred for new code.
    pub hint_type: Option<HintType>,
    /// Optional verification command the agent should run after a successful retry
    /// to confirm the result matches the user's intent. Most valuable when
    /// `hint_type == SemanticCorrection`.
    pub verify_after: Option<String>,
    /// Whether the API indicated this error is retriable (from `error.isRetriable` in response).
    pub retriable: Option<bool>,
    /// Server-assigned request ID for support correlation (from `error.requestId`).
    pub request_id: Option<String>,
    /// Additional error details from the API (from `error.moreDetails`).
    pub more_details: Option<Vec<ErrorDetail>>,
    /// The resource involved in the error (from `error.relatedResource`).
    pub related_resource: Option<RelatedResource>,
}

impl FabioError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: None,
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        }
    }

    /// Create an error with a hint for agent self-correction.
    ///
    /// The `hint_type` is left as `None` and will be inferred at render time
    /// from the error code and hint content. Prefer `with_typed_hint()` for
    /// new code where the classification is known.
    pub fn with_hint(code: ErrorCode, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            hint: Some(hint.into()),
            hint_type: None,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        }
    }

    /// Create an error with an explicitly classified hint.
    ///
    /// Use this for new code where the hint's semantic impact is known at the call site.
    pub fn with_typed_hint(
        code: ErrorCode,
        message: impl Into<String>,
        hint: impl Into<String>,
        hint_type: HintType,
    ) -> Self {
        Self {
            code,
            message: message.into(),
            hint: Some(hint.into()),
            hint_type: Some(hint_type),
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        }
    }

    /// Set a verification command the agent should run after a successful retry (builder).
    ///
    /// Most valuable when `hint_type == SemanticCorrection`. The command should
    /// be a read-only fabio invocation that confirms the result matches intent.
    #[must_use]
    pub fn set_verify_after(mut self, cmd: impl Into<String>) -> Self {
        self.verify_after = Some(cmd.into());
        self
    }

    #[inline]
    pub fn auth_required(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::AuthRequired, message)
    }

    #[inline]
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, message)
    }

    #[inline]
    pub fn api_error(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::ApiError, message)
    }

    #[inline]
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message)
    }

    /// Set the retriable flag (builder pattern).
    #[must_use]
    pub const fn set_retriable(mut self, retriable: Option<bool>) -> Self {
        self.retriable = retriable;
        self
    }

    /// Set the request ID from the API response (builder pattern).
    #[must_use]
    pub fn set_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    /// Set additional error details from the API response (builder pattern).
    #[must_use]
    pub fn set_more_details(mut self, more_details: Option<Vec<ErrorDetail>>) -> Self {
        self.more_details = more_details;
        self
    }

    /// Set related resource info from the API response (builder pattern).
    #[must_use]
    pub fn set_related_resource(mut self, related_resource: Option<RelatedResource>) -> Self {
        self.related_resource = related_resource;
        self
    }
}

/// Convert HTTP status codes to appropriate error codes.
impl FabioError {
    pub fn from_status(status: u16, message: impl Into<String>) -> Self {
        Self::from_status_with_body(status, message, "")
    }

    /// Create an error from HTTP status with the full response body for context-aware hints.
    pub fn from_status_with_body(status: u16, message: impl Into<String>, body: &str) -> Self {
        let msg = message.into();
        let code = match status {
            401 => ErrorCode::AuthRequired,
            403 => ErrorCode::Forbidden,
            404 => ErrorCode::NotFound,
            409 | 412 => ErrorCode::Conflict,
            429 | 430 => ErrorCode::RateLimited,
            _ if (500..600).contains(&status) => ErrorCode::ApiError,
            _ => ErrorCode::ApiError,
        };
        let hint = match code {
            ErrorCode::AuthRequired => Some(auth_required_hint(&msg, body)),
            ErrorCode::Forbidden => Some(forbidden_hint(&msg, body)),
            ErrorCode::Conflict => Some(if status == 412 {
                // 412 Precondition Failed: always use the ETag-specific hint regardless of body
                // content — an empty or unrecognized body must not fall through to the generic
                // "Resource conflict (409)" message.
                "ETag precondition failed. Re-fetch the resource using the corresponding get \
                 command, then retry with --if-match using the returned etag value. Pass the \
                 full value verbatim, including any surrounding double-quotes \
                 (bash: --if-match '\"a1b2c3d4\"'; PowerShell: --if-match \"`\"a1b2c3d4`\"\")."
                    .to_string()
            } else {
                conflict_hint(&msg, body)
            }),
            ErrorCode::RateLimited => {
                Some("Too many requests. Retry after a short backoff.".to_string())
            }
            _ => None,
        };
        // Infer hint_type for HTTP-status-derived hints (known at construction time).
        let hint_type = hint.as_ref().map(|_| match code {
            ErrorCode::AuthRequired => HintType::AuthFix,
            ErrorCode::RateLimited => HintType::RetrySafe,
            // Forbidden and Conflict hints suggest investigating/fixing permissions
            // or resolving naming conflicts — semantic corrections by nature.
            _ => HintType::SemanticCorrection,
        });
        Self {
            code,
            message: msg,
            hint,
            hint_type,
            verify_after: None,
            retriable: None,
            request_id: None,
            more_details: None,
            related_resource: None,
        }
    }
}

/// Enrich a `FabioError` with an operation-specific permission hint.
///
/// If the error is `Forbidden`, replaces the generic hint with one tailored
/// to the operation (e.g., "item create requires Member role"). For non-Forbidden
/// errors, returns the original error unchanged.
pub fn enrich_forbidden(err: anyhow::Error, operation: &str, required_role: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    if fabio_err.code != ErrorCode::Forbidden {
        return err;
    }

    let hint = format!(
        "'{operation}' requires at least '{required_role}' role on the workspace. \
         Workspace roles: Admin > Member > Contributor > Viewer. \
         Check your access with: fabio workspace show --id <workspace-id>. \
         Ask a workspace Admin to grant you the required role."
    );

    FabioError::with_hint(ErrorCode::Forbidden, fabio_err.message.clone(), hint).into()
}

/// Enrich a failed ontology definition push (`import`, `bind`,
/// `update-definition`) with a diagnostic hint.
///
/// Fabric validates an ontology's parts together at `updateDefinition` time and,
/// on failure, returns a generic `ALMOperationImportFailed` with little context.
/// Live testing established the recurring, non-obvious causes — this maps the
/// error to an actionable checklist so agents can self-correct without a portal
/// round-trip. Only applies to `ApiError`/`InvalidInput`; other codes (`Forbidden`,
/// `NotFound`, ...) pass through so their own enrichment wins.
pub fn enrich_ontology_definition_error(err: anyhow::Error, operation: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    let msg_lower = fabio_err.message.to_lowercase();
    let is_import_failure = msg_lower.contains("almoperationimportfailed")
        || msg_lower.contains("import failed")
        || (msg_lower.contains("invalid definition") && msg_lower.contains("ontolog"));

    // Only enrich generic server-side validation failures; leave permission,
    // conflict, and not-found errors to their dedicated enrichers.
    if !is_import_failure
        || !matches!(
            fabio_err.code,
            ErrorCode::ApiError | ErrorCode::InvalidInput
        )
    {
        return err;
    }

    let hint = format!(
        "'{operation}' was rejected by Fabric's ontology validator (ALMOperationImportFailed \
         is generic — the real cause is usually one of these, in order of likelihood): \
         (1) A DataBinding's propertyBindings targets an UNTYPED property (valueType 'Any'). \
         Untyped properties live only under entityType.untypedProperties and must NEVER appear \
         in a binding — remove them from propertyBindings. \
         (2) A DataBinding or Contextualization references an entityTypeId / propertyId / \
         relationshipTypeId that is not defined by any part in the same push. IDs are \
         case-sensitive and must match exactly. \
         (3) A TimeSeries DataBinding is missing its timestampColumn, or a property marked \
         timeSeries has no corresponding timeseries binding. \
         (4) Malformed part: wrong $schema, missing required field, or a Contextualization \
         whose sourceEntityTypeId/targetEntityTypeId do not match the relationship's endpoints. \
         Note: Fabric does NOT validate that the referenced Lakehouse table/columns exist at \
         this stage (that is deferred to query time), so a missing table is NOT the cause here. \
         Inspect the current stored definition with: \
         fabio ontology get-definition --workspace <WS> --id <ID> --decode"
    );

    FabioError::with_typed_hint(
        fabio_err.code,
        fabio_err.message.clone(),
        hint,
        HintType::SemanticCorrection,
    )
    .into()
}

/// Enrich errors from admin commands with tenant-level hints.
///
/// Unlike `enrich_forbidden` (workspace-scoped), admin commands require
/// tenant-level Fabric Admin role. This function also detects specific
/// admin error patterns and provides actionable guidance.
pub fn enrich_admin(err: anyhow::Error, operation: &str) -> anyhow::Error {
    let Some(fabio_err) = err.downcast_ref::<FabioError>() else {
        return err;
    };

    let msg_lower = fabio_err.message.to_lowercase();

    // Detect specific admin error patterns and provide targeted hints
    // NOTE: More specific checks must come BEFORE generic ones (e.g., "external data sharing"
    // before generic "tenant setting disabled" since the former contains both patterns).

    if msg_lower.contains("external data sharing") && msg_lower.contains("disabled") {
        let hint = format!(
            "'{operation}' requires the 'External data sharing' tenant setting to be enabled. \
             Enable it with: fabio admin update-tenant-setting \
             --setting-name AllowExternalDataSharingSwitch --content '{{\"enabled\":true}}'"
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    if msg_lower.contains("tenant setting") && msg_lower.contains("disabled") {
        let hint = format!(
            "'{operation}' failed because a required tenant setting is disabled. \
             Enable it in the Fabric Admin Portal > Tenant Settings, or use: \
             fabio admin update-tenant-setting --setting-name <SETTING> --content '{{\"enabled\":true}}'"
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    if msg_lower.contains("not supported for the requested item type") {
        let hint = format!(
            "'{operation}' only supports specific item types. \
             For bulk-remove-sharing-links, only 'Report' type is supported. \
             Change the 'type' field in your request body to 'Report'."
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    if msg_lower.contains("label is not assigned to user") || msg_lower.contains("label not found")
    {
        let hint = format!(
            "'{operation}' requires Microsoft Purview sensitivity labels configured in the tenant. \
             Prerequisites: (1) M365 E5 or equivalent licensing, \
             (2) Purview sensitivity labels published via label policy, \
             (3) Labels enabled for Fabric in the Admin Portal. \
             Verify label IDs with your compliance administrator."
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    if msg_lower.contains("feature is not available") || msg_lower.contains("featurenotavailable") {
        let hint = format!(
            "'{operation}' requires a feature that is not enabled in this tenant. \
             This is typically controlled by a tenant admin setting. \
             Check available settings with: fabio admin list-tenant-settings. \
             Contact your Fabric administrator to enable the required feature."
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    if msg_lower.contains("syncing admins to subdomains is not supported") {
        let hint = format!(
            "'{operation}' only supports syncing the 'Contributor' role to subdomains. \
             Admin role sync is not supported by the API. \
             Use --role Contributor (default) instead of --role Admin."
        );
        return FabioError::with_hint(fabio_err.code, fabio_err.message.clone(), hint).into();
    }

    // For Forbidden errors, provide tenant-admin-level guidance
    if fabio_err.code == ErrorCode::Forbidden {
        let hint = if msg_lower.contains("sufficient scopes") {
            format!(
                "'{operation}' requires the Tenant.Read.All or Tenant.ReadWrite.All delegated scope. \
                 Ensure the authenticated identity has Fabric Admin role assigned in the \
                 Microsoft 365 Admin Center > Roles > Fabric Administrator. \
                 Re-authenticate with: fabio auth login"
            )
        } else {
            format!(
                "'{operation}' requires tenant-level Fabric Administrator role. \
                 This is NOT a workspace role — it must be assigned in the Microsoft 365 \
                 Admin Center > Roles > Fabric Administrator (or Power BI Administrator). \
                 Verify with: fabio admin list-workspaces (if this also fails, you lack admin access). \
                 Re-authenticate with: fabio auth login"
            )
        };
        return FabioError::with_hint(ErrorCode::Forbidden, fabio_err.message.clone(), hint).into();
    }

    err
}

/// Generate a context-aware hint for 401 Unauthorized errors.
///
/// Fabric returns `401` in two very different situations that require different fixes:
///
/// 1. **The token is missing/expired/invalid** — re-authentication fixes it.
/// 2. **The token is valid, but the caller is not permitted to perform this specific
///    operation** — Fabric replies `401` with the message
///    `"The caller is not authenticated to access this resource"`. This is the
///    signature response when a *service principal* is allowed to use Fabric APIs
///    (reads succeed) but a separate tenant setting gates the mutation. The most
///    common case is workspace/connection/deployment-pipeline creation, which
///    requires the **"Service principals can create workspaces, connections, and
///    deployment pipelines"** tenant setting to be enabled with the SP in the
///    allowed security group. Re-running `fabio auth login` will NOT fix this.
fn auth_required_hint(message: &str, body: &str) -> String {
    let combined = format!("{} {}", message.to_lowercase(), body.to_lowercase());

    // "The caller is not authenticated to access this resource" is Fabric's response
    // when the token is valid but the identity is not permitted for this operation.
    // A genuinely expired/invalid token produces a token-validation error instead.
    if combined.contains("caller is not authenticated") {
        return "The token is valid but the caller is not permitted to perform this \
                operation. If you authenticated with a service principal, creating \
                workspaces, connections, or deployment pipelines requires the tenant \
                setting 'Service principals can create workspaces, connections, and \
                deployment pipelines' to be enabled in the Fabric Admin Portal > \
                Tenant settings > Developer settings, with the service principal (or a \
                security group containing it) added to the allowed list. This is a \
                separate setting from 'Service principals can use Fabric APIs' (which \
                only enables read access). If you are using a user account, re-running \
                'fabio auth login' will NOT help — the account lacks permission for this \
                action. Otherwise, if the token has expired, re-authenticate with: \
                fabio auth login"
            .to_string();
    }

    "Not authenticated. Your token may be missing or expired. \
     Re-authenticate with: fabio auth login (or 'fabio auth login --service-principal' \
     for non-interactive auth)."
        .to_string()
}

/// Generate a context-aware hint for 403 Forbidden errors based on the error message and body.
fn forbidden_hint(message: &str, body: &str) -> String {
    let msg_lower = message.to_lowercase();
    let body_lower = body.to_lowercase();
    let combined = format!("{msg_lower} {body_lower}");

    // Detect admin/tenant-level permission issues (check first — most specific)
    if combined.contains("sufficient scopes")
        || combined.contains("tenant.read")
        || combined.contains("tenant.readwrite")
    {
        return "Insufficient tenant-level scopes. This operation requires Fabric Administrator \
                role assigned in the Microsoft 365 Admin Center > Roles > Fabric Administrator. \
                Re-authenticate with: fabio auth login"
            .to_string();
    }

    // Detect tenant setting disabled (admin 403)
    if combined.contains("tenant setting") && combined.contains("disabled") {
        return "A required tenant setting is disabled. Enable it in the Fabric Admin Portal \
                > Tenant Settings, or use: fabio admin update-tenant-setting --setting-name <NAME> \
                --content '{\"enabled\":true}'"
            .to_string();
    }

    // Detect feature not available (tenant feature flag)
    if combined.contains("feature is not available") || combined.contains("featurenotavailable") {
        return "This feature is not enabled in the tenant. Contact your Fabric administrator \
                to enable the required feature flag in Tenant Settings."
            .to_string();
    }

    // Detect git-specific permission issues (check before generic patterns)
    if combined.contains("git") || combined.contains("source control") {
        return "Insufficient permissions for git operations. Git connect/commit/pull requires \
                Admin or Member workspace role. Verify your role with: \
                fabio workspace show --id <workspace-id>."
            .to_string();
    }

    // Detect OneLake/storage permission issues (check before generic patterns)
    if combined.contains("storage") || combined.contains("onelake") || combined.contains("blob") {
        return "Insufficient OneLake storage permissions. Ensure you have at least \
                Contributor role on the workspace, or that OneLake data access is enabled. \
                Verify workspace role with: fabio workspace show --id <workspace-id>."
            .to_string();
    }

    // Detect generic insufficient workspace role
    if combined.contains("insufficient privileges")
        || combined.contains("does not have permission")
        || combined.contains("unauthorized")
        || combined.contains("access denied")
        || combined.contains("forbidden")
    {
        return "Insufficient workspace permissions. Fabric workspace roles required: \
                Admin (full control), Member (create/edit items), Contributor (edit items), \
                Viewer (read-only). Check your role with: fabio workspace show --id <workspace-id> \
                or ask a workspace Admin to grant you the required role."
            .to_string();
    }

    // Generic 403 hint
    "Insufficient permissions for this operation. Possible causes: \
     (1) Your workspace role (Viewer/Contributor/Member/Admin) is too low for this action. \
     (2) The API scope in your token lacks the required permission. \
     (3) A tenant admin policy restricts this operation. \
     Check your role with: fabio workspace show --id <workspace-id>. \
     Re-authenticate with: fabio auth login."
        .to_string()
}

/// Generate a context-aware hint for 409 Conflict errors.
fn conflict_hint(message: &str, body: &str) -> String {
    let msg_lower = message.to_lowercase();
    let body_lower = body.to_lowercase();
    let combined = format!("{msg_lower} {body_lower}");

    if combined.contains("already in use") || combined.contains("already exists") {
        return "An item with this name already exists in the workspace. \
                Use a different name, or delete the existing item first with: \
                fabio <resource> delete --workspace <WS> --id <ID>"
            .to_string();
    }

    if combined.contains("capacity") {
        return "Capacity conflict. The capacity may already be assigned or in a \
                transitional state. Check capacity status with: fabio capacity show --id <ID>"
            .to_string();
    }

    "Resource conflict (409). The item may already exist or be in a state that \
     conflicts with this operation. Check existing items with: \
     fabio <resource> list --workspace <WS>"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_code_display() {
        assert_eq!(ErrorCode::AuthRequired.to_string(), "AUTH_REQUIRED");
        assert_eq!(ErrorCode::Forbidden.to_string(), "FORBIDDEN");
        assert_eq!(ErrorCode::NotFound.to_string(), "NOT_FOUND");
        assert_eq!(ErrorCode::RateLimited.to_string(), "RATE_LIMITED");
    }

    #[test]
    fn fabio_error_new_has_no_hint() {
        let err = FabioError::new(ErrorCode::NotFound, "item not found");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert_eq!(err.message, "item not found");
        assert!(err.hint.is_none());
    }

    #[test]
    fn fabio_error_with_hint_carries_hint() {
        let err = FabioError::with_hint(
            ErrorCode::InvalidInput,
            "invalid mode",
            "Valid values: Overwrite, Append",
        );
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(err.hint.unwrap(), "Valid values: Overwrite, Append");
    }

    #[test]
    fn from_status_401_maps_to_auth_required_with_hint() {
        let err = FabioError::from_status(401, "unauthorized");
        assert_eq!(err.code, ErrorCode::AuthRequired);
        assert!(err.hint.is_some());
        assert!(err.hint.unwrap().contains("fabio auth login"));
    }

    #[test]
    fn from_status_401_caller_not_authenticated_gives_sp_tenant_setting_hint() {
        // Fabric returns 401 (not 403) when a service principal is authenticated
        // (reads succeed) but a tenant setting gates the mutation (e.g. workspace create).
        let body = r#"{"requestId":"abc","errorCode":"Unauthorized","message":"The caller is not authenticated to access this resource","isRetriable":false}"#;
        let err = FabioError::from_status_with_body(
            401,
            "The caller is not authenticated to access this resource",
            body,
        );
        assert_eq!(err.code, ErrorCode::AuthRequired);
        let hint = err.hint.unwrap();
        assert!(hint.contains("service principal"));
        assert!(hint.contains("Service principals can create workspaces"));
        assert!(!hint.starts_with("Not authenticated."));
    }

    #[test]
    fn from_status_429_maps_to_rate_limited_with_hint() {
        let err = FabioError::from_status(429, "slow down");
        assert_eq!(err.code, ErrorCode::RateLimited);
        assert!(err.hint.unwrap().contains("backoff"));
    }

    #[test]
    fn from_status_404_has_no_hint() {
        let err = FabioError::from_status(404, "not found");
        assert_eq!(err.code, ErrorCode::NotFound);
        assert!(err.hint.is_none());
    }

    #[test]
    fn from_status_500_maps_to_api_error() {
        let err = FabioError::from_status(500, "server error");
        assert_eq!(err.code, ErrorCode::ApiError);
    }

    #[test]
    fn from_status_403_maps_to_forbidden_with_hint() {
        let err = FabioError::from_status(403, "insufficient privileges for this action");
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("workspace role"),
            "Hint should mention workspace roles: {hint}"
        );
    }

    #[test]
    fn from_status_403_generic_message_gives_generic_hint() {
        let err = FabioError::from_status(403, "some error");
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("Insufficient permissions"),
            "Hint should be generic: {hint}"
        );
    }

    #[test]
    fn from_status_403_with_body_context_detects_storage() {
        let err = FabioError::from_status_with_body(
            403,
            "AuthorizationFailure",
            r#"{"error":{"code":"AuthorizationFailure","message":"OneLake storage denied"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("OneLake"),
            "Hint should mention OneLake: {hint}"
        );
    }

    #[test]
    fn from_status_403_with_body_context_detects_git() {
        let err = FabioError::from_status_with_body(
            403,
            "permission denied",
            r#"{"error":{"code":"Forbidden","message":"Git source control access denied"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("git operations"),
            "Hint should mention git: {hint}"
        );
    }

    #[test]
    fn from_status_409_maps_to_conflict_with_hint() {
        let err = FabioError::from_status(409, "item already exists");
        assert_eq!(err.code, ErrorCode::Conflict);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("already exists"),
            "Hint should mention name conflict: {hint}"
        );
    }

    #[test]
    fn from_status_409_name_in_use_gives_rename_hint() {
        let err = FabioError::from_status_with_body(
            409,
            "Conflict",
            r#"{"error":{"code":"Conflict","message":"Requested 'MyReport' is already in use"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Conflict);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("already exists"),
            "Hint should suggest different name: {hint}"
        );
    }

    #[test]
    fn from_status_409_generic_gives_resource_conflict_hint() {
        let err = FabioError::from_status(409, "some conflict");
        assert_eq!(err.code, ErrorCode::Conflict);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("Resource conflict"),
            "Hint should be generic conflict: {hint}"
        );
    }

    #[test]
    fn from_status_412_maps_to_conflict_with_etag_hint() {
        let err = FabioError::from_status_with_body(
            412,
            "Precondition failed",
            r#"{"error":{"code":"PreconditionFailed","message":"ETag does not match current resource version"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Conflict);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("ETag precondition failed"),
            "Hint should explain stale ETag: {hint}"
        );
    }

    #[test]
    fn from_status_403_detects_tenant_scopes() {
        let err = FabioError::from_status_with_body(
            403,
            "The caller does not have sufficient scopes",
            r#"{"error":{"code":"Forbidden","message":"The caller does not have sufficient scopes to perform this operation"}}"#,
        );
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("tenant-level") || hint.contains("Fabric Administrator"),
            "Hint should mention tenant admin: {hint}"
        );
    }

    #[test]
    fn from_status_403_detects_tenant_setting_disabled() {
        let err = FabioError::from_status_with_body(
            403,
            "The operation is not allowed since tenant setting 'External data sharing' is disabled",
            "",
        );
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("tenant setting") && hint.contains("disabled"),
            "Hint should mention tenant setting: {hint}"
        );
    }

    #[test]
    fn from_status_403_detects_feature_not_available() {
        let err = FabioError::from_status_with_body(
            403,
            "FeatureNotAvailable: The feature is not available",
            "",
        );
        assert_eq!(err.code, ErrorCode::Forbidden);
        let hint = err.hint.unwrap();
        assert!(
            hint.contains("feature") && hint.contains("not enabled"),
            "Hint should mention feature flag: {hint}"
        );
    }

    #[test]
    fn enrich_admin_forbidden_gives_tenant_hint() {
        let err: anyhow::Error = FabioError::new(ErrorCode::Forbidden, "access denied").into();
        let enriched = enrich_admin(err, "admin list-workspaces");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::Forbidden);
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("tenant-level Fabric Administrator"),
            "Hint should mention tenant admin: {hint}"
        );
        assert!(
            !hint.contains("Workspace roles: Admin > Member"),
            "Hint should NOT mention workspace roles: {hint}"
        );
    }

    #[test]
    fn enrich_admin_detects_item_type_not_supported() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "The bulk sharing link removal operation is not supported for the requested item type.",
        )
        .into();
        let enriched = enrich_admin(err, "admin bulk-remove-sharing-links");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("Report"),
            "Hint should mention Report type: {hint}"
        );
    }

    #[test]
    fn enrich_admin_detects_purview_label_error() {
        let err: anyhow::Error =
            FabioError::new(ErrorCode::ApiError, "Label is not assigned to user").into();
        let enriched = enrich_admin(err, "admin bulk-set-labels");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("Purview") && hint.contains("M365 E5"),
            "Hint should mention Purview and licensing: {hint}"
        );
    }

    #[test]
    fn enrich_admin_detects_external_sharing_disabled() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::Forbidden,
            "The operation is not allowed since tenant setting 'External data sharing' is disabled",
        )
        .into();
        let enriched = enrich_admin(err, "admin list-external-data-shares");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("AllowExternalDataSharingSwitch"),
            "Hint should mention the specific setting name: {hint}"
        );
    }

    #[test]
    fn enrich_admin_detects_sync_admin_not_supported() {
        let err: anyhow::Error = FabioError::new(
            ErrorCode::ApiError,
            "Syncing admins to subdomains is not supported",
        )
        .into();
        let enriched = enrich_admin(err, "admin sync-domain-roles-to-subdomains");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("Contributor") && hint.contains("--role"),
            "Hint should suggest Contributor role: {hint}"
        );
    }

    #[test]
    fn enrich_admin_passes_through_non_matching_errors() {
        let err: anyhow::Error = FabioError::new(ErrorCode::NotFound, "item not found").into();
        let enriched = enrich_admin(err, "admin show-item");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.code, ErrorCode::NotFound);
        assert!(fabio_err.hint.is_none());
    }

    #[test]
    fn set_retriable_sets_field() {
        let err = FabioError::new(ErrorCode::ApiError, "server error").set_retriable(Some(true));
        assert_eq!(err.retriable, Some(true));
    }

    #[test]
    fn set_retriable_none_leaves_field_none() {
        let err = FabioError::new(ErrorCode::ApiError, "server error").set_retriable(None);
        assert_eq!(err.retriable, None);
    }

    #[test]
    fn new_error_has_retriable_none() {
        let err = FabioError::new(ErrorCode::NotFound, "not found");
        assert_eq!(err.retriable, None);
    }

    #[test]
    fn new_error_has_all_optional_fields_none() {
        let err = FabioError::new(ErrorCode::ApiError, "test");
        assert!(err.request_id.is_none());
        assert!(err.more_details.is_none());
        assert!(err.related_resource.is_none());
    }

    #[test]
    fn set_request_id_sets_field() {
        let err = FabioError::new(ErrorCode::ApiError, "test")
            .set_request_id(Some("req-123".to_string()));
        assert_eq!(err.request_id.as_deref(), Some("req-123"));
    }

    #[test]
    fn set_more_details_sets_field() {
        let details = vec![ErrorDetail {
            error_code: "SubError".to_string(),
            message: "detail msg".to_string(),
        }];
        let err = FabioError::new(ErrorCode::ApiError, "test").set_more_details(Some(details));
        assert_eq!(err.more_details.as_ref().unwrap().len(), 1);
        assert_eq!(err.more_details.as_ref().unwrap()[0].error_code, "SubError");
    }

    #[test]
    fn set_related_resource_sets_field() {
        let resource = RelatedResource {
            resource_id: "item-456".to_string(),
            resource_type: "Notebook".to_string(),
        };
        let err = FabioError::new(ErrorCode::NotFound, "test").set_related_resource(Some(resource));
        let r = err.related_resource.as_ref().unwrap();
        assert_eq!(r.resource_id, "item-456");
        assert_eq!(r.resource_type, "Notebook");
    }

    #[test]
    fn builder_chain_sets_all_fields() {
        let err = FabioError::new(ErrorCode::ApiError, "multi-error")
            .set_retriable(Some(true))
            .set_request_id(Some("req-abc".to_string()))
            .set_more_details(Some(vec![ErrorDetail {
                error_code: "E1".to_string(),
                message: "m1".to_string(),
            }]))
            .set_related_resource(Some(RelatedResource {
                resource_id: "r1".to_string(),
                resource_type: "Lakehouse".to_string(),
            }));
        assert_eq!(err.retriable, Some(true));
        assert_eq!(err.request_id.as_deref(), Some("req-abc"));
        assert_eq!(err.more_details.as_ref().unwrap().len(), 1);
        assert_eq!(
            err.related_resource.as_ref().unwrap().resource_type,
            "Lakehouse"
        );
    }

    // ─── with_typed_hint and set_verify_after tests ──────────────────────────

    #[test]
    fn with_typed_hint_sets_all_fields() {
        let err = FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            "bad input",
            "Use --force to proceed",
            HintType::SafetyBypass,
        );
        assert_eq!(err.code, ErrorCode::InvalidInput);
        assert_eq!(err.message, "bad input");
        assert_eq!(err.hint.as_deref(), Some("Use --force to proceed"));
        assert_eq!(err.hint_type, Some(HintType::SafetyBypass));
        assert!(err.verify_after.is_none());
    }

    #[test]
    fn set_verify_after_chains_correctly() {
        let err = FabioError::with_typed_hint(
            ErrorCode::InvalidInput,
            "stale plan",
            "Use --force to apply",
            HintType::SafetyBypass,
        )
        .set_verify_after("fabio deploy plan --dry-run");

        assert_eq!(
            err.verify_after.as_deref(),
            Some("fabio deploy plan --dry-run")
        );
        assert_eq!(err.hint_type, Some(HintType::SafetyBypass));
    }

    #[test]
    fn with_hint_leaves_hint_type_none() {
        let err = FabioError::with_hint(ErrorCode::InvalidInput, "test", "some hint");
        assert!(err.hint_type.is_none());
        assert!(err.verify_after.is_none());
    }

    #[test]
    fn from_status_401_sets_auth_fix_hint_type() {
        let err = FabioError::from_status_with_body(401, "unauthorized", "");
        assert_eq!(err.hint_type, Some(HintType::AuthFix));
    }

    #[test]
    fn from_status_429_sets_retry_safe_hint_type() {
        let err = FabioError::from_status_with_body(429, "too many requests", "");
        assert_eq!(err.hint_type, Some(HintType::RetrySafe));
    }

    #[test]
    fn from_status_403_sets_semantic_correction_hint_type() {
        let err = FabioError::from_status_with_body(403, "forbidden", "");
        assert_eq!(err.hint_type, Some(HintType::SemanticCorrection));
    }

    #[test]
    fn enrich_ontology_import_failure_adds_untyped_binding_hint() {
        let err: anyhow::Error =
            FabioError::api_error("ALMOperationImportFailed: {0} {1} {2}").into();
        let enriched = enrich_ontology_definition_error(err, "ontology import");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        let hint = fabio_err.hint.as_ref().unwrap();
        assert!(
            hint.contains("UNTYPED property"),
            "Hint should call out untyped-property bindings: {hint}"
        );
        assert!(
            hint.contains("get-definition"),
            "Hint should point at the inspection command: {hint}"
        );
        assert_eq!(fabio_err.hint_type, Some(HintType::SemanticCorrection));
    }

    #[test]
    fn enrich_ontology_import_failure_matches_invalid_definition() {
        let err: anyhow::Error =
            FabioError::invalid_input("Invalid definition for ontology parts").into();
        let enriched = enrich_ontology_definition_error(err, "ontology update-definition");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.is_some());
    }

    #[test]
    fn enrich_ontology_definition_error_passes_through_forbidden() {
        // Permission errors must keep their own (workspace-role) hint, not the
        // generic validator checklist.
        let err: anyhow::Error =
            FabioError::with_hint(ErrorCode::Forbidden, "access denied", "role hint").into();
        let enriched = enrich_ontology_definition_error(err, "ontology import");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert_eq!(fabio_err.hint.as_deref(), Some("role hint"));
    }

    #[test]
    fn enrich_ontology_definition_error_passes_through_unrelated_api_error() {
        let err: anyhow::Error = FabioError::api_error("Some unrelated server error").into();
        let enriched = enrich_ontology_definition_error(err, "ontology import");
        let fabio_err = enriched.downcast_ref::<FabioError>().unwrap();
        assert!(fabio_err.hint.is_none());
    }
}
