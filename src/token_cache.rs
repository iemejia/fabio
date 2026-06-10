//! Persistent `OAuth2` token cache for fabio's own authentication.
//!
//! Stores access and refresh tokens at `~/.fabio/token_cache.json`.
//! - **Windows**: Encrypted with DPAPI (`CryptProtectData`, user scope) — only
//!   the current Windows user can decrypt. Matches Azure CLI behavior.
//! - **Linux/macOS**: Plaintext JSON with `0600` file permissions (owner-only).
//!   Matches Azure CLI behavior on these platforms.
//!
//! Supports the Microsoft Identity Platform device code flow and token refresh.

use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, FabioError};

/// Fabio CLI's own Entra ID app registration (multitenant, public client).
/// Users see "Fabio CLI" in the consent screen and audit logs — independent from
/// Azure CLI or Azure PowerShell identity.
const PUBLIC_CLIENT_ID: &str = "38715dcd-c115-46b4-8ed1-967d06c9ec6d";

/// Default tenant for multi-tenant auth.
const DEFAULT_TENANT: &str = "common";

/// Fabric API scope.
const FABRIC_SCOPE: &str = "https://api.fabric.microsoft.com/.default";

/// Margin before expiry to consider token stale and attempt refresh.
const REFRESH_MARGIN: Duration = Duration::from_secs(300); // 5 minutes

/// Cached token data persisted to disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenData {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix timestamp (seconds) when the access token expires.
    pub expires_on: u64,
    /// The tenant used for authentication.
    pub tenant: String,
    /// The scope used for authentication.
    pub scope: String,
}

impl TokenData {
    /// Check if the access token is expired or about to expire.
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.expires_on.saturating_sub(REFRESH_MARGIN.as_secs()) <= now
    }
}

/// Device code response from Microsoft Identity Platform.
#[derive(Debug, Deserialize)]
struct DeviceCodeResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    #[serde(default)]
    message: String,
    /// Polling interval in seconds.
    #[serde(default = "default_interval")]
    interval: u64,
    /// Lifetime of the device code in seconds.
    #[serde(default = "default_expires_in")]
    expires_in: u64,
}

const fn default_interval() -> u64 {
    5
}
const fn default_expires_in() -> u64 {
    900
}

/// Token response from Microsoft Identity Platform.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    /// Seconds until the access token expires.
    expires_in: u64,
}

/// Error response from the token endpoint during polling.
#[derive(Debug, Deserialize)]
struct TokenErrorResponse {
    error: String,
    #[serde(default)]
    error_description: String,
}

/// Returns the path to the token cache file.
fn cache_path() -> Result<PathBuf> {
    let home = home::home_dir().ok_or_else(|| {
        FabioError::new(
            ErrorCode::Unknown,
            "Cannot determine home directory for token cache.",
        )
    })?;
    Ok(home.join(".fabio").join("token_cache.json"))
}

/// Returns the path to the logout marker file.
fn logout_marker_path() -> Result<PathBuf> {
    let home = home::home_dir().ok_or_else(|| {
        FabioError::new(
            ErrorCode::Unknown,
            "Cannot determine home directory for token cache.",
        )
    })?;
    Ok(home.join(".fabio").join(".logged_out"))
}

/// Check if the user has explicitly logged out.
/// When true, the credential chain should NOT fall back to Azure CLI or other sources.
pub fn is_explicitly_logged_out() -> bool {
    logout_marker_path().ok().is_some_and(|p| p.exists())
}

/// Load cached token from disk.
/// On Windows, decrypts the DPAPI-encrypted blob before parsing.
pub fn load_cached_token() -> Option<TokenData> {
    let path = cache_path().ok()?;

    #[cfg(windows)]
    {
        let encrypted = std::fs::read(&path).ok()?;
        if encrypted.is_empty() {
            return None;
        }
        // Try DPAPI decryption first; fall back to plaintext for migration
        let json_bytes = dpapi_decrypt(&encrypted).ok().or_else(|| {
            // Legacy plaintext cache — try parsing directly for seamless upgrade
            Some(encrypted.clone())
        })?;
        let content = String::from_utf8(json_bytes).ok()?;
        serde_json::from_str(&content).ok()
    }

    #[cfg(not(windows))]
    {
        let content = std::fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }
}

/// Save token data to disk (creates ~/.fabio/ if needed).
/// On Windows, the token is encrypted with DPAPI before writing.
/// Also removes the logout marker if present.
pub fn save_token(data: &TokenData) -> Result<()> {
    let path = cache_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
        // Restrict directory permissions on Unix
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))?;
        }
    }
    let json = serde_json::to_string_pretty(data)?;

    // ── Platform-specific write ──────────────────────────────────────────
    #[cfg(unix)]
    {
        // Plaintext JSON with restricted permissions (matches az CLI on Linux/macOS)
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(json.as_bytes())?;
    }
    #[cfg(windows)]
    {
        // Encrypt with DPAPI (user scope) before writing — matches az CLI on Windows
        let encrypted = dpapi_encrypt(json.as_bytes())?;
        let temp_path = path.with_extension("tmp");
        std::fs::write(&temp_path, &encrypted)?;
        std::fs::rename(&temp_path, &path)?;
    }
    #[cfg(not(any(unix, windows)))]
    {
        // Fallback for other platforms: plaintext
        std::fs::write(&path, &json)?;
    }

    // Remove logout marker on successful login
    if let Ok(marker) = logout_marker_path() {
        if marker.exists() {
            std::fs::remove_file(&marker).ok();
        }
    }

    Ok(())
}

/// Delete the token cache file and write a logout marker.
pub fn clear_cache() -> Result<()> {
    let path = cache_path()?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }

    // Write logout marker so credential chain doesn't fall back to Azure CLI
    let marker = logout_marker_path()?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&marker, "")?;

    Ok(())
}

/// Attempt to get a valid access token from cache, refreshing if needed.
/// Returns None if no cache exists or refresh fails.
pub async fn get_valid_token() -> Option<TokenData> {
    let cached = load_cached_token()?;

    if !cached.is_expired() {
        return Some(cached);
    }

    // Try refreshing with the refresh token
    let refresh_token = cached.refresh_token.as_ref()?;
    refresh_access_token(refresh_token, &cached.tenant, &cached.scope)
        .await
        .ok()
        .inspect(|new_data| {
            save_token(new_data).ok();
        })
}

/// Attempt to get a valid access token for a specific scope (e.g., storage, SQL).
/// Uses the refresh token from the cached session to acquire a token for the requested scope.
pub async fn get_token_for_scope(scope: &str) -> Option<TokenData> {
    let cached = load_cached_token()?;
    let refresh_token = cached.refresh_token.as_ref()?;

    // If the cached token already covers this scope and is valid, return it
    if cached.scope == scope && !cached.is_expired() {
        return Some(cached);
    }

    // Use the refresh token to get a token for the specific scope
    refresh_access_token(refresh_token, &cached.tenant, scope)
        .await
        .ok()
}

/// Run the `OAuth2` device code flow interactively.
#[allow(clippy::too_many_lines)]
pub async fn device_code_login(tenant: Option<&str>, scope: Option<&str>) -> Result<TokenData> {
    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let scope = scope.unwrap_or(FABRIC_SCOPE);

    // Disable redirects on token endpoint client to prevent credential forwarding
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    // Step 1: Request device code
    let device_code_url =
        format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/devicecode");

    let resp = http
        .post(&device_code_url)
        .form(&[
            ("client_id", PUBLIC_CLIENT_ID),
            ("scope", &format!("{scope} offline_access")),
        ])
        .send()
        .await
        .map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Device code request failed: {e}"),
            )
        })?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Device code request failed: {text}"),
            "Check your network connection and tenant ID.".to_string(),
        )
        .into());
    }

    let dc: DeviceCodeResponse = resp.json().await.map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("Invalid device code response: {e}"),
        )
    })?;

    // Step 2: Display instructions to the user (validate URI is from Microsoft)
    let valid_verification_hosts = ["login.microsoftonline.com", "microsoft.com", "aka.ms"];
    let uri_lower = dc.verification_uri.to_lowercase();
    let uri_trusted = uri_lower.starts_with("https://")
        && valid_verification_hosts.iter().any(|host| {
            uri_lower.strip_prefix("https://").is_some_and(|rest| {
                let domain = rest.split('/').next().unwrap_or("");
                domain == *host || domain.ends_with(&format!(".{host}"))
            })
        });
    if !uri_trusted {
        return Err(FabioError::new(
            ErrorCode::AuthRequired,
            format!(
                "Device code verification URI is not a trusted Microsoft domain: {}",
                dc.verification_uri
            ),
        )
        .into());
    }

    if dc.message.is_empty() {
        eprintln!(
            "To sign in, open: {}\nEnter the code: {}",
            dc.verification_uri, dc.user_code
        );
    } else {
        eprintln!("{}", dc.message);
    }

    // Step 3: Poll for token
    let token_url = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token");
    let interval = Duration::from_secs(dc.interval.max(1));
    let deadline = SystemTime::now() + Duration::from_secs(dc.expires_in);

    loop {
        tokio::time::sleep(interval).await;

        if SystemTime::now() > deadline {
            return Err(FabioError::new(
                ErrorCode::Timeout,
                "Device code flow timed out. Please try again.",
            )
            .into());
        }

        let resp = http
            .post(&token_url)
            .form(&[
                ("client_id", PUBLIC_CLIENT_ID),
                ("device_code", dc.device_code.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await
            .map_err(|e| {
                FabioError::new(ErrorCode::NetworkError, format!("Token poll failed: {e}"))
            })?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            let token_resp: TokenResponse = serde_json::from_str(&body).map_err(|e| {
                FabioError::new(
                    ErrorCode::AuthRequired,
                    format!("Invalid token response: {e}"),
                )
            })?;

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();

            let data = TokenData {
                access_token: token_resp.access_token,
                refresh_token: token_resp.refresh_token,
                expires_on: now + token_resp.expires_in,
                tenant: tenant.to_string(),
                scope: scope.to_string(),
            };

            save_token(&data)?;
            return Ok(data);
        }

        // Check error type
        if let Ok(err_resp) = serde_json::from_str::<TokenErrorResponse>(&body) {
            match err_resp.error.as_str() {
                "authorization_pending" => continue, // User hasn't authenticated yet
                "slow_down" => {
                    // Back off
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
                "authorization_declined" => {
                    return Err(FabioError::new(
                        ErrorCode::AuthRequired,
                        "Authentication was declined by the user.",
                    )
                    .into());
                }
                "expired_token" => {
                    return Err(FabioError::new(
                        ErrorCode::Timeout,
                        "Device code expired. Please try 'fabio auth login' again.",
                    )
                    .into());
                }
                _ => {
                    return Err(FabioError::with_hint(
                        ErrorCode::AuthRequired,
                        format!("Authentication failed: {}", err_resp.error_description),
                        format!("Error code: {}", err_resp.error),
                    )
                    .into());
                }
            }
        }

        // Unknown error — only include status code, not raw response body
        // (which may contain internal traces, correlation IDs, or injected content)
        let sanitized = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or_else(|| v.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_else(|| format!("HTTP {status} (unrecognized error format)"));
        return Err(FabioError::new(ErrorCode::AuthRequired, sanitized).into());
    }
}

/// Refresh an access token using a refresh token.
async fn refresh_access_token(refresh_token: &str, tenant: &str, scope: &str) -> Result<TokenData> {
    // Disable redirects to prevent credential forwarding via POST body
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());
    let token_url = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token");

    let resp = http
        .post(&token_url)
        .form(&[
            ("client_id", PUBLIC_CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("scope", &format!("{scope} offline_access")),
        ])
        .send()
        .await
        .map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Token refresh failed: {e}"),
            )
        })?;

    if !resp.status().is_success() {
        let status = resp.status().as_u16();
        let text = resp.text().await.unwrap_or_default();
        // Extract only structured error fields — never expose raw response body
        let sanitized = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or_else(|| v.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_else(|| format!("HTTP {status} (token refresh failed)"));
        return Err(FabioError::new(ErrorCode::AuthRequired, sanitized).into());
    }

    let token_resp: TokenResponse = resp.json().await.map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("Invalid refresh response: {e}"),
        )
    })?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(TokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp
            .refresh_token
            .or_else(|| Some(refresh_token.to_string())),
        expires_on: now + token_resp.expires_in,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    })
}

// ── Service Principal Login Methods ─────────────────────────────────────────

/// Authenticate a service principal using a client secret.
pub async fn sp_login_secret(
    tenant: &str,
    client_id: &str,
    client_secret: &str,
    scope: &str,
) -> Result<TokenData> {
    use azure_core::credentials::TokenCredential;

    let credential = azure_identity::ClientSecretCredential::new(
        tenant,
        client_id.to_string(),
        azure_core::credentials::Secret::new(client_secret.to_string()),
        None,
    )
    .map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("Failed to create client secret credential: {e}"),
        )
    })?;

    let token = credential.get_token(&[scope], None).await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Service principal authentication failed: {e}"),
            "Check --tenant, --client-id, and --client-secret values.".to_string(),
        )
    })?;

    let expires_on = std::time::SystemTime::from(token.expires_on)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let data = TokenData {
        access_token: token.token.secret().to_string(),
        refresh_token: None, // SP tokens don't have refresh tokens
        expires_on,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    };

    save_token(&data)?;
    Ok(data)
}

/// Authenticate a service principal using a PEM or PFX certificate.
pub async fn sp_login_certificate(
    tenant: &str,
    client_id: &str,
    certificate_path: &str,
    certificate_password: Option<&str>,
    scope: &str,
) -> Result<TokenData> {
    use azure_core::credentials::TokenCredential;

    let cert_bytes = std::fs::read(certificate_path).map_err(|e| {
        FabioError::new(
            ErrorCode::InvalidInput,
            format!("Failed to read certificate file '{certificate_path}': {e}"),
        )
    })?;

    if cert_bytes.is_empty() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            format!("Certificate file '{certificate_path}' is empty."),
        )
        .into());
    }

    let options =
        certificate_password.map(|pw| azure_identity::ClientCertificateCredentialOptions {
            password: Some(azure_core::credentials::Secret::new(pw.to_string())),
            ..Default::default()
        });

    let credential = azure_identity::ClientCertificateCredential::new(
        tenant.to_string(),
        client_id.to_string(),
        azure_core::credentials::SecretBytes::from(cert_bytes),
        options,
    )
    .map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Failed to create certificate credential: {e}"),
            "Ensure the certificate file is valid PEM or PFX format. For PFX, provide --certificate-password.".to_string(),
        )
    })?;

    let token = credential.get_token(&[scope], None).await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Certificate authentication failed: {e}"),
            "Check the certificate is valid and not expired. Ensure it matches the app registration.".to_string(),
        )
    })?;

    let expires_on = std::time::SystemTime::from(token.expires_on)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let data = TokenData {
        access_token: token.token.secret().to_string(),
        refresh_token: None,
        expires_on,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    };

    save_token(&data)?;
    Ok(data)
}

/// Authenticate a service principal using a federated token (OIDC assertion).
/// Used for workload identity in CI/CD (GitHub Actions, Azure DevOps Pipelines).
pub async fn sp_login_federated(
    tenant: &str,
    client_id: &str,
    federated_token: &str,
    scope: &str,
) -> Result<TokenData> {
    use azure_core::credentials::TokenCredential;

    // Create a static assertion provider that returns the federated token
    let assertion = StaticAssertion(federated_token.to_string());

    let credential = azure_identity::ClientAssertionCredential::new(
        tenant.to_string(),
        client_id.to_string(),
        assertion,
        None,
    )
    .map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("Failed to create federated token credential: {e}"),
        )
    })?;

    let token = credential.get_token(&[scope], None).await.map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("Federated token authentication failed: {e}"),
            "Check the federated token is valid and not expired. Ensure the app registration has the correct federated credential configured.".to_string(),
        )
    })?;

    let expires_on = std::time::SystemTime::from(token.expires_on)
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let data = TokenData {
        access_token: token.token.secret().to_string(),
        refresh_token: None,
        expires_on,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    };

    save_token(&data)?;
    Ok(data)
}

/// A static OIDC assertion that always returns the same token string.
/// Used for federated identity login where the token is provided once.
#[derive(Debug)]
struct StaticAssertion(String);

#[async_trait::async_trait]
impl azure_identity::ClientAssertion for StaticAssertion {
    async fn secret(
        &self,
        _options: Option<azure_core::http::ClientMethodOptions<'_>>,
    ) -> azure_core::Result<String> {
        Ok(self.0.clone())
    }
}

// ── Browser-based PKCE authentication ───────────────────────────────────────

/// Authenticate via browser-based Authorization Code flow with PKCE.
///
/// Opens the system browser to the Azure AD authorize endpoint and listens
/// on localhost for the redirect with the authorization code. On macOS with
/// the Microsoft Enterprise SSO Extension installed, this provides transparent
/// SSO without password entry.
#[allow(clippy::too_many_lines)]
pub async fn browser_login(tenant: Option<&str>, scope: Option<&str>) -> Result<TokenData> {
    use base64::Engine;
    use sha2::Digest;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    let tenant = tenant.unwrap_or(DEFAULT_TENANT);
    let scope = scope.unwrap_or(FABRIC_SCOPE);

    // Step 1: Generate PKCE challenge
    let mut verifier_bytes = [0u8; 32];
    getrandom::fill(&mut verifier_bytes).map_err(|e| {
        FabioError::new(
            ErrorCode::Unknown,
            format!("Failed to generate random bytes for PKCE: {e}"),
        )
    })?;
    let code_verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge_hash = sha2::Sha256::digest(code_verifier.as_bytes());
    let code_challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(challenge_hash);

    // Step 2: Bind a localhost listener on a random port
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|e| {
        FabioError::new(
            ErrorCode::NetworkError,
            format!("Failed to bind localhost listener for OAuth2 redirect: {e}"),
        )
    })?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://localhost:{port}");

    // Step 3: Build the authorize URL
    let authorize_url = format!(
        "https://login.microsoftonline.com/{tenant}/oauth2/v2.0/authorize?\
         client_id={client_id}\
         &response_type=code\
         &redirect_uri={redirect_uri}\
         &scope={scope}+offline_access\
         &code_challenge={code_challenge}\
         &code_challenge_method=S256",
        client_id = urlencoding::encode(PUBLIC_CLIENT_ID),
        redirect_uri = urlencoding::encode(&redirect_uri),
        scope = urlencoding::encode(scope),
        code_challenge = urlencoding::encode(&code_challenge),
    );

    // Step 4: Open the system browser
    eprintln!("Opening browser for authentication...");
    eprintln!("If the browser doesn't open, visit: {authorize_url}");
    open_browser(&authorize_url);

    // Step 5: Wait for the redirect (with timeout)
    let code = tokio::time::timeout(Duration::from_secs(300), async {
        let (mut stream, _addr) = listener.accept().await.map_err(|e| {
            FabioError::new(ErrorCode::NetworkError, format!("Listener accept failed: {e}"))
        })?;

        // Read the HTTP request
        let mut buf = vec![0u8; 4096];
        let n = tokio::io::AsyncReadExt::read(&mut stream, &mut buf).await.map_err(|e| {
            FabioError::new(ErrorCode::NetworkError, format!("Read from redirect failed: {e}"))
        })?;
        let request = String::from_utf8_lossy(&buf[..n]);

        // Extract the authorization code from the query string
        let code = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1)) // GET /path?code=...&... HTTP/1.1
            .and_then(|path| path.split('?').nth(1))
            .and_then(|query| {
                query.split('&').find_map(|param| {
                    let (key, value) = param.split_once('=')?;
                    if key == "code" {
                        Some(urlencoding::decode(value).unwrap_or_default().into_owned())
                    } else {
                        None
                    }
                })
            });

        // Check for error in redirect
        let error = request
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|path| path.split('?').nth(1))
            .and_then(|query| {
                query.split('&').find_map(|param| {
                    let (key, value) = param.split_once('=')?;
                    if key == "error_description" {
                        Some(urlencoding::decode(value).unwrap_or_default().into_owned())
                    } else {
                        None
                    }
                })
            });

        // Send a response to the browser
        let body = if code.is_some() {
            "<html><body><h2>Authentication successful!</h2><p>You can close this window and return to the terminal.</p></body></html>"
        } else {
            "<html><body><h2>Authentication failed.</h2><p>Check the terminal for details.</p></body></html>"
        };
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.ok();
        stream.flush().await.ok();

        code.map_or_else(
            || {
                let msg = error.unwrap_or_else(|| "No authorization code received".to_string());
                Err(FabioError::new(ErrorCode::AuthRequired, msg).into())
            },
            Ok::<String, anyhow::Error>,
        )
    })
    .await
    .map_err(|_| {
        FabioError::new(
            ErrorCode::Timeout,
            "Browser login timed out after 5 minutes. No redirect received.",
        )
    })??;

    // Step 6: Exchange the code for tokens
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| reqwest::Client::new());

    let token_url = format!("https://login.microsoftonline.com/{tenant}/oauth2/v2.0/token");
    let resp = http
        .post(&token_url)
        .form(&[
            ("client_id", PUBLIC_CLIENT_ID),
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect_uri),
            ("code_verifier", &code_verifier),
            ("scope", &format!("{scope} offline_access")),
        ])
        .send()
        .await
        .map_err(|e| {
            FabioError::new(
                ErrorCode::NetworkError,
                format!("Token exchange request failed: {e}"),
            )
        })?;

    if !resp.status().is_success() {
        let text = resp.text().await.unwrap_or_default();
        let sanitized = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| {
                v.get("error_description")
                    .or_else(|| v.get("error"))
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .unwrap_or_else(|| "Token exchange failed".to_string());
        return Err(FabioError::new(ErrorCode::AuthRequired, sanitized).into());
    }

    let token_resp: TokenResponse = resp.json().await.map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("Invalid token response: {e}"),
        )
    })?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let data = TokenData {
        access_token: token_resp.access_token,
        refresh_token: token_resp.refresh_token,
        expires_on: now + token_resp.expires_in,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    };

    save_token(&data)?;
    Ok(data)
}

/// Open the system browser to a URL. Best-effort (no error on failure).
fn open_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn().ok();
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open").arg(url).spawn().ok();
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn()
            .ok();
    }
}

// ── WAM broker authentication for Windows SSO ───────────────────────────────

/// Acquire a token via Windows Web Account Manager (WAM) broker.
///
/// WAM provides SSO with the Windows account — no browser or device code flow needed.
/// Tries silent token acquisition first (cached SSO), falls back to interactive UI.
///
/// The `client_id` should be the Fabio CLI app registration (public client).
/// The `authority` is the AAD tenant (e.g., `"organizations"` for multi-tenant).
#[cfg(windows)]
#[allow(clippy::unused_async, clippy::too_many_lines)]
pub async fn wam_login(
    tenant: Option<&str>,
    scope: Option<&str>,
    client_id: Option<&str>,
) -> Result<TokenData> {
    use windows::Security::Authentication::Web::Core::{
        WebAuthenticationCoreManager, WebTokenRequest, WebTokenRequestStatus,
    };
    let tenant = tenant.unwrap_or("organizations");
    let scope = scope.unwrap_or(FABRIC_SCOPE);
    let client_id = client_id.unwrap_or(PUBLIC_CLIENT_ID);

    // Step 1: Find the AAD WAM provider
    let authority = format!("https://login.microsoftonline.com/{tenant}");
    let provider_op = WebAuthenticationCoreManager::FindAccountProviderWithAuthorityAsync(
        &windows::core::HSTRING::from("https://login.microsoft.com"),
        &windows::core::HSTRING::from(&authority),
    )
    .map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("WAM: Failed to find account provider: {e}"),
            "WAM broker requires Windows 10+ with a signed-in Microsoft account.".to_string(),
        )
    })?;

    let provider = provider_op.GetResults().map_err(|e| {
        FabioError::with_hint(
            ErrorCode::AuthRequired,
            format!("WAM: Account provider not available: {e}"),
            "Ensure you are signed in to Windows with a Microsoft (Entra ID) account.".to_string(),
        )
    })?;

    // Step 2: Build token request
    let request = WebTokenRequest::Create(
        &provider,
        &windows::core::HSTRING::from(scope),
        &windows::core::HSTRING::from(client_id),
    )
    .map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("WAM: Failed to create token request: {e}"),
        )
    })?;

    // Step 3: Try silent token acquisition (SSO, no UI)
    let silent_op = WebAuthenticationCoreManager::GetTokenSilentlyAsync(&request).map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("WAM: Silent token request failed: {e}"),
        )
    })?;

    let silent_result = silent_op.GetResults().map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("WAM: Silent token request error: {e}"),
        )
    })?;

    // Check if silent succeeded
    let result = if silent_result.ResponseStatus()? == WebTokenRequestStatus::Success {
        silent_result
    } else {
        // Step 4: Fall back to interactive UI
        eprintln!("WAM: Silent SSO not available, requesting interactive sign-in...");
        let interactive_op =
            WebAuthenticationCoreManager::RequestTokenAsync(&request).map_err(|e| {
                FabioError::new(
                    ErrorCode::AuthRequired,
                    format!("WAM: Interactive token request failed: {e}"),
                )
            })?;

        let interactive_result = interactive_op.GetResults().map_err(|e| {
            FabioError::new(
                ErrorCode::AuthRequired,
                format!("WAM: Interactive token error: {e}"),
            )
        })?;

        if interactive_result.ResponseStatus()? != WebTokenRequestStatus::Success {
            let error_status = interactive_result.ResponseStatus()?;
            let error_msg = interactive_result
                .ResponseError()
                .ok()
                .and_then(|e| e.ErrorMessage().ok())
                .map_or_else(
                    || format!("WAM authentication failed with status: {error_status:?}"),
                    |msg| msg.to_string_lossy(),
                );
            return Err(FabioError::with_hint(
                ErrorCode::AuthRequired,
                error_msg,
                "Ensure your Windows account is linked to an Entra ID tenant with Fabric access."
                    .to_string(),
            )
            .into());
        }

        interactive_result
    };

    // Step 5: Extract the token from the response
    let responses = result.ResponseData().map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("WAM: Failed to read response data: {e}"),
        )
    })?;

    if responses.Size()? == 0 {
        return Err(FabioError::new(
            ErrorCode::AuthRequired,
            "WAM: No token returned in response.",
        )
        .into());
    }

    let response = responses.GetAt(0).map_err(|e| {
        FabioError::new(
            ErrorCode::AuthRequired,
            format!("WAM: Failed to get token response: {e}"),
        )
    })?;

    let access_token = response
        .Token()
        .map_err(|e| {
            FabioError::new(
                ErrorCode::AuthRequired,
                format!("WAM: Failed to extract token: {e}"),
            )
        })?
        .to_string_lossy();

    if access_token.is_empty() {
        return Err(FabioError::new(ErrorCode::AuthRequired, "WAM: Received empty token.").into());
    }

    // WAM doesn't give us a precise expiry — assume 1 hour (standard AAD default)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let data = TokenData {
        access_token,
        refresh_token: None, // WAM manages its own refresh
        expires_on: now + 3600,
        tenant: tenant.to_string(),
        scope: scope.to_string(),
    };

    save_token(&data)?;
    Ok(data)
}

// ── DPAPI encryption for Windows ────────────────────────────────────────────

/// Encrypt data using Windows DPAPI (user scope).
/// Only the current Windows user can decrypt the result.
#[cfg(windows)]
#[allow(unsafe_code)]
fn dpapi_encrypt(data: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(u32::MAX),
        pbData: data.as_ptr().cast_mut(),
    };

    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // SAFETY: CryptProtectData is a Windows API that encrypts data using the
    // current user's credentials. Input is a valid byte slice, output is zeroed.
    // We free the output buffer with LocalFree after copying.
    let result = unsafe {
        CryptProtectData(
            &raw const input,
            std::ptr::null(), // description (optional)
            std::ptr::null(), // entropy (optional)
            std::ptr::null(), // reserved
            std::ptr::null(), // prompt struct (optional)
            0,                // flags: 0 = user scope
            &raw mut output,
        )
    };

    if result == 0 {
        return Err(FabioError::new(
            ErrorCode::Unknown,
            "DPAPI CryptProtectData failed. Cannot encrypt token cache.",
        )
        .into());
    }

    // SAFETY: output.pbData was allocated by CryptProtectData and is valid for cbData bytes.
    let encrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };

    // SAFETY: LocalFree releases the buffer allocated by CryptProtectData.
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(output.pbData.cast());
    }

    Ok(encrypted)
}

/// Decrypt data using Windows DPAPI (user scope).
#[cfg(windows)]
#[allow(unsafe_code)]
fn dpapi_decrypt(data: &[u8]) -> Result<Vec<u8>> {
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

    let input = CRYPT_INTEGER_BLOB {
        cbData: u32::try_from(data.len()).unwrap_or(u32::MAX),
        pbData: data.as_ptr().cast_mut(),
    };

    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // SAFETY: CryptUnprotectData decrypts data previously encrypted with CryptProtectData.
    // Input is a valid byte slice, output is zeroed. We free the output buffer with LocalFree.
    let result = unsafe {
        CryptUnprotectData(
            &raw const input,
            std::ptr::null_mut(), // description out (PWSTR)
            std::ptr::null(),     // entropy
            std::ptr::null(),     // reserved
            std::ptr::null(),     // prompt struct
            0,                    // flags
            &raw mut output,
        )
    };

    if result == 0 {
        return Err(FabioError::new(
            ErrorCode::AuthRequired,
            "DPAPI CryptUnprotectData failed. Token cache may be corrupted or created by a different user.",
        )
        .into());
    }

    // SAFETY: output.pbData was allocated by CryptUnprotectData and is valid for cbData bytes.
    let decrypted =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };

    // SAFETY: LocalFree releases the buffer allocated by CryptUnprotectData.
    unsafe {
        windows_sys::Win32::Foundation::LocalFree(output.pbData.cast());
    }

    Ok(decrypted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_data_not_expired_when_far_future() {
        let data = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_on: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 3600, // 1 hour from now
            tenant: "test".to_string(),
            scope: "test".to_string(),
        };
        assert!(!data.is_expired());
    }

    #[test]
    fn token_data_expired_when_past() {
        let data = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            expires_on: 0, // epoch = definitely expired
            tenant: "test".to_string(),
            scope: "test".to_string(),
        };
        assert!(data.is_expired());
    }

    #[test]
    fn token_data_expired_within_refresh_margin() {
        let data = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            // Expires in 4 minutes (less than 5-minute REFRESH_MARGIN)
            expires_on: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 240,
            tenant: "test".to_string(),
            scope: "test".to_string(),
        };
        assert!(data.is_expired());
    }

    #[test]
    fn token_data_not_expired_just_outside_margin() {
        let data = TokenData {
            access_token: "test".to_string(),
            refresh_token: None,
            // Expires in 6 minutes (more than 5-minute REFRESH_MARGIN)
            expires_on: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 360,
            tenant: "test".to_string(),
            scope: "test".to_string(),
        };
        assert!(!data.is_expired());
    }

    #[test]
    fn token_data_serializes_to_json() {
        let data = TokenData {
            access_token: "abc123".to_string(),
            refresh_token: Some("refresh456".to_string()),
            expires_on: 1_700_000_000,
            tenant: "my-tenant".to_string(),
            scope: "https://api.fabric.microsoft.com/.default".to_string(),
        };
        let json = serde_json::to_value(&data).unwrap();
        assert_eq!(json["access_token"], "abc123");
        assert_eq!(json["refresh_token"], "refresh456");
        assert_eq!(json["expires_on"], 1_700_000_000);
        assert_eq!(json["tenant"], "my-tenant");
    }

    #[test]
    fn token_data_deserializes_from_json() {
        let json = r#"{
            "access_token": "xyz",
            "refresh_token": null,
            "expires_on": 9999999999,
            "tenant": "t",
            "scope": "s"
        }"#;
        let data: TokenData = serde_json::from_str(json).unwrap();
        assert_eq!(data.access_token, "xyz");
        assert!(data.refresh_token.is_none());
        assert_eq!(data.expires_on, 9_999_999_999);
    }

    #[test]
    fn token_data_sp_has_no_refresh_token() {
        // SP tokens should never have a refresh token
        let data = TokenData {
            access_token: "sp-token".to_string(),
            refresh_token: None,
            expires_on: 1_700_000_000,
            tenant: "sp-tenant".to_string(),
            scope: "https://api.fabric.microsoft.com/.default".to_string(),
        };
        assert!(data.refresh_token.is_none());
    }

    #[test]
    fn cache_path_is_under_home_fabio() {
        let path = cache_path().unwrap();
        assert!(
            path.ends_with(".fabio/token_cache.json") || path.ends_with(".fabio\\token_cache.json")
        );
    }

    #[test]
    fn logout_marker_path_is_under_home_fabio() {
        let path = logout_marker_path().unwrap();
        assert!(path.ends_with(".fabio/.logged_out") || path.ends_with(".fabio\\.logged_out"));
    }

    #[tokio::test]
    async fn static_assertion_returns_token() {
        use azure_identity::ClientAssertion;
        let assertion = StaticAssertion("my-oidc-token".to_string());
        let result = assertion.secret(None).await.unwrap();
        assert_eq!(result, "my-oidc-token");
    }

    #[tokio::test]
    async fn static_assertion_returns_empty_token() {
        use azure_identity::ClientAssertion;
        let assertion = StaticAssertion(String::new());
        let result = assertion.secret(None).await.unwrap();
        assert_eq!(result, "");
    }

    // ── DPAPI unit tests (Windows only) ─────────────────────────────────

    #[test]
    #[cfg(windows)]
    fn dpapi_encrypt_decrypt_roundtrip() {
        let plaintext = b"hello fabio token cache";
        let encrypted = dpapi_encrypt(plaintext).expect("encrypt should succeed");
        assert_ne!(encrypted, plaintext, "encrypted must differ from plaintext");
        let decrypted = dpapi_decrypt(&encrypted).expect("decrypt should succeed");
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_encrypt_decrypt_json_roundtrip() {
        let data = TokenData {
            access_token: "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.test".to_string(),
            refresh_token: Some("0.ARwA6L_K.AgABAAEAAAA".to_string()),
            expires_on: 1_700_000_000,
            tenant: "f32b018c-68ee-40d8-9e1a-d7ab42193a10".to_string(),
            scope: "https://api.fabric.microsoft.com/.default".to_string(),
        };
        let json = serde_json::to_string_pretty(&data).unwrap();
        let encrypted = dpapi_encrypt(json.as_bytes()).expect("encrypt should succeed");
        let decrypted = dpapi_decrypt(&encrypted).expect("decrypt should succeed");
        let restored: TokenData =
            serde_json::from_slice(&decrypted).expect("should parse back to TokenData");
        assert_eq!(restored.access_token, data.access_token);
        assert_eq!(restored.refresh_token, data.refresh_token);
        assert_eq!(restored.expires_on, data.expires_on);
        assert_eq!(restored.tenant, data.tenant);
        assert_eq!(restored.scope, data.scope);
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_encrypt_empty_data() {
        let encrypted = dpapi_encrypt(b"").expect("encrypt empty should succeed");
        let decrypted = dpapi_decrypt(&encrypted).expect("decrypt should succeed");
        assert!(decrypted.is_empty());
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_decrypt_garbage_fails() {
        let result = dpapi_decrypt(b"this is not encrypted data");
        assert!(result.is_err(), "decrypting garbage should fail");
    }

    #[test]
    #[cfg(windows)]
    fn dpapi_encrypt_large_payload() {
        // Tokens can be large (multi-KB JWTs)
        let large = "x".repeat(16_384);
        let encrypted = dpapi_encrypt(large.as_bytes()).expect("encrypt should succeed");
        let decrypted = dpapi_decrypt(&encrypted).expect("decrypt should succeed");
        assert_eq!(decrypted, large.as_bytes());
    }

    #[test]
    #[cfg(windows)]
    fn save_and_load_token_roundtrip_encrypted() {
        // Full integration: save_token encrypts, load_cached_token decrypts
        let data = TokenData {
            access_token: "dpapi-roundtrip-test-token".to_string(),
            refresh_token: None,
            expires_on: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                + 7200,
            tenant: "test-tenant".to_string(),
            scope: "https://api.fabric.microsoft.com/.default".to_string(),
        };
        save_token(&data).expect("save should succeed");
        let loaded = load_cached_token().expect("load should return saved token");
        assert_eq!(loaded.access_token, "dpapi-roundtrip-test-token");
        assert!(loaded.refresh_token.is_none());
        assert_eq!(loaded.tenant, "test-tenant");

        // Verify the file on disk is NOT plaintext
        let path = cache_path().unwrap();
        let raw_bytes = std::fs::read(&path).unwrap();
        let raw_str = String::from_utf8(raw_bytes);
        assert!(
            raw_str.is_err() || !raw_str.unwrap().contains("dpapi-roundtrip-test-token"),
            "token should be encrypted on disk, not plaintext"
        );

        // Clean up
        std::fs::remove_file(&path).ok();
    }
}
