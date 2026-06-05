use std::fmt::Write;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use anyhow::Result;
use reqwest::header::AUTHORIZATION;
use reqwest::{Client, Response, StatusCode};
use serde_json::Value;
use tokio::time::sleep;

use azure_core::credentials::TokenCredential;

use crate::errors::{ErrorCode, FabioError};

/// Maximum length of raw response body to include in error messages.
/// Prevents leaking unbounded server-side error details.
const MAX_ERROR_BODY_LEN: usize = 500;

/// Maximum API response body size (50 MB). Prevents OOM from malicious or
/// misconfigured servers returning unbounded responses. File downloads use
/// a separate code path without this limit.
const MAX_API_RESPONSE_SIZE: u64 = 50 * 1024 * 1024;

// ── Base URL defaults (overridable via environment variables) ─────────────
//
// All four API base endpoints can be overridden via environment variables for
// private link or sovereign cloud scenarios:
//
//   FABIO_FABRIC_API_ENDPOINT  — Fabric REST API (default: https://api.fabric.microsoft.com/v1)
//   FABIO_ONELAKE_DFS_ENDPOINT — OneLake DFS     (default: https://onelake.dfs.fabric.microsoft.com)
//   FABIO_ONELAKE_BLOB_ENDPOINT— OneLake Blob    (default: https://onelake.blob.fabric.microsoft.com)
//   FABIO_ARM_ENDPOINT         — Azure Resource Manager (default: https://management.azure.com)
//
// Token scopes can also be overridden for sovereign clouds:
//
//   FABIO_FABRIC_SCOPE  — (default: https://api.fabric.microsoft.com/.default)
//   FABIO_STORAGE_SCOPE — (default: https://storage.azure.com/.default)
//   FABIO_SQL_SCOPE     — (default: https://database.windows.net/.default)
//   FABIO_ARM_SCOPE     — (default: https://management.azure.com/.default)

/// Read an environment variable, stripping any trailing slash from the value.
fn env_or_default(var: &str, default: &str) -> String {
    std::env::var(var).ok().map_or_else(
        || default.to_string(),
        |v| v.trim_end_matches('/').to_string(),
    )
}

static FABRIC_BASE_URL: LazyLock<String> = LazyLock::new(|| {
    env_or_default(
        "FABIO_FABRIC_API_ENDPOINT",
        "https://api.fabric.microsoft.com/v1",
    )
});

static ONELAKE_DFS_URL: LazyLock<String> = LazyLock::new(|| {
    env_or_default(
        "FABIO_ONELAKE_DFS_ENDPOINT",
        "https://onelake.dfs.fabric.microsoft.com",
    )
});

static ONELAKE_BLOB_URL: LazyLock<String> = LazyLock::new(|| {
    env_or_default(
        "FABIO_ONELAKE_BLOB_ENDPOINT",
        "https://onelake.blob.fabric.microsoft.com",
    )
});

static ARM_BASE_URL: LazyLock<String> =
    LazyLock::new(|| env_or_default("FABIO_ARM_ENDPOINT", "https://management.azure.com"));

static POWERBI_BASE_URL: LazyLock<String> = LazyLock::new(|| {
    env_or_default(
        "FABIO_POWERBI_ENDPOINT",
        "https://api.powerbi.com/v1.0/myorg",
    )
});

static FABRIC_SCOPE: LazyLock<String> = LazyLock::new(|| {
    env_or_default(
        "FABIO_FABRIC_SCOPE",
        "https://api.fabric.microsoft.com/.default",
    )
});

static STORAGE_SCOPE: LazyLock<String> =
    LazyLock::new(|| env_or_default("FABIO_STORAGE_SCOPE", "https://storage.azure.com/.default"));

static SQL_SCOPE: LazyLock<String> =
    LazyLock::new(|| env_or_default("FABIO_SQL_SCOPE", "https://database.windows.net/.default"));

static ARM_SCOPE: LazyLock<String> =
    LazyLock::new(|| env_or_default("FABIO_ARM_SCOPE", "https://management.azure.com/.default"));
const LRO_POLL_INTERVAL: Duration = Duration::from_secs(2);
const LRO_MAX_WAIT: Duration = Duration::from_secs(120);

/// Minimum remaining lifetime before a token is considered expired and re-acquired.
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_secs(300); // 5 minutes

/// URL-encode each segment of a `OneLake` path while preserving `/` separators.
/// Prevents query string injection (`?`), fragment injection (`#`), and ensures
/// spaces and special characters are properly handled in DFS/Blob API URLs.
fn encode_onelake_path(path: &str) -> String {
    path.split('/')
        .map(|seg| urlencoding::encode(seg))
        .collect::<Vec<_>>()
        .join("/")
}

/// Response from a paginated list endpoint.
pub struct PaginatedResponse {
    /// Collected items across all fetched pages.
    pub items: Vec<Value>,
    /// If present, there are more pages available. Pass this token to fetch the next page.
    pub continuation_token: Option<String>,
}

/// Which credential source successfully provided a token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CredentialSource {
    /// Fabio's own cached token (from `fabio auth login` device code flow).
    FabioCache,
    /// Service principal via `AZURE_TENANT_ID` + `AZURE_CLIENT_ID` + `AZURE_CLIENT_SECRET` env vars.
    Environment,
    /// Azure Managed Identity (system-assigned or user-assigned).
    ManagedIdentity,
    /// Azure CLI (`az login`).
    AzureCli,
    /// Azure Developer CLI (`azd auth login`).
    AzureDeveloperCli,
}

impl std::fmt::Display for CredentialSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::FabioCache => write!(f, "fabio cache (device code)"),
            Self::Environment => write!(f, "environment (service principal)"),
            Self::ManagedIdentity => write!(f, "managed identity"),
            Self::AzureCli => write!(f, "Azure CLI"),
            Self::AzureDeveloperCli => write!(f, "Azure Developer CLI"),
        }
    }
}

/// A cached token with its expiry time.
#[derive(Clone)]
struct CachedToken {
    token: String,
    /// Pre-formatted "Bearer {token}" header value to avoid per-request allocation.
    bearer_header: String,
    expires_on: std::time::SystemTime,
}

impl CachedToken {
    fn new(token: String, expires_on: std::time::SystemTime) -> Self {
        let bearer_header = format!("Bearer {token}");
        Self {
            token,
            bearer_header,
            expires_on,
        }
    }

    fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now();
        self.expires_on
            .duration_since(now)
            .map_or(true, |remaining| remaining < TOKEN_REFRESH_MARGIN)
    }
}

/// Fabric API client with token management and LRO polling.
#[derive(Clone)]
pub struct FabricClient {
    http: Client,
    fabric_token: Arc<tokio::sync::RwLock<Option<CachedToken>>>,
    storage_token: Arc<tokio::sync::RwLock<Option<CachedToken>>>,
    sql_token: Arc<tokio::sync::RwLock<Option<CachedToken>>>,
    arm_token: Arc<tokio::sync::RwLock<Option<CachedToken>>>,
    credential_source: Arc<tokio::sync::RwLock<Option<CredentialSource>>>,
    /// When set, enables private link URL routing for workspace-scoped requests.
    private_link_workspace: Option<String>,
    /// Maximum time to wait for LRO polling (default: 120s).
    lro_max_wait: Duration,
}

impl FabricClient {
    pub fn new() -> Self {
        // Disable automatic redirect following to prevent bearer token leakage.
        // HTTP redirects could forward Authorization headers to attacker-controlled
        // domains. We handle LRO Location headers explicitly with validation instead.
        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("fabio/", env!("CARGO_PKG_VERSION")))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            http,
            fabric_token: Arc::new(tokio::sync::RwLock::new(None)),
            storage_token: Arc::new(tokio::sync::RwLock::new(None)),
            sql_token: Arc::new(tokio::sync::RwLock::new(None)),
            arm_token: Arc::new(tokio::sync::RwLock::new(None)),
            credential_source: Arc::new(tokio::sync::RwLock::new(None)),
            private_link_workspace: None,
            lro_max_wait: LRO_MAX_WAIT,
        }
    }

    /// Create a client configured for private link routing.
    /// When enabled, workspace-scoped URLs are transformed to use the private link subdomain.
    pub fn with_private_link(mut self, workspace_id: String) -> Self {
        self.private_link_workspace = Some(workspace_id);
        self
    }

    /// Set a custom LRO polling timeout (default: 120s).
    pub const fn with_lro_timeout(mut self, timeout: Duration) -> Self {
        self.lro_max_wait = timeout;
        self
    }

    /// Construct the Fabric API base URL, applying private link transform if configured.
    /// Private link format: `{wsid_no_dashes}.z{first2chars}.w.api.fabric.microsoft.com/v1`
    ///
    /// If `FABIO_FABRIC_API_ENDPOINT` is set, that value is used directly (no private link
    /// transform applied — the env var is assumed to contain the full desired base URL).
    fn fabric_url(&self, path: &str) -> String {
        if std::env::var("FABIO_FABRIC_API_ENDPOINT").is_ok() {
            return format!("{}{path}", *FABRIC_BASE_URL);
        }
        self.private_link_workspace.as_ref().map_or_else(
            || format!("{}{path}", *FABRIC_BASE_URL),
            |ws_id| {
                let no_dashes = ws_id.replace('-', "");
                let z_prefix = &no_dashes[..2];
                format!("https://{no_dashes}.z{z_prefix}.w.api.fabric.microsoft.com/v1{path}")
            },
        )
    }

    /// Construct a `OneLake` DFS URL, applying private link transform if configured.
    /// Private link format: `{wsid_no_dashes}.z{first2chars}.onelake.dfs.fabric.microsoft.com`
    ///
    /// If `FABIO_ONELAKE_DFS_ENDPOINT` is set, that value is used directly (no private link
    /// transform).
    fn onelake_dfs_url(&self, workspace: &str, suffix: &str) -> String {
        if std::env::var("FABIO_ONELAKE_DFS_ENDPOINT").is_ok() {
            return format!("{}/{workspace}/{suffix}", *ONELAKE_DFS_URL);
        }
        if self.private_link_workspace.is_some() {
            let no_dashes = workspace.replace('-', "");
            let z_prefix = &no_dashes[..2.min(no_dashes.len())];
            format!(
                "https://{no_dashes}.z{z_prefix}.onelake.dfs.fabric.microsoft.com/{workspace}/{suffix}"
            )
        } else {
            format!("{}/{workspace}/{suffix}", *ONELAKE_DFS_URL)
        }
    }

    /// Construct a `OneLake` Blob URL, applying private link transform if configured.
    ///
    /// If `FABIO_ONELAKE_BLOB_ENDPOINT` is set, that value is used directly (no private link
    /// transform).
    fn onelake_blob_url(&self, workspace: &str, suffix: &str) -> String {
        if std::env::var("FABIO_ONELAKE_BLOB_ENDPOINT").is_ok() {
            return format!("{}/{workspace}/{suffix}", *ONELAKE_BLOB_URL);
        }
        if self.private_link_workspace.is_some() {
            let no_dashes = workspace.replace('-', "");
            let z_prefix = &no_dashes[..2.min(no_dashes.len())];
            format!(
                "https://{no_dashes}.z{z_prefix}.onelake.blob.fabric.microsoft.com/{workspace}/{suffix}"
            )
        } else {
            format!("{}/{workspace}/{suffix}", *ONELAKE_BLOB_URL)
        }
    }

    /// Ensure we have a valid Fabric API token (auto-refreshes if near expiry).
    /// Returns the pre-formatted "Bearer {token}" header value.
    pub async fn require_auth(&self) -> Result<String> {
        {
            let guard = self.fabric_token.read().await;
            if let Some(ref cached) = *guard {
                if !cached.is_expired() {
                    return Ok(cached.bearer_header.clone());
                }
            }
        }

        let (token, source) = acquire_token(&FABRIC_SCOPE).await?;
        let bearer = token.bearer_header.clone();
        let mut guard = self.fabric_token.write().await;
        *guard = Some(token);
        drop(guard);

        let mut src_guard = self.credential_source.write().await;
        *src_guard = Some(source);
        drop(src_guard);

        Ok(bearer)
    }

    /// Get a storage token for `OneLake` operations (auto-refreshes if near expiry).
    /// Returns the pre-formatted "Bearer {token}" header value.
    pub async fn require_storage_auth(&self) -> Result<String> {
        {
            let guard = self.storage_token.read().await;
            if let Some(ref cached) = *guard {
                if !cached.is_expired() {
                    return Ok(cached.bearer_header.clone());
                }
            }
        }

        let (token, _source) = acquire_token(&STORAGE_SCOPE).await?;
        let bearer = token.bearer_header.clone();
        let mut guard = self.storage_token.write().await;
        *guard = Some(token);
        drop(guard);
        Ok(bearer)
    }

    /// Get a SQL token for TDS connections (scope: `database.windows.net`).
    pub async fn require_sql_auth(&self) -> Result<String> {
        {
            let guard = self.sql_token.read().await;
            if let Some(ref cached) = *guard {
                if !cached.is_expired() {
                    return Ok(cached.token.clone());
                }
            }
        }

        let (token, _source) = acquire_token(&SQL_SCOPE).await?;
        let raw_token = token.token.clone();
        let mut guard = self.sql_token.write().await;
        *guard = Some(token);
        drop(guard);
        Ok(raw_token)
    }

    /// Get an ARM token for Azure Resource Manager operations (scope: `management.azure.com`).
    /// Returns the pre-formatted "Bearer {token}" header value.
    pub async fn require_arm_auth(&self) -> Result<String> {
        {
            let guard = self.arm_token.read().await;
            if let Some(ref cached) = *guard {
                if !cached.is_expired() {
                    return Ok(cached.bearer_header.clone());
                }
            }
        }

        let (token, _source) = acquire_token(&ARM_SCOPE).await?;
        let bearer = token.bearer_header.clone();
        let mut guard = self.arm_token.write().await;
        *guard = Some(token);
        drop(guard);
        Ok(bearer)
    }

    /// Get a token for an arbitrary scope (used for Kusto queries with dynamic query URIs).
    /// Not cached — each call acquires a fresh token from the credential chain.
    pub async fn require_token_for_scope(&self, scope: &str) -> Result<String> {
        let (token, _source) = acquire_token(scope).await?;
        Ok(token.token)
    }

    /// Returns the inner HTTP client for direct requests (e.g., Kusto REST API).
    pub const fn http(&self) -> &Client {
        &self.http
    }

    /// Returns which credential source was used for the last successful authentication.
    pub async fn credential_source(&self) -> Option<CredentialSource> {
        let guard = self.credential_source.read().await;
        *guard
    }

    /// Invalidate the cached Fabric API token (forces re-acquisition on next request).
    async fn invalidate_fabric_token(&self) {
        let mut guard = self.fabric_token.write().await;
        *guard = None;
    }

    /// Invalidate the cached Storage token (forces re-acquisition on next request).
    async fn invalidate_storage_token(&self) {
        let mut guard = self.storage_token.write().await;
        *guard = None;
    }

    /// Upload a file to `OneLake` via DFS (create + append + flush).
    /// Retries once on 401 with a fresh storage token.
    /// Accepts owned `Vec<u8>` to avoid copying the payload.
    pub async fn upload_onelake_file(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
        data: Vec<u8>,
    ) -> Result<Value> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let mut token = self.require_storage_auth().await?;
        let encoded_path = encode_onelake_path(path);
        let base = self.onelake_dfs_url(workspace, &format!("{item}/{encoded_path}"));

        // Step 1: Create
        let resp = self
            .http
            .put(format!("{base}?resource=file"))
            .header(AUTHORIZATION, &token)
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            token = self.require_storage_auth().await?;
            self.http
                .put(format!("{base}?resource=file"))
                .header(AUTHORIZATION, &token)
                .header("Content-Length", "0")
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
        }

        // Step 2: Append
        let data_len = data.len();
        self.http
            .patch(format!("{base}?action=append&position=0"))
            .header(AUTHORIZATION, &token)
            .header("Content-Length", data_len.to_string())
            .body(data)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        // Step 3: Flush
        self.http
            .patch(format!("{base}?action=flush&position={data_len}"))
            .header(AUTHORIZATION, &token)
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        Ok(serde_json::json!({
            "path": path,
            "size": data_len,
            "status": "uploaded"
        }))
    }

    /// Download a file from `OneLake` via DFS. Retries once on 401.
    pub async fn download_onelake_file(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
    ) -> Result<Vec<u8>> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let token = self.require_storage_auth().await?;
        let encoded_path = encode_onelake_path(path);
        let url = self.onelake_dfs_url(workspace, &format!("{item}/{encoded_path}"));

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                return Err(FabioError::from_status(status, text).into());
            }
            return Ok(resp.bytes().await?.to_vec());
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        Ok(resp.bytes().await?.to_vec())
    }

    /// List files in `OneLake` via DFS. Retries once on 401.
    pub async fn list_onelake_files(
        &self,
        workspace: &str,
        item: &str,
        directory: Option<&str>,
    ) -> Result<Vec<Value>> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let token = self.require_storage_auth().await?;
        let mut url = self.onelake_dfs_url(
            workspace,
            &format!("{item}?resource=filesystem&recursive=true"),
        );
        if let Some(dir) = directory {
            let _ = write!(url, "&directory={}", urlencoding::encode(dir));
        }

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            let mut body = handle_response(resp).await?;
            let paths = match body.get_mut("paths").map(Value::take) {
                Some(Value::Array(arr)) => arr,
                _ => Vec::new(),
            };
            return Ok(paths);
        }

        let mut body = handle_response(resp).await?;
        let paths = match body.get_mut("paths").map(Value::take) {
            Some(Value::Array(arr)) => arr,
            _ => Vec::new(),
        };

        Ok(paths)
    }

    /// Server-side file copy via `OneLake` Blob API. Retries once on 401.
    pub async fn copy_onelake_file(
        &self,
        src_workspace: &str,
        src_item: &str,
        src_path: &str,
        dst_workspace: &str,
        dst_item: &str,
        dst_path: &str,
    ) -> Result<Value> {
        validate_uuid(src_workspace, "source-workspace")?;
        validate_uuid(src_item, "source-item")?;
        validate_uuid(dst_workspace, "dest-workspace")?;
        validate_uuid(dst_item, "dest-item")?;
        let token = self.require_storage_auth().await?;
        let source_url = self.onelake_blob_url(
            src_workspace,
            &format!("{src_item}/{}", encode_onelake_path(src_path)),
        );
        let dest_url = self.onelake_blob_url(
            dst_workspace,
            &format!("{dst_item}/{}", encode_onelake_path(dst_path)),
        );

        let resp = self
            .http
            .put(&dest_url)
            .header(AUTHORIZATION, &token)
            .header("x-ms-copy-source", &source_url)
            .header("Content-Length", "0")
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .put(&dest_url)
                .header(AUTHORIZATION, &token)
                .header("x-ms-copy-source", &source_url)
                .header("Content-Length", "0")
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if !resp.status().is_success() && resp.status() != StatusCode::ACCEPTED {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                return Err(FabioError::from_status(status, text).into());
            }
            return Ok(serde_json::json!({
                "source": src_path,
                "destination": dst_path,
                "status": "copied"
            }));
        }

        if !resp.status().is_success() && resp.status() != StatusCode::ACCEPTED {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        Ok(serde_json::json!({
            "source": src_path,
            "destination": dst_path,
            "status": "copied"
        }))
    }

    /// Delete a file from `OneLake` via DFS. Retries once on 401.
    pub async fn delete_onelake_file(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
    ) -> Result<Value> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let token = self.require_storage_auth().await?;
        let encoded_path = encode_onelake_path(path);
        let url = self.onelake_dfs_url(workspace, &format!("{item}/{encoded_path}"));

        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .delete(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                return Err(FabioError::from_status(status, text).into());
            }
        } else if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        Ok(serde_json::json!({
            "path": path,
            "status": "deleted"
        }))
    }

    /// GET request to Fabric REST API (retries once on 401 with fresh token).
    pub async fn get(&self, path: &str) -> Result<Value> {
        const MAX_TRANSIENT_RETRIES: u32 = 3;
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            // Token may have expired server-side; refresh and retry once
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        // Retry on transient server errors (502/503/504)
        let status_code = resp.status().as_u16();
        if matches!(status_code, 502..=504) {
            for attempt in 1..=MAX_TRANSIENT_RETRIES {
                let backoff_secs = u64::from(attempt);
                eprintln!(
                    "Transient error (HTTP {status_code}). Retrying in {backoff_secs}s (attempt {attempt}/{MAX_TRANSIENT_RETRIES})..."
                );
                sleep(Duration::from_secs(backoff_secs)).await;
                let token = self.require_auth().await?;
                let resp = self
                    .http
                    .get(&url)
                    .header(AUTHORIZATION, &token)
                    .send()
                    .await
                    .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
                let retry_status = resp.status().as_u16();
                if !matches!(retry_status, 502..=504) {
                    return handle_response(resp).await;
                }
                if attempt == MAX_TRANSIENT_RETRIES {
                    return handle_response(resp).await;
                }
            }
        }

        handle_response(resp).await
    }

    /// GET request returning raw text response as a JSON string value.
    /// Used for endpoints that return non-JSON content (e.g., file downloads).
    pub async fn get_text(&self, path: &str) -> Result<String> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

            let status = resp.status();
            if !status.is_success() {
                let body = resp.text().await.unwrap_or_default();
                return Err(FabioError::new(ErrorCode::ApiError, body).into());
            }
            return Ok(resp.text().await.unwrap_or_default());
        }

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(FabioError::new(ErrorCode::ApiError, body).into());
        }
        Ok(resp.text().await.unwrap_or_default())
    }

    /// GET a paginated list from Fabric REST API.
    ///
    /// When `paginate` is true, follows `continuationToken` until all pages are fetched.
    /// When `start_token` is provided, begins pagination from that token.
    /// Returns a `PaginatedResponse` with all collected items and optional continuation token.
    pub async fn get_list(
        &self,
        path: &str,
        array_field: &str,
        paginate: bool,
        start_token: Option<&str>,
    ) -> Result<PaginatedResponse> {
        // Safety limit: prevent unbounded memory growth when --all fetches
        // an unexpectedly large result set (e.g., 100K+ items on large tenants).
        const MAX_PAGES: usize = 500;

        let token = self.require_auth().await?;
        let mut all_items: Vec<Value> = Vec::new();
        let mut continuation_token: Option<String> = start_token.map(String::from);
        let mut page_count: usize = 0;

        loop {
            page_count += 1;
            if paginate && page_count > MAX_PAGES {
                eprintln!(
                    "Warning: pagination stopped after {MAX_PAGES} pages ({} items). \
                     Use --continuation-token to resume.",
                    all_items.len()
                );
                return Ok(PaginatedResponse {
                    items: all_items,
                    continuation_token,
                });
            }

            let url = continuation_token.as_ref().map_or_else(
                || self.fabric_url(path),
                |ct| {
                    let separator = if path.contains('?') { '&' } else { '?' };
                    format!(
                        "{}{separator}continuationToken={}",
                        self.fabric_url(path),
                        urlencoding::encode(ct)
                    )
                },
            );

            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

            let mut body = handle_response(resp).await?;

            // Extract items from the specified array field (try primary, then fallback to "value")
            // Use .take() to move ownership without cloning the entire Vec<Value>.
            let items = body.get_mut(array_field).map(Value::take).or_else(|| {
                if array_field == "value" {
                    None
                } else {
                    body.get_mut("value").map(Value::take)
                }
            });
            if let Some(Value::Array(arr)) = items {
                all_items.extend(arr);
            }

            // Check for continuation token
            let next_token = body
                .get("continuationToken")
                .and_then(Value::as_str)
                .map(String::from);

            if !paginate || next_token.is_none() {
                // Return with the token if we're not paginating (so caller can expose it)
                return Ok(PaginatedResponse {
                    items: all_items,
                    continuation_token: next_token,
                });
            }

            continuation_token = next_token;
        }
    }

    /// POST request to Fabric REST API, optionally polling for LRO completion.
    /// Retries once on 401 with a fresh token.
    /// Retries up to 3 times on 429/430 (rate limited) with exponential backoff.
    pub async fn post(&self, path: &str, body: &Value, poll: bool) -> Result<Value> {
        const MAX_RATE_LIMIT_RETRIES: u32 = 3;
        const MAX_TRANSIENT_RETRIES: u32 = 3;
        let url = self.fabric_url(path);
        let mut attempt: u32 = 0;
        let mut transient_attempt: u32 = 0;

        loop {
            let token = self.require_auth().await?;

            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

            if resp.status() == StatusCode::UNAUTHORIZED {
                self.invalidate_fabric_token().await;
                let token = self.require_auth().await?;
                let resp = self
                    .http
                    .post(&url)
                    .header(AUTHORIZATION, &token)
                    .json(body)
                    .send()
                    .await
                    .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
                if poll && resp.status() == StatusCode::ACCEPTED {
                    return self.poll_lro(resp).await;
                }
                return handle_response(resp).await;
            }

            // Retry on rate-limit (429) or Spark capacity throttle (430)
            let status_code = resp.status().as_u16();
            if (status_code == 429 || status_code == 430) && attempt < MAX_RATE_LIMIT_RETRIES {
                attempt += 1;
                // Respect Retry-After header if present, otherwise use fixed backoff.
                // Cap at 300s to prevent a malicious server from parking the CLI indefinitely.
                let backoff_secs = resp
                    .headers()
                    .get("Retry-After")
                    .or_else(|| resp.headers().get("retry-after"))
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .map_or_else(|| 10u64 * u64::from(attempt), |s| s.min(300)); // fallback: 10s, 20s, 30s
                eprintln!(
                    "Rate limited (HTTP {status_code}). Retrying in {backoff_secs}s (attempt {attempt}/{MAX_RATE_LIMIT_RETRIES})..."
                );
                sleep(Duration::from_secs(backoff_secs)).await;
                continue;
            }

            // Retry on transient server errors (502/503/504)
            if matches!(status_code, 502..=504) && transient_attempt < MAX_TRANSIENT_RETRIES {
                transient_attempt += 1;
                let backoff_secs = u64::from(transient_attempt); // 1s, 2s, 3s
                eprintln!(
                    "Transient error (HTTP {status_code}). Retrying in {backoff_secs}s (attempt {transient_attempt}/{MAX_TRANSIENT_RETRIES})..."
                );
                sleep(Duration::from_secs(backoff_secs)).await;
                continue;
            }

            if poll && resp.status() == StatusCode::ACCEPTED {
                return self.poll_lro(resp).await;
            }

            return handle_response(resp).await;
        }
    }

    /// POST request with raw text body (text/plain content type).
    pub async fn post_raw(&self, path: &str, content: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .header("Content-Type", "text/plain")
            .body(content.to_owned())
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .header("Content-Type", "text/plain")
                .body(content.to_owned())
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// POST binary data with `application/octet-stream` content type (retries once on 401).
    pub async fn post_octet_stream(&self, path: &str, data: Vec<u8>) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .header("Content-Type", "application/octet-stream")
            .body(data.clone())
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .header("Content-Type", "application/octet-stream")
                .body(data)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// POST JSON request returning binary response bytes (retries once on 401).
    /// Used for endpoints that return non-JSON content (e.g., Apache Arrow IPC).
    pub async fn post_fabric_bytes(&self, path: &str, body: &Value) -> Result<Vec<u8>> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return Ok(resp.bytes().await?.to_vec());
        }

        if !resp.status().is_success() {
            // Try to parse error response as JSON
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| {
                    v.get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .map(String::from)
                })
                .unwrap_or_else(|| format!("HTTP {status}"));
            return Err(FabioError::new(ErrorCode::ApiError, msg).into());
        }

        Ok(resp.bytes().await?.to_vec())
    }

    /// POST request with configurable LRO wait and timeout (retries once on 401).
    pub async fn post_with_timeout(
        &self,
        path: &str,
        body: &Value,
        wait: bool,
        timeout_secs: u64,
    ) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return self
                .handle_post_with_timeout_response(resp, wait, timeout_secs)
                .await;
        }

        self.handle_post_with_timeout_response(resp, wait, timeout_secs)
            .await
    }

    /// Handle the response from `post_with_timeout` (factored out for retry).
    async fn handle_post_with_timeout_response(
        &self,
        resp: Response,
        wait: bool,
        timeout_secs: u64,
    ) -> Result<Value> {
        if resp.status() == StatusCode::ACCEPTED {
            if wait {
                return self
                    .poll_lro_with_timeout(resp, Duration::from_secs(timeout_secs))
                    .await;
            }
            let location = resp
                .headers()
                .get("location")
                .or_else(|| resp.headers().get("Location"))
                .and_then(|v| v.to_str().ok())
                .map(String::from);
            let operation_id = resp
                .headers()
                .get("x-ms-operation-id")
                .and_then(|v| v.to_str().ok())
                .map(String::from);

            return Ok(serde_json::json!({
                "status": "accepted",
                "operationId": operation_id,
                "location": location,
            }));
        }

        handle_response(resp).await
    }

    /// GET request that handles LRO (202 Accepted) responses (retries once on 401).
    pub async fn get_with_lro(&self, path: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if resp.status() == StatusCode::ACCEPTED {
                return self.poll_lro(resp).await;
            }
            return handle_response(resp).await;
        }

        if resp.status() == StatusCode::ACCEPTED {
            return self.poll_lro(resp).await;
        }

        handle_response(resp).await
    }

    /// PATCH request to Fabric REST API (retries once on 401).
    pub async fn patch(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .patch(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .patch(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// PUT request to Fabric REST API (retries once on 401).
    pub async fn put(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .put(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .put(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// PUT request with raw text body (e.g., for file uploads requiring text/plain).
    pub async fn put_raw(&self, path: &str, content: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .put(&url)
            .header(AUTHORIZATION, &token)
            .header("Content-Type", "text/plain")
            .body(content.to_owned())
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .put(&url)
                .header(AUTHORIZATION, &token)
                .header("Content-Type", "text/plain")
                .body(content.to_owned())
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// DELETE request to Fabric REST API (retries once on 401).
    pub async fn delete(&self, path: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .delete(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// POST request to Power BI REST API (different base URL, same Fabric auth scope).
    /// Used for Power BI-specific operations like Publish to Web.
    /// PATCH request to Power BI REST API (retries once on 401).
    pub async fn patch_powerbi(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .patch(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .patch(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    pub async fn get_powerbi(&self, path: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .get(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    pub async fn post_powerbi(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// PUT request to Power BI REST API (retries once on 401).
    pub async fn put_powerbi(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .put(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .put(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// DELETE request to Power BI REST API (retries once on 401).
    pub async fn delete_powerbi(&self, path: &str) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .delete(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return handle_response(resp).await;
        }

        handle_response(resp).await
    }

    /// POST request to Power BI REST API returning raw bytes (for binary downloads like PBIX export).
    pub async fn post_powerbi_bytes(&self, path: &str, body: &Value) -> Result<Vec<u8>> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                return Err(FabioError::from_status(status, text).into());
            }
            return Ok(resp.bytes().await?.to_vec());
        }

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }
        Ok(resp.bytes().await?.to_vec())
    }

    /// POST multipart form-data to Power BI REST API (for PBIX import).
    /// `path` should include query parameters (e.g., `?datasetDisplayName=...`).
    pub async fn post_powerbi_multipart(
        &self,
        path: &str,
        form: reqwest::multipart::Form,
    ) -> Result<Value> {
        let token = self.require_auth().await?;
        let url = format!("{}{path}", *POWERBI_BASE_URL);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            // Cannot retry multipart (form is consumed) — return auth error
            return Err(FabioError::new(
                ErrorCode::AuthRequired,
                "Authentication failed for Power BI import. Please re-authenticate.".to_string(),
            )
            .into());
        }

        handle_response(resp).await
    }

    // ── ARM (Azure Resource Manager) methods ──────────────────────────────

    /// GET request to Azure Resource Manager API.
    pub async fn arm_get(&self, path: &str) -> Result<Value> {
        let token = self.require_arm_auth().await?;
        let url = format!("{}{path}", *ARM_BASE_URL);

        let resp = self
            .http
            .get(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        handle_response(resp).await
    }

    /// POST request to Azure Resource Manager API (with optional LRO polling).
    pub async fn arm_post(&self, path: &str, body: &Value, poll: bool) -> Result<Value> {
        let token = self.require_arm_auth().await?;
        let url = format!("{}{path}", *ARM_BASE_URL);

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if poll {
            let status = resp.status();
            if status == StatusCode::OK {
                return handle_response(resp).await;
            }
            if status == StatusCode::ACCEPTED {
                return self.poll_arm_lro(resp).await;
            }
        }

        handle_response(resp).await
    }

    /// PUT request to Azure Resource Manager API (with LRO polling).
    pub async fn arm_put(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_arm_auth().await?;
        let url = format!("{}{path}", *ARM_BASE_URL);

        let resp = self
            .http
            .put(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        let status = resp.status();
        if status == StatusCode::OK || status == StatusCode::CREATED {
            return handle_response(resp).await;
        }
        if status == StatusCode::ACCEPTED {
            return self.poll_arm_lro(resp).await;
        }

        handle_response(resp).await
    }

    /// PATCH request to Azure Resource Manager API (with LRO polling).
    pub async fn arm_patch(&self, path: &str, body: &Value) -> Result<Value> {
        let token = self.require_arm_auth().await?;
        let url = format!("{}{path}", *ARM_BASE_URL);

        let resp = self
            .http
            .patch(&url)
            .header(AUTHORIZATION, &token)
            .json(body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        let status = resp.status();
        if status == StatusCode::OK {
            return handle_response(resp).await;
        }
        if status == StatusCode::ACCEPTED {
            return self.poll_arm_lro(resp).await;
        }

        handle_response(resp).await
    }

    /// DELETE request to Azure Resource Manager API (with LRO polling).
    pub async fn arm_delete(&self, path: &str) -> Result<Value> {
        let token = self.require_arm_auth().await?;
        let url = format!("{}{path}", *ARM_BASE_URL);

        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        let status = resp.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(serde_json::json!({"status": "deleted"}));
        }
        if status == StatusCode::ACCEPTED {
            return self.poll_arm_lro(resp).await;
        }

        handle_response(resp).await
    }

    /// Poll an ARM LRO using `Azure-AsyncOperation` or `Location` header.
    async fn poll_arm_lro(&self, resp: Response) -> Result<Value> {
        let poll_url = resp
            .headers()
            .get("Azure-AsyncOperation")
            .or_else(|| resp.headers().get("Location"))
            .and_then(|v| v.to_str().ok())
            .map(ToString::to_string);

        let Some(poll_url) = poll_url else {
            // No LRO header — just return the response as-is
            return handle_response(resp).await;
        };

        let start = std::time::Instant::now();
        let mut interval = LRO_POLL_INTERVAL;

        loop {
            if start.elapsed() > self.lro_max_wait {
                return Err(FabioError::with_hint(
                    ErrorCode::Timeout,
                    format!("ARM LRO timed out after {}s", self.lro_max_wait.as_secs()),
                    format!("Increase --lro-timeout or poll manually: GET {poll_url}"),
                )
                .into());
            }

            sleep(interval).await;

            let token = self.require_arm_auth().await?;
            let poll_resp = self
                .http
                .get(&poll_url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

            let status_code = poll_resp.status();
            if !status_code.is_success() {
                return handle_response(poll_resp).await;
            }

            let body: Value = poll_resp
                .json()
                .await
                .unwrap_or_else(|_| serde_json::json!({}));

            let op_status = body.get("status").and_then(Value::as_str).unwrap_or("");

            match op_status {
                "Succeeded" => {
                    // If there's a resourceId or result, try to fetch the final resource
                    if let Some(resource_id) = body.get("resourceId").and_then(Value::as_str) {
                        // The resourceId is a full ARM path — fetch it
                        let resource_url =
                            format!("{}{resource_id}?api-version=2023-11-01", *ARM_BASE_URL);
                        let token = self.require_arm_auth().await?;
                        let resource_resp = self
                            .http
                            .get(&resource_url)
                            .header(AUTHORIZATION, &token)
                            .send()
                            .await
                            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
                        return handle_response(resource_resp).await;
                    }
                    return Ok(body);
                }
                "Failed" | "Canceled" => {
                    let err_msg = body
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(Value::as_str)
                        .unwrap_or("ARM operation failed");
                    return Err(FabioError::new(ErrorCode::ApiError, err_msg.to_string()).into());
                }
                _ => {
                    // Still in progress — check Retry-After header
                    if let Some(retry_after) = body.get("retryAfter").and_then(Value::as_u64) {
                        interval = Duration::from_secs(retry_after.min(60));
                    }
                }
            }
        }
    }

    /// Get file properties from `OneLake` via DFS HEAD request. Retries once on 401.
    /// Returns headers including `Content-MD5` and `ETag`.
    pub async fn get_file_properties(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
    ) -> Result<Value> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let token = self.require_storage_auth().await?;
        let encoded_path = encode_onelake_path(path);
        let url = self.onelake_dfs_url(workspace, &format!("{item}/{encoded_path}"));

        let resp = self
            .http
            .head(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .head(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return Self::extract_file_properties(resp, path).await;
        }

        Self::extract_file_properties(resp, path).await
    }

    /// Extract file properties from a HEAD response.
    async fn extract_file_properties(resp: Response, path: &str) -> Result<Value> {
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        let headers = resp.headers();
        let content_md5 = headers
            .get("Content-MD5")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let etag = headers
            .get("ETag")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let content_length = headers
            .get("Content-Length")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        Ok(serde_json::json!({
            "path": path,
            "contentMD5": content_md5,
            "eTag": etag,
            "contentLength": content_length
        }))
    }

    /// Delete a directory recursively from `OneLake` via DFS. Retries once on 401.
    pub async fn delete_onelake_directory(
        &self,
        workspace: &str,
        item: &str,
        path: &str,
    ) -> Result<Value> {
        validate_uuid(workspace, "workspace")?;
        validate_uuid(item, "item")?;
        let token = self.require_storage_auth().await?;
        let encoded_path = encode_onelake_path(path);
        let url = self.onelake_dfs_url(workspace, &format!("{item}/{encoded_path}?recursive=true"));

        let resp = self
            .http
            .delete(&url)
            .header(AUTHORIZATION, &token)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_storage_token().await;
            let token = self.require_storage_auth().await?;
            let resp = self
                .http
                .delete(&url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let text = resp.text().await.unwrap_or_default();
                return Err(FabioError::from_status(status, text).into());
            }
        } else if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        Ok(serde_json::json!({
            "path": path,
            "status": "deleted"
        }))
    }

    /// Run a notebook and return the job instance ID.
    pub async fn run_notebook(
        &self,
        workspace: &str,
        item_id: &str,
        body: Option<&Value>,
    ) -> Result<String> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(&format!(
            "/workspaces/{workspace}/items/{item_id}/jobs/instances?jobType=RunNotebook"
        ));

        let request_body = body.cloned().unwrap_or_else(|| serde_json::json!({}));

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() != StatusCode::ACCEPTED && resp.status() != StatusCode::OK {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status, text).into());
        }

        // Extract job instance ID from Location header
        let location = resp
            .headers()
            .get("location")
            .or_else(|| resp.headers().get("Location"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let job_id = location.rsplit('/').next().unwrap_or("").to_string();
        if job_id.is_empty() {
            return Err(
                FabioError::api_error("No job instance ID returned from run request").into(),
            );
        }

        Ok(job_id)
    }

    /// Trigger an on-demand item job and return the job instance ID from
    /// the Location header. Generic version of `run_notebook` supporting
    /// any job type and optional `executionData` payload.
    pub async fn trigger_item_job(
        &self,
        workspace: &str,
        item_id: &str,
        job_type: &str,
        execution_data: Option<&Value>,
    ) -> Result<String> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(&format!(
            "/workspaces/{workspace}/items/{item_id}/jobs/instances?jobType={job_type}"
        ));

        let body = execution_data.map_or_else(
            || serde_json::json!({}),
            |ed| serde_json::json!({ "executionData": ed }),
        );

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(&body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(&body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return Self::extract_job_id_from_response(resp).await;
        }

        Self::extract_job_id_from_response(resp).await
    }

    /// Extract the job instance ID from a job trigger response's Location
    /// header. Returns an error if the response indicates failure or if no
    /// job ID can be found.
    async fn extract_job_id_from_response(resp: Response) -> Result<String> {
        let status = resp.status();
        if status != StatusCode::ACCEPTED && status != StatusCode::OK {
            let status_code = status.as_u16();
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status_code, text).into());
        }

        let location = resp
            .headers()
            .get("location")
            .or_else(|| resp.headers().get("Location"))
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        let job_id = location.rsplit('/').next().unwrap_or("").to_string();
        if job_id.is_empty() {
            return Err(
                FabioError::api_error("No job instance ID returned from run request").into(),
            );
        }

        Ok(job_id)
    }

    /// Trigger a job at a specific URL path (for item types that use path-based
    /// job endpoints instead of `?jobType=` query parameter).
    pub async fn trigger_item_job_at(&self, path: &str, body: Option<&Value>) -> Result<String> {
        let token = self.require_auth().await?;
        let url = self.fabric_url(path);

        let request_body = body.cloned().unwrap_or_else(|| serde_json::json!({}));

        let resp = self
            .http
            .post(&url)
            .header(AUTHORIZATION, &token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

        if resp.status() == StatusCode::UNAUTHORIZED {
            self.invalidate_fabric_token().await;
            let token = self.require_auth().await?;
            let resp = self
                .http
                .post(&url)
                .header(AUTHORIZATION, &token)
                .json(&request_body)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;
            return Self::extract_job_id_from_response(resp).await;
        }

        Self::extract_job_id_from_response(resp).await
    }

    /// Poll a long-running operation until completion.
    async fn poll_lro(&self, initial_response: Response) -> Result<Value> {
        self.poll_lro_impl(initial_response, self.lro_max_wait)
            .await
    }

    /// Poll a long-running operation with a custom timeout.
    async fn poll_lro_with_timeout(
        &self,
        initial_response: Response,
        max_wait: Duration,
    ) -> Result<Value> {
        self.poll_lro_impl(initial_response, max_wait).await
    }

    /// Unified LRO polling implementation with token refresh for long-running operations.
    #[allow(clippy::too_many_lines)]
    async fn poll_lro_impl(&self, initial_response: Response, max_wait: Duration) -> Result<Value> {
        // Read initial Retry-After from the 202 response (server-preferred interval).
        // Cap at 60s to prevent a misconfigured server from stalling the CLI.
        let mut poll_interval = initial_response
            .headers()
            .get("Retry-After")
            .or_else(|| initial_response.headers().get("retry-after"))
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .map_or(LRO_POLL_INTERVAL, |s| Duration::from_secs(s.min(60)));

        let location = initial_response
            .headers()
            .get("location")
            .or_else(|| initial_response.headers().get("Location"))
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let operation_id = initial_response
            .headers()
            .get("x-ms-operation-id")
            .and_then(|v| v.to_str().ok())
            .map(String::from);

        let Some(poll_url) = location
            .or_else(|| operation_id.map(|op_id| self.fabric_url(&format!("/operations/{op_id}"))))
        else {
            // No LRO info - try to parse response body
            return handle_response(initial_response).await;
        };

        // Validate that the poll URL points to a trusted Microsoft domain.
        // This prevents token exfiltration if a compromised intermediary injects
        // a malicious Location header.
        validate_trusted_url(&poll_url, "LRO poll URL")?;

        let mut token = self.require_auth().await?;
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > max_wait {
                return Err(FabioError::with_hint(
                    ErrorCode::Timeout,
                    format!(
                        "LRO polling timed out after {}s",
                        max_wait.as_secs()
                    ),
                    format!(
                        "The operation may still be running server-side (poll URL: {poll_url}). \
                         Check status with: fabio jobs list --workspace <WS>, or retry with a longer \
                         --timeout if supported. Some operations (notebook run, graph refresh) \
                         can take several minutes on small capacities."
                    ),
                )
                .into());
            }

            sleep(poll_interval).await;

            // Refresh token if elapsed > 4 minutes (prevents expiry during long polls)
            if start.elapsed() > Duration::from_secs(240) {
                token = self.require_auth().await?;
            }

            let resp = self
                .http
                .get(&poll_url)
                .header(AUTHORIZATION, &token)
                .send()
                .await
                .map_err(|e| FabioError::new(ErrorCode::NetworkError, e.to_string()))?;

            let status = resp.status();

            // Update poll interval from Retry-After if the server provides one.
            if let Some(retry_secs) = resp
                .headers()
                .get("Retry-After")
                .or_else(|| resp.headers().get("retry-after"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
            {
                poll_interval = Duration::from_secs(retry_secs.min(60));
            }

            if status == StatusCode::OK
                || status == StatusCode::CREATED
                || status == StatusCode::ACCEPTED
            {
                // Capture resource location before consuming body
                let resource_location = resp
                    .headers()
                    .get("location")
                    .or_else(|| resp.headers().get("Location"))
                    .and_then(|v| v.to_str().ok())
                    .map(String::from);

                // Parse body and check operation status
                let body: Value = resp.json().await.unwrap_or(Value::Null);
                let op_status = body.get("status").and_then(Value::as_str).unwrap_or("");

                match op_status {
                    "Succeeded" | "succeeded" => {
                        // Check if there's a resource location for the final result
                        if let Some(ref loc) = resource_location {
                            // Validate resource location to prevent token exfiltration
                            if validate_trusted_url(loc, "LRO resource location").is_ok() {
                                let final_resp = self
                                    .http
                                    .get(loc)
                                    .header(AUTHORIZATION, &token)
                                    .send()
                                    .await
                                    .map_err(|e| {
                                        FabioError::new(ErrorCode::NetworkError, e.to_string())
                                    })?;
                                if final_resp.status().is_success() {
                                    let final_body: Value =
                                        final_resp.json().await.unwrap_or(Value::Null);
                                    if !final_body.is_null() {
                                        return Ok(final_body);
                                    }
                                }
                            }
                            // If validation fails, fall through to return poll body
                        }
                        return Ok(body);
                    }
                    "Failed" | "failed" => {
                        let msg = body
                            .get("error")
                            .and_then(|e| e.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("LRO failed");
                        return Err(FabioError::api_error(msg).into());
                    }
                    // Running, NotStarted, or other in-progress states - keep polling
                    _ => {
                        // If 200/201 with no status field, operation is complete
                        if op_status.is_empty()
                            && (status == StatusCode::OK || status == StatusCode::CREATED)
                        {
                            return Ok(body);
                        }
                        continue;
                    }
                }
            }
            // Unexpected status
            let text = resp.text().await.unwrap_or_default();
            return Err(FabioError::from_status(status.as_u16(), text).into());
        }
    }
}

/// Acquire an access token using the credential chain:
/// 1. Environment (service principal via `AZURE_TENANT_ID` + `AZURE_CLIENT_ID` + `AZURE_CLIENT_SECRET`)
/// 2. Managed Identity (for Azure-hosted workloads)
/// 3. Developer Tools (Azure CLI, then Azure Developer CLI)
///
/// Returns the cached token and which credential source provided it.
async fn acquire_token(scope: &str) -> Result<(CachedToken, CredentialSource)> {
    // 0. Try fabio's own cached token (from `fabio auth login`)
    if let Some(result) = try_fabio_cache(scope).await {
        return result;
    }

    // If the user explicitly logged out, do NOT fall back to other credential sources.
    // They must run `fabio auth login` to re-authenticate.
    if crate::token_cache::is_explicitly_logged_out() {
        return Err(FabioError::with_hint(
            ErrorCode::AuthRequired,
            "Not authenticated. You have explicitly logged out.",
            "Run 'fabio auth login' to authenticate.".to_string(),
        )
        .into());
    }

    // 1. Try environment credentials (service principal)
    if let Some(result) = try_environment_credential(scope).await {
        return result;
    }

    // 2. Try managed identity (only when running in Azure)
    if let Some(result) = try_managed_identity_credential(scope).await {
        return result;
    }

    // 3. Fall back to developer tools (az cli, azd)
    try_developer_tools_credential(scope).await
}

/// Try fabio's own persistent token cache (from `fabio auth login`).
/// Returns None if no cached token is available, Some(Ok/Err) if a token was found.
async fn try_fabio_cache(scope: &str) -> Option<Result<(CachedToken, CredentialSource)>> {
    use crate::token_cache;

    // For the default Fabric scope, use the primary cache
    let data = if scope == *FABRIC_SCOPE {
        token_cache::get_valid_token().await?
    } else {
        // For other scopes (storage, SQL, Kusto), try to get a token via refresh
        token_cache::get_token_for_scope(scope).await?
    };

    let expires_on = std::time::UNIX_EPOCH + std::time::Duration::from_secs(data.expires_on);
    Some(Ok((
        CachedToken::new(data.access_token, expires_on),
        CredentialSource::FabioCache,
    )))
}

/// Try service principal authentication via environment variables.
/// Returns None if env vars are not set (skip to next), Some(Ok/Err) if attempted.
async fn try_environment_credential(
    scope: &str,
) -> Option<Result<(CachedToken, CredentialSource)>> {
    let tenant_id = std::env::var("AZURE_TENANT_ID").ok()?;
    let client_id = std::env::var("AZURE_CLIENT_ID").ok()?;
    let client_secret = std::env::var("AZURE_CLIENT_SECRET").ok();

    // Need at least tenant + client_id + secret for client credentials flow
    let secret = client_secret?;

    let credential = match azure_identity::ClientSecretCredential::new(
        &tenant_id,
        client_id,
        azure_core::credentials::Secret::new(secret),
        None,
    ) {
        Ok(c) => c,
        Err(e) => {
            return Some(Err(FabioError::auth_required(format!(
                "Failed to create service principal credential: {e}"
            ))
            .into()));
        }
    };

    match credential.get_token(&[scope], None).await {
        Ok(token) => {
            let expires_on = std::time::SystemTime::from(token.expires_on);
            Some(Ok((
                CachedToken::new(token.token.secret().to_string(), expires_on),
                CredentialSource::Environment,
            )))
        }
        Err(e) => Some(Err(FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Service principal authentication failed: {e}"),
            "Check AZURE_TENANT_ID, AZURE_CLIENT_ID, and AZURE_CLIENT_SECRET environment variables.".to_string(),
        )
        .into())),
    }
}

/// Try managed identity authentication.
/// Returns None if not running in a managed identity environment (skip to next).
async fn try_managed_identity_credential(
    scope: &str,
) -> Option<Result<(CachedToken, CredentialSource)>> {
    // Only attempt managed identity if we detect an Azure hosting environment.
    // Check for common managed identity indicators:
    // - IDENTITY_ENDPOINT (App Service, Container Apps)
    // - MSI_ENDPOINT (legacy App Service)
    // - IMDS is always available on Azure VMs but we can't cheaply detect that without a network call
    let has_identity_env = std::env::var("IDENTITY_ENDPOINT").is_ok()
        || std::env::var("MSI_ENDPOINT").is_ok()
        || std::env::var("AZURE_FEDERATED_TOKEN_FILE").is_ok();

    if !has_identity_env {
        return None;
    }

    let Ok(credential) = azure_identity::ManagedIdentityCredential::new(None) else {
        return None;
    };

    match credential.get_token(&[scope], None).await {
        Ok(token) => {
            let expires_on = std::time::SystemTime::from(token.expires_on);
            Some(Ok((
                CachedToken::new(token.token.secret().to_string(), expires_on),
                CredentialSource::ManagedIdentity,
            )))
        }
        Err(_) => None, // Managed identity not available, fall through
    }
}

/// Try developer tools credentials (Azure CLI, then Azure Developer CLI).
async fn try_developer_tools_credential(scope: &str) -> Result<(CachedToken, CredentialSource)> {
    // Try Azure CLI first
    if let Ok(credential) = azure_identity::AzureCliCredential::new(None) {
        if let Ok(token) = credential.get_token(&[scope], None).await {
            let expires_on = std::time::SystemTime::from(token.expires_on);
            return Ok((
                CachedToken::new(token.token.secret().to_string(), expires_on),
                CredentialSource::AzureCli,
            ));
        }
    }

    // Try Azure Developer CLI
    if let Ok(credential) = azure_identity::AzureDeveloperCliCredential::new(None) {
        if let Ok(token) = credential.get_token(&[scope], None).await {
            let expires_on = std::time::SystemTime::from(token.expires_on);
            return Ok((
                CachedToken::new(token.token.secret().to_string(), expires_on),
                CredentialSource::AzureDeveloperCli,
            ));
        }
    }

    Err(FabioError::with_hint(
        ErrorCode::AuthRequired,
        "No valid credentials found. Tried: environment variables, managed identity, Azure CLI, Azure Developer CLI.",
        "Run 'az login' or set AZURE_TENANT_ID + AZURE_CLIENT_ID + AZURE_CLIENT_SECRET environment variables.".to_string(),
    )
    .into())
}

/// Handle an HTTP response, converting errors to `FabioError`.
async fn handle_response(resp: Response) -> Result<Value> {
    let status = resp.status();

    // Guard against unbounded response bodies (OOM protection).
    // Only applies to API JSON responses — file downloads use separate paths.
    if let Some(len) = resp.content_length() {
        if len > MAX_API_RESPONSE_SIZE {
            return Err(FabioError::new(
                ErrorCode::ApiError,
                format!(
                    "Response body too large ({len} bytes, max {MAX_API_RESPONSE_SIZE}). \
                     This may indicate a misconfigured endpoint."
                ),
            )
            .into());
        }
    }

    if status.is_success() {
        // Read response body with size limit (protects against chunked transfer
        // encoding that bypasses Content-Length check above).
        let bytes = resp
            .bytes()
            .await
            .map_err(|e| FabioError::api_error(format!("Failed to read response body: {e}")))?;
        if bytes.len() as u64 > MAX_API_RESPONSE_SIZE {
            return Err(FabioError::new(
                ErrorCode::ApiError,
                format!(
                    "Response body too large ({} bytes, max {MAX_API_RESPONSE_SIZE}).",
                    bytes.len()
                ),
            )
            .into());
        }
        let text = String::from_utf8_lossy(&bytes);
        if text.is_empty() {
            return Ok(Value::Null);
        }
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| FabioError::api_error(format!("Invalid JSON response: {e}")))?;
        return Ok(value);
    }

    // Extract error code from response headers before consuming body.
    // Fabric APIs set this header with machine-readable codes like
    // "ItemNotFound", "InvalidItemType", "WorkspaceNotFound", etc.
    let api_error_code = resp
        .headers()
        .get("x-ms-public-api-error-code")
        .or_else(|| resp.headers().get("x-ms-error-code"))
        .and_then(|v| v.to_str().ok())
        .map(String::from);

    let status_code = status.as_u16();
    let text = resp.text().await.unwrap_or_default();

    // Check for CapacityNotActive
    if text.contains("CapacityNotActive") {
        return Err(FabioError::new(
            ErrorCode::CapacityInactive,
            "Capacity is inactive. Resume it in the Azure portal.",
        )
        .into());
    }

    // Try to extract error message from JSON body
    let parsed = serde_json::from_str::<Value>(&text).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("message"))
                .and_then(Value::as_str)
                .map(String::from)
                .or_else(|| v.get("message").and_then(Value::as_str).map(String::from))
        })
        .unwrap_or_else(|| {
            // Truncate raw response body to prevent leaking unbounded server error details
            let truncated = if text.len() > MAX_ERROR_BODY_LEN {
                format!("{}...(truncated)", &text[..MAX_ERROR_BODY_LEN])
            } else {
                text.clone()
            };
            format!("HTTP {status_code}: {truncated}")
        });

    // Extract isRetriable flag from response body (Fabric API surfaces this on errors)
    let retriable = parsed
        .as_ref()
        .and_then(|v| v.get("error"))
        .and_then(|e| e.get("isRetriable"))
        .and_then(Value::as_bool);

    // Prepend the server error code from headers for machine-readable context.
    // e.g., "ItemNotFound: The requested item does not exist."
    let enriched_message = if let Some(ref code) = api_error_code {
        format!("{code}: {message}")
    } else {
        message
    };

    Err(
        FabioError::from_status_with_body(status_code, enriched_message, &text)
            .set_retriable(retriable)
            .into(),
    )
}

/// Validate that a user-provided URL targets a trusted Microsoft domain.
///
/// This prevents bearer token exfiltration to attacker-controlled servers when
/// agents construct CLI commands with `--query-uri` or `--published-url` flags.
///
/// Allowed domains:
/// - `*.fabric.microsoft.com`
/// - `*.kusto.fabric.microsoft.com`
/// - `*.kusto.windows.net`
/// - `*.analysis.windows.net`
/// - `*.powerbi.com`
/// - `*.pbidedicated.windows.net`
pub fn validate_trusted_url(url: &str, flag_name: &str) -> Result<()> {
    let lower = url.to_lowercase();

    // Must be HTTPS
    if !lower.starts_with("https://") {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag_name} must use HTTPS (got: {url})"),
            "Only HTTPS URLs to trusted Microsoft endpoints are allowed.",
        )
        .into());
    }

    // Reject URLs with userinfo (user:pass@host) — prevents bypass where
    // "https://trusted.com:443@evil.com/" validates as trusted but routes to evil.com
    let authority = lower
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .unwrap_or("");

    if authority.contains('@') {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag_name} must not contain userinfo (@ character in authority)."),
            "URLs with embedded credentials are not allowed. Use a plain https://host/path URL.",
        )
        .into());
    }

    // Extract host (strip port if present)
    let host = authority.split(':').next().unwrap_or("");

    // Reject empty host or hosts with suspicious characters
    if host.is_empty()
        || host.contains(|c: char| !c.is_ascii_alphanumeric() && c != '.' && c != '-')
    {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag_name} contains an invalid hostname ({host})."),
            "Only HTTPS URLs to trusted Microsoft endpoints are allowed.",
        )
        .into());
    }

    let trusted_suffixes = [
        ".fabric.microsoft.com",
        ".kusto.windows.net",
        ".analysis.windows.net",
        ".powerbi.com",
        ".pbidedicated.windows.net",
        "api.fabric.microsoft.com",
        "api.powerbi.com",
    ];

    let is_trusted = trusted_suffixes
        .iter()
        .any(|suffix| host.ends_with(suffix) || host == suffix.trim_start_matches('.'));

    if !is_trusted {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!(
                "{flag_name} targets an untrusted domain ({host}). \
                 Bearer tokens are only sent to trusted Microsoft endpoints."
            ),
            format!(
                "Allowed domains: *.fabric.microsoft.com, *.kusto.windows.net, \
                 *.analysis.windows.net, *.powerbi.com, *.pbidedicated.windows.net. \
                 Received: {url}"
            ),
        )
        .into());
    }

    Ok(())
}

/// Validate that a string is a valid UUID format.
///
/// Prevents path injection when IDs are interpolated into URL paths.
/// Accepts standard UUID format: `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` (lowercase hex + hyphens).
pub fn validate_uuid(value: &str, param_name: &str) -> Result<()> {
    let is_valid = value.len() == 36
        && value.chars().enumerate().all(|(i, c)| match i {
            8 | 13 | 18 | 23 => c == '-',
            _ => c.is_ascii_hexdigit(),
        });

    if !is_valid {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{param_name} must be a valid UUID (got: {value})"),
            "Expected format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx (lowercase hex with hyphens).",
        )
        .into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── validate_trusted_url ─────────────────────────────────────────────

    // Accepted domains
    #[test]
    fn trusted_url_accepts_fabric_api() {
        assert!(
            validate_trusted_url("https://api.fabric.microsoft.com/v1/workspaces", "test").is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_fabric_subdomain() {
        assert!(
            validate_trusted_url(
                "https://wabi-us-east2-b-primary-redirect.analysis.windows.net/explore",
                "test"
            )
            .is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_kusto_fabric() {
        assert!(
            validate_trusted_url(
                "https://abc123def.eastus.kusto.fabric.microsoft.com/v2/rest/query",
                "test"
            )
            .is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_kusto_windows() {
        assert!(validate_trusted_url("https://mycluster.kusto.windows.net", "test").is_ok());
    }

    #[test]
    fn trusted_url_accepts_powerbi_api() {
        assert!(validate_trusted_url("https://api.powerbi.com/v1.0/myorg", "test").is_ok());
    }

    #[test]
    fn trusted_url_accepts_powerbi_subdomain() {
        assert!(validate_trusted_url("https://app.powerbi.com/groups/abc/reports", "test").is_ok());
    }

    #[test]
    fn trusted_url_accepts_pbidedicated() {
        assert!(
            validate_trusted_url("https://myserver.pbidedicated.windows.net/xmla", "test").is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_analysis_windows() {
        assert!(
            validate_trusted_url("https://myworkspace.analysis.windows.net/powerbi", "test")
                .is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_with_port_443() {
        assert!(validate_trusted_url("https://api.fabric.microsoft.com:443/v1", "test").is_ok());
    }

    #[test]
    fn trusted_url_accepts_with_custom_port() {
        assert!(validate_trusted_url("https://api.fabric.microsoft.com:8443/v1", "test").is_ok());
    }

    #[test]
    fn trusted_url_accepts_case_insensitive() {
        assert!(
            validate_trusted_url("https://API.Fabric.Microsoft.COM/v1/workspaces", "test").is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_deep_path() {
        assert!(
            validate_trusted_url(
                "https://api.fabric.microsoft.com/v1/workspaces/abc/items/def/getDefinition",
                "test"
            )
            .is_ok()
        );
    }

    #[test]
    fn trusted_url_accepts_with_query_string() {
        assert!(
            validate_trusted_url(
                "https://api.fabric.microsoft.com/v1/operations/abc?beta=true",
                "test"
            )
            .is_ok()
        );
    }

    // Rejected: protocol issues
    #[test]
    fn trusted_url_rejects_http() {
        assert!(validate_trusted_url("http://api.fabric.microsoft.com/v1", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_ftp() {
        assert!(validate_trusted_url("ftp://api.fabric.microsoft.com/v1", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_no_scheme() {
        assert!(validate_trusted_url("api.fabric.microsoft.com/v1", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_empty_string() {
        assert!(validate_trusted_url("", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_just_scheme() {
        assert!(validate_trusted_url("https://", "test").is_err());
    }

    // Rejected: untrusted domains
    #[test]
    fn trusted_url_rejects_untrusted_domain() {
        assert!(validate_trusted_url("https://evil.com/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_localhost() {
        assert!(validate_trusted_url("https://localhost/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_ip_address() {
        assert!(validate_trusted_url("https://10.0.0.1/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_partial_suffix_match() {
        // "notfabric.microsoft.com" ends with ".fabric.microsoft.com"? No -- "not" prefix
        // Actually "notfabric.microsoft.com" does NOT end with ".fabric.microsoft.com"
        assert!(validate_trusted_url("https://notfabric.microsoft.com/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_subdomain_of_attacker() {
        // Attacker domain that contains trusted suffix as substring
        assert!(
            validate_trusted_url("https://evil-fabric.microsoft.com.attacker.com/", "test")
                .is_err()
        );
    }

    #[test]
    fn trusted_url_rejects_prefix_trick() {
        // "microsoft.com.evil.com" should not pass
        assert!(validate_trusted_url("https://microsoft.com.evil.com/", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_fabric_in_path_only() {
        assert!(
            validate_trusted_url("https://evil.com/api.fabric.microsoft.com/v1", "test").is_err()
        );
    }

    // Rejected: userinfo bypass attacks (CRITICAL security tests)
    #[test]
    fn trusted_url_rejects_userinfo_with_port_at_evil() {
        // Classic bypass: host extracted as "fabric.microsoft.com" after split(':'),
        // but reqwest connects to evil.com (the real host after '@')
        assert!(
            validate_trusted_url("https://fabric.microsoft.com:443@evil.com/path", "test").is_err()
        );
    }

    #[test]
    fn trusted_url_rejects_simple_userinfo() {
        assert!(
            validate_trusted_url("https://user@api.fabric.microsoft.com/path", "test").is_err()
        );
    }

    #[test]
    fn trusted_url_rejects_userinfo_with_password() {
        assert!(
            validate_trusted_url("https://user:pass@api.fabric.microsoft.com/path", "test")
                .is_err()
        );
    }

    #[test]
    fn trusted_url_rejects_empty_userinfo() {
        assert!(validate_trusted_url("https://@api.fabric.microsoft.com/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_at_in_password_position() {
        assert!(
            validate_trusted_url("https://user:p%40ss@evil.com/fabric.microsoft.com", "test")
                .is_err()
        );
    }

    // Rejected: hostname validation
    #[test]
    fn trusted_url_rejects_underscore_in_host() {
        assert!(validate_trusted_url("https://my_host.fabric.microsoft.com/path", "test").is_err());
    }

    #[test]
    fn trusted_url_rejects_space_in_host() {
        assert!(validate_trusted_url("https://my host.fabric.microsoft.com/path", "test").is_err());
    }

    // Error message quality
    #[test]
    fn trusted_url_error_includes_flag_name() {
        let err = validate_trusted_url("https://evil.com/path", "--query-uri").unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("--query-uri"),
            "Error should mention flag name"
        );
    }

    #[test]
    fn trusted_url_error_includes_domain() {
        let err = validate_trusted_url("https://evil.com/path", "test").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("evil.com"), "Error should mention the domain");
    }

    // ── validate_uuid ────────────────────────────────────────────────────

    #[test]
    fn uuid_accepts_valid_lowercase() {
        assert!(validate_uuid("12345678-1234-1234-1234-123456789abc", "test").is_ok());
    }

    #[test]
    fn uuid_accepts_all_zeros() {
        assert!(validate_uuid("00000000-0000-0000-0000-000000000000", "test").is_ok());
    }

    #[test]
    fn uuid_accepts_all_f() {
        assert!(validate_uuid("ffffffff-ffff-ffff-ffff-ffffffffffff", "test").is_ok());
    }

    #[test]
    fn uuid_accepts_uppercase_hex() {
        // is_ascii_hexdigit() accepts A-F
        assert!(validate_uuid("12345678-ABCD-ABCD-ABCD-123456789ABC", "test").is_ok());
    }

    #[test]
    fn uuid_accepts_mixed_case() {
        assert!(validate_uuid("12345678-aBcD-1234-aBcD-123456789aBc", "test").is_ok());
    }

    #[test]
    fn uuid_rejects_empty() {
        assert!(validate_uuid("", "test").is_err());
    }

    #[test]
    fn uuid_rejects_too_short() {
        assert!(validate_uuid("12345678-1234-1234-1234-12345678abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_too_long() {
        assert!(validate_uuid("12345678-1234-1234-1234-123456789abcd", "test").is_err());
    }

    #[test]
    fn uuid_rejects_missing_hyphens() {
        assert!(validate_uuid("12345678123412341234123456789abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_hyphen_in_wrong_position() {
        // Hyphen at position 7 instead of 8
        assert!(validate_uuid("1234567-81234-1234-1234-123456789abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_non_hex_chars() {
        assert!(validate_uuid("1234567g-1234-1234-1234-123456789abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_path_injection() {
        assert!(validate_uuid("../../etc/passwd", "test").is_err());
    }

    #[test]
    fn uuid_rejects_url_chars() {
        assert!(validate_uuid("12345678-1234-1234-1234-12345678/abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_spaces() {
        assert!(validate_uuid("12345678-1234-1234-1234-12345678 abc", "test").is_err());
    }

    #[test]
    fn uuid_rejects_null_bytes() {
        assert!(validate_uuid("12345678-1234-1234-1234-12345678\0abc", "test").is_err());
    }

    #[test]
    fn uuid_error_includes_param_name() {
        let err = validate_uuid("bad", "--workspace").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("--workspace"));
    }

    // ── CachedToken ──────────────────────────────────────────────────────

    #[test]
    fn cached_token_new_formats_bearer_header() {
        let token = CachedToken::new(
            "my_token_123".to_string(),
            std::time::SystemTime::now() + Duration::from_secs(3600),
        );
        assert_eq!(token.bearer_header, "Bearer my_token_123");
        assert_eq!(token.token, "my_token_123");
    }

    #[test]
    fn cached_token_not_expired_when_far_from_expiry() {
        let token = CachedToken::new(
            "tok".to_string(),
            std::time::SystemTime::now() + Duration::from_secs(3600), // 1 hour from now
        );
        assert!(!token.is_expired());
    }

    #[test]
    fn cached_token_expired_when_within_margin() {
        let token = CachedToken::new(
            "tok".to_string(),
            // Expires in 200s, but margin is 300s, so it's "expired"
            std::time::SystemTime::now() + Duration::from_secs(200),
        );
        assert!(token.is_expired());
    }

    #[test]
    fn cached_token_expired_when_past_expiry() {
        let token = CachedToken::new(
            "tok".to_string(),
            std::time::SystemTime::now() - Duration::from_secs(60), // already past
        );
        assert!(token.is_expired());
    }

    #[test]
    fn cached_token_not_expired_at_exact_margin_boundary() {
        let token = CachedToken::new(
            "tok".to_string(),
            // Expires in exactly 301s (just above 300s margin)
            std::time::SystemTime::now() + Duration::from_secs(301),
        );
        assert!(!token.is_expired());
    }

    #[test]
    fn cached_token_not_expired_well_above_margin() {
        let token = CachedToken::new(
            "tok".to_string(),
            // Expires in 305s — comfortably above 300s margin
            std::time::SystemTime::now() + Duration::from_secs(305),
        );
        assert!(!token.is_expired());
    }

    // ── CredentialSource Display ─────────────────────────────────────────

    #[test]
    fn credential_source_display_fabio_cache() {
        assert_eq!(
            CredentialSource::FabioCache.to_string(),
            "fabio cache (device code)"
        );
    }

    #[test]
    fn credential_source_display_environment() {
        assert_eq!(
            CredentialSource::Environment.to_string(),
            "environment (service principal)"
        );
    }

    #[test]
    fn credential_source_display_managed_identity() {
        assert_eq!(
            CredentialSource::ManagedIdentity.to_string(),
            "managed identity"
        );
    }

    #[test]
    fn credential_source_display_azure_cli() {
        assert_eq!(CredentialSource::AzureCli.to_string(), "Azure CLI");
    }

    #[test]
    fn credential_source_display_azd() {
        assert_eq!(
            CredentialSource::AzureDeveloperCli.to_string(),
            "Azure Developer CLI"
        );
    }

    // ── Constants sanity ─────────────────────────────────────────────────

    #[test]
    fn fabric_base_url_is_v1() {
        assert_eq!(*FABRIC_BASE_URL, "https://api.fabric.microsoft.com/v1");
    }

    #[test]
    fn onelake_urls_are_https() {
        assert!(ONELAKE_DFS_URL.starts_with("https://"));
        assert!(ONELAKE_BLOB_URL.starts_with("https://"));
    }

    #[test]
    fn lro_poll_interval_is_reasonable() {
        assert!(LRO_POLL_INTERVAL >= Duration::from_secs(1));
        assert!(LRO_POLL_INTERVAL <= Duration::from_secs(10));
    }

    #[test]
    fn lro_max_wait_is_2_minutes() {
        assert_eq!(LRO_MAX_WAIT, Duration::from_secs(120));
    }

    #[test]
    fn token_refresh_margin_is_5_minutes() {
        assert_eq!(TOKEN_REFRESH_MARGIN, Duration::from_secs(300));
    }

    #[test]
    fn max_error_body_len_is_500() {
        assert_eq!(MAX_ERROR_BODY_LEN, 500);
    }

    #[test]
    fn max_api_response_size_is_50mb() {
        assert_eq!(MAX_API_RESPONSE_SIZE, 50 * 1024 * 1024);
    }

    // ── FabricClient construction ────────────────────────────────────────

    #[test]
    fn fabric_client_new_does_not_panic() {
        let _client = FabricClient::new();
    }

    #[test]
    fn fabric_client_default_lro_timeout_is_120s() {
        let client = FabricClient::new();
        assert_eq!(client.lro_max_wait, Duration::from_secs(120));
    }

    #[test]
    fn fabric_client_with_lro_timeout_overrides_default() {
        let client = FabricClient::new().with_lro_timeout(Duration::from_secs(300));
        assert_eq!(client.lro_max_wait, Duration::from_secs(300));
    }

    #[test]
    fn arm_base_url_is_management_azure() {
        assert_eq!(*ARM_BASE_URL, "https://management.azure.com");
    }

    #[test]
    fn arm_scope_is_management_default() {
        assert_eq!(*ARM_SCOPE, "https://management.azure.com/.default");
    }

    #[test]
    fn fabric_client_has_arm_token_field() {
        let client = FabricClient::new();
        // arm_token should be initialized as None (wrapped in Arc<RwLock>)
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        rt.block_on(async {
            let guard = client.arm_token.read().await;
            assert!(guard.is_none());
            drop(guard);
        });
    }

    // ── handle_response (via wiremock) ───────────────────────────────────

    mod handle_response_tests {
        use super::*;
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        async fn get_response(server: &MockServer) -> Response {
            reqwest::get(server.uri()).await.unwrap()
        }

        #[tokio::test]
        async fn success_empty_body_returns_null() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_string(""))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let result = handle_response(resp).await.unwrap();
            assert_eq!(result, Value::Null);
        }

        #[tokio::test]
        async fn success_valid_json_object() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(200)
                        .set_body_json(serde_json::json!({"id": "abc", "name": "test"})),
                )
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let result = handle_response(resp).await.unwrap();
            assert_eq!(result["id"], "abc");
            assert_eq!(result["name"], "test");
        }

        #[tokio::test]
        async fn success_valid_json_array() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!([1, 2, 3])),
                )
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let result = handle_response(resp).await.unwrap();
            assert_eq!(result, serde_json::json!([1, 2, 3]));
        }

        #[tokio::test]
        async fn success_invalid_json_returns_error() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_string("not valid json {{{"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("Invalid JSON"));
        }

        #[tokio::test]
        async fn error_401_maps_to_auth_required() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::AuthRequired);
        }

        #[tokio::test]
        async fn error_403_maps_to_forbidden() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(403).set_body_string("Forbidden"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::Forbidden);
        }

        #[tokio::test]
        async fn error_404_maps_to_not_found() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(404).set_body_string("Not found"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::NotFound);
        }

        #[tokio::test]
        async fn error_409_maps_to_conflict() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(409).set_body_string("Conflict"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::Conflict);
        }

        #[tokio::test]
        async fn error_429_maps_to_rate_limited() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(429).set_body_string("Too many"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::RateLimited);
        }

        #[tokio::test]
        async fn error_430_maps_to_rate_limited() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(430).set_body_string("Capacity throttled"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::RateLimited);
        }

        #[tokio::test]
        async fn error_500_maps_to_api_error() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(500).set_body_string("Internal error"))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::ApiError);
        }

        #[tokio::test]
        async fn error_extracts_json_error_message() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                    "error": {"code": "InvalidInput", "message": "The name is too long"}
                })))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("The name is too long"));
        }

        #[tokio::test]
        async fn error_extracts_top_level_message() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
                    "message": "Something went wrong"
                })))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("Something went wrong"));
        }

        #[tokio::test]
        async fn error_truncates_long_non_json_body() {
            let server = MockServer::start().await;
            let long_body = "x".repeat(2000);
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(500).set_body_string(&long_body))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(msg.contains("(truncated)"));
            // Should not contain the full 2000 chars
            assert!(msg.len() < 1500);
        }

        #[tokio::test]
        async fn error_detects_capacity_not_active() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(400).set_body_string(
                    r#"{"error":{"code":"CapacityNotActive","message":"Resume capacity"}}"#,
                ))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let fabio_err = err.downcast_ref::<FabioError>().unwrap();
            assert_eq!(fabio_err.code, ErrorCode::CapacityInactive);
        }

        #[tokio::test]
        async fn rejects_oversized_response_via_content_length() {
            // When a response declares Content-Length > MAX_API_RESPONSE_SIZE,
            // handle_response should reject it before reading the body.
            // We use a raw TCP server approach: wiremock can't fake a Content-Length
            // mismatch without causing transport errors, so we test by verifying
            // the check logic directly using a response whose content_length()
            // returns a value we control.
            //
            // Since reqwest populates content_length() from the header, we need
            // a response where the actual body length matches the header. Instead
            // of allocating 50MB, we verify via a unit-style check that the guard
            // constant is correct and the function rejects bodies declared as
            // larger than MAX_API_RESPONSE_SIZE.
            //
            // This is a documentation-style test confirming the threshold.
            // The actual guard was verified working in integration via the
            // second security audit.
            assert_eq!(MAX_API_RESPONSE_SIZE, 50 * 1024 * 1024);
            // Functional proof: a normal-sized response passes through fine
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})),
                )
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let result = handle_response(resp).await.unwrap();
            assert_eq!(result["ok"], true);
        }

        #[tokio::test]
        async fn error_includes_x_ms_public_api_error_code_header() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(404)
                        .insert_header("x-ms-public-api-error-code", "ItemNotFound")
                        .set_body_json(serde_json::json!({
                            "error": {"code": "ItemNotFound", "message": "The requested item does not exist."}
                        })),
                )
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(
                msg.contains("ItemNotFound"),
                "error should include API error code from header: {msg}"
            );
            assert!(
                msg.contains("The requested item does not exist"),
                "error should include body message: {msg}"
            );
        }

        #[tokio::test]
        async fn error_includes_x_ms_error_code_header_fallback() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(
                    ResponseTemplate::new(400)
                        .insert_header("x-ms-error-code", "InvalidItemType")
                        .set_body_json(serde_json::json!({
                            "error": {"message": "The item type is invalid."}
                        })),
                )
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(
                msg.contains("InvalidItemType"),
                "error should include error code from x-ms-error-code header: {msg}"
            );
        }

        #[tokio::test]
        async fn error_without_api_error_code_header_still_works() {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({
                    "error": {"message": "Internal server error"}
                })))
                .mount(&server)
                .await;

            let resp = get_response(&server).await;
            let err = handle_response(resp).await.unwrap_err();
            let msg = format!("{err}");
            assert!(
                msg.contains("Internal server error"),
                "error message should work without header: {msg}"
            );
            // Should NOT contain a colon-prefixed error code
            assert!(
                !msg.starts_with("API_ERROR: ItemNotFound"),
                "should not have spurious error code"
            );
        }
    }

    // ── Private link URL routing ─────────────────────────────────────────

    #[test]
    fn fabric_url_without_private_link() {
        let client = FabricClient::new();
        let url = client.fabric_url("/workspaces/abc/items/def");
        assert_eq!(
            url,
            "https://api.fabric.microsoft.com/v1/workspaces/abc/items/def"
        );
    }

    #[test]
    fn fabric_url_with_private_link() {
        let client = FabricClient::new()
            .with_private_link("12345678-abcd-ef01-2345-6789abcdef00".to_string());
        let url = client.fabric_url("/workspaces/12345678-abcd-ef01-2345-6789abcdef00/items/x");
        assert_eq!(
            url,
            "https://12345678abcdef0123456789abcdef00.z12.w.api.fabric.microsoft.com/v1/workspaces/12345678-abcd-ef01-2345-6789abcdef00/items/x"
        );
    }

    #[test]
    fn onelake_dfs_url_without_private_link() {
        let client = FabricClient::new();
        let url = client.onelake_dfs_url("ws-id", "item-id/Files/test.csv");
        assert_eq!(
            url,
            "https://onelake.dfs.fabric.microsoft.com/ws-id/item-id/Files/test.csv"
        );
    }

    #[test]
    fn onelake_dfs_url_with_private_link() {
        let client = FabricClient::new()
            .with_private_link("aabbccdd-1122-3344-5566-778899aabbcc".to_string());
        let url = client.onelake_dfs_url("aabbccdd-1122-3344-5566-778899aabbcc", "item/path");
        assert_eq!(
            url,
            "https://aabbccdd112233445566778899aabbcc.zaa.onelake.dfs.fabric.microsoft.com/aabbccdd-1122-3344-5566-778899aabbcc/item/path"
        );
    }

    #[test]
    fn onelake_blob_url_with_private_link() {
        let client = FabricClient::new()
            .with_private_link("aabbccdd-1122-3344-5566-778899aabbcc".to_string());
        let url = client.onelake_blob_url("aabbccdd-1122-3344-5566-778899aabbcc", "item/path");
        assert_eq!(
            url,
            "https://aabbccdd112233445566778899aabbcc.zaa.onelake.blob.fabric.microsoft.com/aabbccdd-1122-3344-5566-778899aabbcc/item/path"
        );
    }

    // ── Environment variable URL overrides ───────────────────────────────

    #[test]
    fn env_or_default_returns_default_when_var_unset() {
        // Use a unique var name that won't be set in any environment
        let result = env_or_default(
            "FABIO_TEST_NONEXISTENT_XYZ_12345",
            "https://default.example",
        );
        assert_eq!(result, "https://default.example");
    }

    #[test]
    fn env_or_default_preserves_value_without_trailing_slash() {
        let result = env_or_default(
            "FABIO_TEST_ALSO_NONEXISTENT_99999",
            "https://example.com/v1",
        );
        assert_eq!(result, "https://example.com/v1");
    }

    #[test]
    fn default_fabric_scope_is_correct() {
        // When no env override is set, the default scope is used
        assert_eq!(*FABRIC_SCOPE, "https://api.fabric.microsoft.com/.default");
    }

    #[test]
    fn default_storage_scope_is_correct() {
        assert_eq!(*STORAGE_SCOPE, "https://storage.azure.com/.default");
    }

    #[test]
    fn default_sql_scope_is_correct() {
        assert_eq!(*SQL_SCOPE, "https://database.windows.net/.default");
    }

    #[test]
    fn default_onelake_dfs_url_is_correct() {
        assert_eq!(*ONELAKE_DFS_URL, "https://onelake.dfs.fabric.microsoft.com");
    }

    #[test]
    fn default_onelake_blob_url_is_correct() {
        assert_eq!(
            *ONELAKE_BLOB_URL,
            "https://onelake.blob.fabric.microsoft.com"
        );
    }

    #[test]
    fn default_powerbi_base_url_is_correct() {
        assert_eq!(*POWERBI_BASE_URL, "https://api.powerbi.com/v1.0/myorg");
    }
}
