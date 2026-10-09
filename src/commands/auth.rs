use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::output;
use crate::token_cache;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum AuthCommand {
    /// Validate Azure CLI authentication, or use an explicit interactive/workload flow
    Login {
        /// Microsoft Entra tenant ID (uses Azure CLI's current tenant by default)
        #[arg(long)]
        tenant: Option<String>,

        /// `OAuth2` scope (defaults to Fabric API scope)
        #[arg(long)]
        scope: Option<String>,

        /// Authenticate as a service principal (requires --tenant and --client-id)
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        service_principal: bool,

        /// Use device-code login with a customer-managed public client
        #[arg(long, conflicts_with_all = ["browser", "wam"])]
        device_code: bool,

        /// Use Windows WAM with a customer-managed public client (Windows only)
        #[arg(long, conflicts_with_all = ["device_code", "browser"])]
        wam: bool,

        /// Use browser PKCE with a customer-managed public client
        #[arg(long, conflicts_with_all = ["device_code", "wam"])]
        browser: bool,

        /// Public-client ID for interactive login, or application ID for a service principal
        #[arg(long)]
        client_id: Option<String>,

        /// Client secret for service principal authentication
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        client_secret: Option<String>,

        /// Path to a PEM or PFX certificate file for service principal authentication
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        certificate: Option<String>,

        /// Password for the certificate file (PFX/PKCS12)
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        certificate_password: Option<String>,

        /// Federated token (OIDC assertion) for workload identity authentication
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        federated_token: Option<String>,

        /// Path to a file containing the federated token (OIDC assertion)
        #[arg(long, conflicts_with_all = ["device_code", "wam", "browser"])]
        federated_token_file: Option<String>,
    },
    /// Clear Fabio-managed cached credentials (ambient credentials remain available)
    Logout,
    /// Show current authentication status and credential source
    Status,
}

/// Whether an `auth login` invocation should use the service-principal flow.
///
/// True when `--service-principal` is set OR any actual service-principal credential
/// (client secret, certificate, or federated/OIDC token) is provided. `--client-id`
/// alone does NOT imply service-principal — it may be a public-client id for an
/// interactive (device-code/WAM) login.
const fn implies_service_principal(
    service_principal: bool,
    client_secret: Option<&str>,
    certificate: Option<&str>,
    certificate_password: Option<&str>,
    federated_token: Option<&str>,
    federated_token_file: Option<&str>,
) -> bool {
    service_principal
        || client_secret.is_some()
        || certificate.is_some()
        || certificate_password.is_some()
        || federated_token.is_some()
        || federated_token_file.is_some()
}

pub async fn execute(cli: &Cli, command: &AuthCommand) -> Result<()> {
    match command {
        AuthCommand::Login {
            tenant,
            scope,
            service_principal,
            device_code,
            wam,
            browser,
            client_id,
            client_secret,
            certificate,
            certificate_password,
            federated_token,
            federated_token_file,
        } => {
            // A service-principal credential flag implies service-principal mode even
            // without the explicit --service-principal flag, so a natural CI invocation
            // like `auth login --tenant .. --client-id .. --federated-token ..` works
            // instead of silently falling back to interactive device-code login.
            let use_service_principal = implies_service_principal(
                *service_principal,
                client_secret.as_deref(),
                certificate.as_deref(),
                certificate_password.as_deref(),
                federated_token.as_deref(),
                federated_token_file.as_deref(),
            );
            if use_service_principal {
                login_service_principal(
                    cli,
                    tenant.as_deref(),
                    scope.as_deref(),
                    client_id.as_deref(),
                    client_secret.as_deref(),
                    certificate.as_deref(),
                    certificate_password.as_deref(),
                    federated_token.as_deref(),
                    federated_token_file.as_deref(),
                )
                .await
            } else if *device_code || *wam || *browser {
                let (public_client_id, client_id_source) = resolve_public_client_id(
                    client_id.as_deref(),
                    std::env::var("FABIO_CLIENT_ID").ok().as_deref(),
                )?;
                if *wam {
                    login_wam(
                        cli,
                        tenant.as_deref(),
                        scope.as_deref(),
                        &public_client_id,
                        client_id_source,
                    )
                    .await
                } else if *browser {
                    login_browser(
                        cli,
                        tenant.as_deref(),
                        scope.as_deref(),
                        &public_client_id,
                        client_id_source,
                    )
                    .await
                } else {
                    login_device_code(
                        cli,
                        tenant.as_deref(),
                        scope.as_deref(),
                        &public_client_id,
                        client_id_source,
                    )
                    .await
                }
            } else if client_id.is_some() {
                Err(crate::errors::FabioError::with_hint(
                    crate::errors::ErrorCode::InvalidInput,
                    "--client-id requires an explicit authentication mode.",
                    "Use --device-code, --browser, --wam, or --service-principal. Plain 'fabio auth login' validates Azure CLI authentication.".to_string(),
                )
                .into())
            } else {
                login_azure_cli(cli, tenant.as_deref(), scope.as_deref()).await
            }
        }
        AuthCommand::Logout => logout(cli),
        AuthCommand::Status => status(cli).await,
    }
}

fn resolve_public_client_id(
    cli_value: Option<&str>,
    env_value: Option<&str>,
) -> Result<(String, token_cache::ClientIdSource)> {
    let (value, source) = if let Some(value) = cli_value.filter(|value| !value.trim().is_empty()) {
        (value.trim(), token_cache::ClientIdSource::CliFlag)
    } else if let Some(value) = env_value.filter(|value| !value.trim().is_empty()) {
        (value.trim(), token_cache::ClientIdSource::FabioClientIdEnv)
    } else {
        return Err(crate::errors::FabioError::with_hint(
            crate::errors::ErrorCode::InvalidInput,
            "Fabio-managed interactive login requires a customer-owned public-client ID.",
            "Pass --client-id <PUBLIC_CLIENT_ID> or set FABIO_CLIENT_ID. To use the default Azure CLI path, run 'az login' followed by 'fabio auth login'.".to_string(),
        )
        .into());
    };
    crate::client::validate_uuid(value, "--client-id")?;
    Ok((value.to_string(), source))
}

async fn login_azure_cli(cli: &Cli, tenant: Option<&str>, scope: Option<&str>) -> Result<()> {
    let scope = scope.unwrap_or("https://api.fabric.microsoft.com/.default");
    let token_expiry = crate::client::validate_azure_cli(tenant, scope).await?;
    token_cache::clear_cache()?;
    let expires_in = token_expiry
        .duration_since(std::time::SystemTime::now())
        .unwrap_or_default()
        .as_secs();
    let obj = serde_json::json!({
        "status": "logged_in",
        "credential_source": "azure_cli",
        "method": "azure_cli",
        "tenant": tenant,
        "scope": scope,
        "expires_in_seconds": expires_in,
        "message": "Azure CLI authentication validated. Fabio-managed cached credentials were cleared; subsequent commands will use Azure CLI unless a higher-priority credential is configured."
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn login_device_code(
    cli: &Cli,
    tenant: Option<&str>,
    scope: Option<&str>,
    client_id: &str,
    client_id_source: token_cache::ClientIdSource,
) -> Result<()> {
    let data = token_cache::device_code_login(tenant, scope, client_id, client_id_source).await?;

    let expires_in = data.expires_on.saturating_sub(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );

    let obj = serde_json::json!({
        "status": "logged_in",
        "credential_source": "fabio_device_code",
        "method": "device_code",
        "client_id": client_id,
        "client_id_source": client_id_source,
        "tenant": data.tenant,
        "expires_in_seconds": expires_in,
        "message": "Successfully authenticated via device code flow. Token cached at ~/.fabio/token_cache.json"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[allow(unused_variables, clippy::unused_async)]
async fn login_wam(
    cli: &Cli,
    tenant: Option<&str>,
    scope: Option<&str>,
    client_id: &str,
    client_id_source: token_cache::ClientIdSource,
) -> Result<()> {
    #[cfg(windows)]
    {
        let data = token_cache::wam_login(tenant, scope, client_id, client_id_source).await?;

        let expires_in = data.expires_on.saturating_sub(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        let obj = serde_json::json!({
            "status": "logged_in",
            "credential_source": "wam_broker",
            "method": "wam",
            "client_id": client_id,
            "client_id_source": client_id_source,
            "tenant": data.tenant,
            "expires_in_seconds": expires_in,
            "message": "Successfully authenticated via Windows WAM broker (SSO). Token cached at ~/.fabio/token_cache.json"
        });
        output::render_object(cli, &obj, "status");
        Ok(())
    }

    #[cfg(not(windows))]
    {
        use crate::errors::{ErrorCode, FabioError};
        // Suppress unused variable warnings
        let _ = (cli, tenant, scope, client_id, client_id_source);
        Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--wam is only supported on Windows.",
            "Use plain 'fabio auth login' with Azure CLI, or --device-code/--browser with a customer public-client ID.".to_string(),
        )
        .into())
    }
}

async fn login_browser(
    cli: &Cli,
    tenant: Option<&str>,
    scope: Option<&str>,
    client_id: &str,
    client_id_source: token_cache::ClientIdSource,
) -> Result<()> {
    let data = token_cache::browser_login(tenant, scope, client_id, client_id_source).await?;

    let expires_in = data.expires_on.saturating_sub(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );

    let obj = serde_json::json!({
        "status": "logged_in",
        "credential_source": "browser_pkce",
        "method": "browser_pkce",
        "client_id": client_id,
        "client_id_source": client_id_source,
        "tenant": data.tenant,
        "expires_in_seconds": expires_in,
        "message": "Successfully authenticated via browser (PKCE). Token cached at ~/.fabio/token_cache.json"
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
async fn login_service_principal(
    cli: &Cli,
    tenant: Option<&str>,
    scope: Option<&str>,
    client_id: Option<&str>,
    client_secret: Option<&str>,
    certificate: Option<&str>,
    certificate_password: Option<&str>,
    federated_token: Option<&str>,
    federated_token_file: Option<&str>,
) -> Result<()> {
    use crate::errors::{ErrorCode, FabioError};

    let tenant = tenant
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--tenant is required for service principal authentication.",
                "Example: fabio auth login --service-principal --tenant <TENANT_ID> --client-id <CLIENT_ID> --client-secret <SECRET>".to_string(),
            )
        })?;

    let client_id = client_id
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--client-id is required for service principal authentication.",
                "Example: fabio auth login --service-principal --tenant <TENANT_ID> --client-id <CLIENT_ID> --client-secret <SECRET>".to_string(),
            )
        })?;

    let scope = scope.unwrap_or("https://api.fabric.microsoft.com/.default");

    // Filter empty strings to treat them as "not provided"
    let client_secret = client_secret.filter(|s| !s.is_empty());
    let certificate = certificate.filter(|s| !s.is_empty());
    let federated_token = federated_token.filter(|s| !s.is_empty());
    let federated_token_file = federated_token_file.filter(|s| !s.is_empty());

    // Determine credential type: secret vs certificate vs federated token
    let has_secret = client_secret.is_some();
    let has_cert = certificate.is_some();
    let has_federated = federated_token.is_some() || federated_token_file.is_some();

    let credential_count = u8::from(has_secret) + u8::from(has_cert) + u8::from(has_federated);
    if credential_count == 0 {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Service principal login requires one of: --client-secret, --certificate, or --federated-token/--federated-token-file.",
            "Example: fabio auth login --service-principal --tenant <T> --client-id <C> --client-secret <S>".to_string(),
        ).into());
    }
    if credential_count > 1 {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Only one credential type allowed: --client-secret, --certificate, or --federated-token/--federated-token-file.",
        ).into());
    }

    let data = if has_secret {
        token_cache::sp_login_secret(tenant, client_id, client_secret.unwrap(), scope).await?
    } else if has_cert {
        token_cache::sp_login_certificate(
            tenant,
            client_id,
            certificate.unwrap(),
            certificate_password,
            scope,
        )
        .await?
    } else {
        // Federated token: prefer inline token over file
        let token_value = if let Some(token) = federated_token {
            token.to_string()
        } else {
            let path = federated_token_file.unwrap();
            let content = std::fs::read_to_string(path).map_err(|e| {
                FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Failed to read federated token file '{path}': {e}"),
                )
            })?;
            let trimmed = content.trim().to_string();
            if trimmed.is_empty() {
                return Err(FabioError::new(
                    ErrorCode::InvalidInput,
                    format!("Federated token file '{path}' is empty."),
                )
                .into());
            }
            trimmed
        };
        token_cache::sp_login_federated(tenant, client_id, &token_value, scope).await?
    };

    let expires_in = data.expires_on.saturating_sub(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    );

    let method = if has_secret {
        "client_secret"
    } else if has_cert {
        "certificate"
    } else {
        "federated_token"
    };

    let obj = serde_json::json!({
        "status": "logged_in",
        "credential_source": "service_principal",
        "method": method,
        "tenant": data.tenant,
        "client_id": client_id,
        "expires_in_seconds": expires_in,
        "message": format!("Successfully authenticated service principal via {method}. Token cached at ~/.fabio/token_cache.json")
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

fn logout(cli: &Cli) -> Result<()> {
    token_cache::clear_cache()?;

    let obj = serde_json::json!({
        "status": "logged_out",
        "message": "Fabio-managed token cache cleared. Azure CLI, environment, managed identity, and Azure Developer CLI credentials were not signed out and may still authenticate commands."
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

async fn status(cli: &Cli) -> Result<()> {
    use crate::client::{CredentialSource, FabricClient};

    let client = FabricClient::new();
    match client.require_auth().await {
        Ok(_) => {
            let source = client.credential_source().await;
            let source_type = source.map_or("unknown", |s| match s {
                CredentialSource::AccessToken => "access_token",
                CredentialSource::FabioCache => "fabio_cache",
                CredentialSource::Environment => "environment",
                CredentialSource::ManagedIdentity => "managed_identity",
                CredentialSource::AzureCli => "azure_cli",
                CredentialSource::AzureDeveloperCli => "azure_developer_cli",
            });
            let source_display = source.map_or_else(|| "unknown".to_string(), |s| s.to_string());
            let mut obj = serde_json::json!({
                "status": "authenticated",
                "credential_source": source_type,
                "message": format!("Token acquired successfully via {source_display}")
            });
            if source == Some(CredentialSource::FabioCache)
                && let Some(cached) = token_cache::load_cached_token()
            {
                obj["method"] = serde_json::json!(cached.auth_method);
                obj["client_id"] = serde_json::json!(cached.client_id);
                obj["client_id_source"] = serde_json::json!(cached.client_id_source);
            }
            output::render_object(cli, &obj, "status");
        }
        Err(e) => {
            let obj = serde_json::json!({
                "status": "not_authenticated",
                "message": e.to_string(),
                "hint": "Run 'az login' then 'fabio auth login'. Alternatively, use --device-code/--browser/--wam with a customer public-client ID, or --service-principal for automation."
            });
            output::render_object(cli, &obj, "status");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{implies_service_principal, resolve_public_client_id};
    use crate::token_cache::ClientIdSource;

    #[test]
    fn explicit_flag_implies_sp() {
        assert!(implies_service_principal(
            true, None, None, None, None, None
        ));
    }

    #[test]
    fn credential_flags_imply_sp_without_explicit_flag() {
        // A natural CI invocation (federated/OIDC token, secret, or cert) routes to
        // the service-principal flow even without --service-principal.
        assert!(implies_service_principal(
            false,
            Some("secret"),
            None,
            None,
            None,
            None
        ));
        assert!(implies_service_principal(
            false,
            None,
            Some("cert.pem"),
            None,
            None,
            None
        ));
        assert!(implies_service_principal(
            false,
            None,
            None,
            Some("password"),
            None,
            None
        ));
        assert!(implies_service_principal(
            false,
            None,
            None,
            None,
            Some("oidc.jwt"),
            None
        ));
        assert!(implies_service_principal(
            false,
            None,
            None,
            None,
            None,
            Some("/tok")
        ));
    }

    #[test]
    fn no_credential_does_not_imply_service_principal() {
        assert!(!implies_service_principal(
            false, None, None, None, None, None
        ));
    }

    #[test]
    fn public_client_cli_flag_takes_precedence() {
        let (id, source) = resolve_public_client_id(
            Some(" 11111111-2222-3333-4444-555555555555 "),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee"),
        )
        .unwrap();
        assert_eq!(id, "11111111-2222-3333-4444-555555555555");
        assert_eq!(source, ClientIdSource::CliFlag);
    }

    #[test]
    fn public_client_uses_environment_fallback() {
        let (id, source) =
            resolve_public_client_id(None, Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")).unwrap();
        assert_eq!(id, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
        assert_eq!(source, ClientIdSource::FabioClientIdEnv);
    }

    #[test]
    fn public_client_requires_explicit_configuration() {
        assert!(resolve_public_client_id(None, None).is_err());
        assert!(resolve_public_client_id(Some("  "), Some(" ")).is_err());
    }
}
