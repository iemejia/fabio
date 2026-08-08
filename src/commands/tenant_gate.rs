//! Generalized **tenant-feature-gate** handling.
//!
//! Many Fabric features are gated by a TENANT SETTING that a Fabric admin can
//! toggle. When a fabio command fails because such a setting is disabled, this
//! module turns the opaque `403 FeatureNotAvailable` into a teaching error that:
//! - names the exact tenant setting (when fabio knows which one gates the command),
//! - tailors the "how to enable it" guidance to whether the caller is a Fabric
//!   admin (probed via the admin-only tenant-settings API), and
//! - carries any feature-specific fallback guidance.
//!
//! It is wired ONCE, generically, into `commands::execute` (which has both the
//! parsed command path and the `FabricClient` in scope), so EVERY command benefits
//! — no per-command wiring. The detection is marker-based (not "any 403"), so a
//! normal RBAC 403 is never mislabeled as a disabled feature.

use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, HintType};

/// A Fabric tenant setting that gates a fabio feature.
pub struct TenantSetting {
    /// The `settingName` used with `fabio admin update-tenant-setting`.
    pub name: &'static str,
    /// Human title (as shown in the Admin portal).
    pub title: &'static str,
    /// Optional feature-specific guidance (what to do meanwhile / not to retry).
    pub fallback: Option<&'static str>,
}

const POWERBI_MCP_FALLBACK: &str = "Do NOT retry `semantic-model generate-dax`/`copilot-schema` or `report copilot-metadata` — \
     they will keep failing. Non-MCP fallbacks that need no Copilot: run DAX with \
     `semantic-model query --dax`; read the model schema with \
     `semantic-model list-tables`/`list-columns`/`list-measures`/`list-relationships`; read a \
     report's definition with `report get-definition`.";

/// Map a command path (`group` or `group.subcommand`) to the tenant setting that
/// gates it. Exact subcommand matches take precedence over group-level ones.
/// Returns `None` when fabio does not know a specific setting for the command
/// (the caller then emits a generic-but-admin-aware hint). Pure/testable.
///
/// To add a NEW gated feature, follow the MANDATORY checklist in AGENTS.md
/// ("Tenant-Feature Gates"): find the exact `settingName` via
/// `fabio admin list-tenant-settings`, VERIFY it actually gates the REST command
/// (many settings gate only the portal UI), add the row below, and extend
/// `registry_maps_known_commands`. NEVER add an unverified mapping — a wrong
/// setting name yields a misleading enable command; the generic hint already
/// covers the un-registered case.
fn setting_for_command(path: &str) -> Option<TenantSetting> {
    let mk = |name, title, fallback| {
        Some(TenantSetting {
            name,
            title,
            fallback,
        })
    };
    // Exact subcommand paths first (a group can mix gated + ungated subcommands).
    match path {
        "semantic-model.generate-dax"
        | "semantic-model.copilot-schema"
        | "report.copilot-metadata" => {
            return mk(
                "PowerBIMCP",
                "Users can use the Power BI Model Context Protocol server endpoint (preview)",
                Some(POWERBI_MCP_FALLBACK),
            );
        }
        "item.create-external-data-share" => {
            return mk(
                "AllowExternalDataSharingSwitch",
                "External data sharing",
                None,
            );
        }
        "item.get-invitation" => {
            return mk(
                "AllowExternalDataSharingReceiverSwitch",
                "Users can accept external data shares",
                None,
            );
        }
        "report.publish-to-web" => {
            return mk("PublishToWeb", "Publish to web", None);
        }
        _ => {}
    }
    // Group-level fallbacks (whole preview item families gated by one setting).
    match path.split('.').next().unwrap_or(path) {
        "azure-databricks-storage" => mk(
            "ArtifactDatabricksStoragePreview",
            "Users can create Azure Databricks Storage items (preview)",
            None,
        ),
        "ontology" => mk(
            "OntologyPreview",
            "Users can create Ontology (preview) items",
            None,
        ),
        "digital-twin-builder" | "digital-twin-builder-flow" => mk(
            "DigitalOperationsPreview",
            "Users can create Digital Twin Builder (preview) items",
            None,
        ),
        "mirrored-catalog" => mk(
            "ArtifactMirroredCatalogPreview",
            "Enable new mirrored catalog items (preview)",
            None,
        ),
        "app-backend" => mk(
            "AppBackendTenant",
            "Enable Fabric App Items (preview)",
            None,
        ),
        _ => None,
    }
}

/// Whether an error indicates a TENANT SETTING is disabled (not a generic RBAC
/// 403). Checks the error's Display AND a `FabioError`'s hint for the
/// tenant-feature markers. Pure/testable.
pub fn is_feature_disabled(err: &anyhow::Error) -> bool {
    let mut hay = err.to_string().to_ascii_lowercase();
    if let Some(fe) = err.downcast_ref::<FabioError>()
        && let Some(h) = &fe.hint
    {
        hay.push(' ');
        hay.push_str(&h.to_ascii_lowercase());
    }
    hay.contains("featurenotavailable")
        || hay.contains("feature is not available")
        || hay.contains("feature is not enabled")
        || hay.contains("not enabled in the tenant")
        || hay.contains("tenantswitchdisabled")
        || (hay.contains("tenant setting") && hay.contains("disabled"))
}

/// Probe whether the authenticated caller is a Fabric administrator by reading
/// the admin tenant-settings API (only admins can access it). `Some(true)` =
/// admin, `Some(false)` = a definitive 401/403 (not admin), `None` = could not
/// determine (network/other error) — so the hint stays non-committal.
pub async fn is_fabric_admin(client: &FabricClient) -> Option<bool> {
    match client.get("/admin/tenantsettings").await {
        Ok(_) => Some(true),
        Err(e) => match e.downcast_ref::<FabioError>() {
            Some(fe) if matches!(fe.code, ErrorCode::Forbidden | ErrorCode::AuthRequired) => {
                Some(false)
            }
            _ => None,
        },
    }
}

/// Build the admin-aware "how to enable it" clause. Pure.
fn enable_clause(is_admin: Option<bool>, setting: Option<&TenantSetting>) -> String {
    match (is_admin, setting) {
        (Some(true), Some(s)) => format!(
            "You HAVE Fabric-admin access — enable it with: `fabio admin update-tenant-setting \
             --setting-name {} --content '{{\"enabled\": true}}'` (or Admin portal → Tenant \
             settings), then retry.",
            s.name
        ),
        (None, Some(s)) => format!(
            "If you have Fabric-admin rights, enable it with: `fabio admin update-tenant-setting \
             --setting-name {} --content '{{\"enabled\": true}}'` (or Admin portal → Tenant \
             settings); otherwise ask your admin.",
            s.name
        ),
        (Some(false), _) => "You are NOT a Fabric administrator, so you cannot enable this \
             yourself — ask your Fabric admin to enable the required tenant setting."
            .to_string(),
        (Some(true), None) => "You HAVE Fabric-admin access — find the setting with `fabio admin \
             list-tenant-settings`, then enable it with `fabio admin update-tenant-setting \
             --setting-name <NAME> --content '{\"enabled\": true}'`."
            .to_string(),
        (None, None) => "If you have Fabric-admin rights, find and enable the setting with `fabio \
             admin list-tenant-settings` + `fabio admin update-tenant-setting`; otherwise ask your \
             admin."
            .to_string(),
    }
}

/// Build the full teaching hint. Pure/testable.
fn build_hint(is_admin: Option<bool>, setting: Option<&TenantSetting>, path: &str) -> String {
    let enable = enable_clause(is_admin, setting);
    setting.map_or_else(
        || {
            format!(
                "`{path}` failed because a required Fabric tenant setting is disabled. {enable}"
            )
        },
        |s| {
            let mut h = format!(
                "The Fabric tenant setting \"{}\" ({}) is disabled — it is required for `{path}`. \
                 {enable}",
                s.title, s.name
            );
            if let Some(fb) = s.fallback {
                h.push(' ');
                h.push_str(fb);
            }
            h
        },
    )
}

/// Build the teaching message. Pure.
fn build_message(setting: Option<&TenantSetting>) -> String {
    setting.map_or_else(
        || "A required Fabric tenant setting is disabled.".to_string(),
        |s| {
            format!(
                "The '{}' feature is not enabled for this tenant (tenant setting {} is disabled).",
                s.title, s.name
            )
        },
    )
}

/// If `err` is a tenant-feature-disabled error, replace it with a teaching error
/// that names the setting (when known), probes the caller's admin status, and
/// gives an admin-aware enable hint. Otherwise `err` is returned unchanged. This
/// is the single generic interception point wired into `commands::execute`.
/// `path` is the `group.subcommand` command path.
pub async fn enrich(client: &FabricClient, path: &str, err: anyhow::Error) -> anyhow::Error {
    if !is_feature_disabled(&err) {
        return err;
    }
    let admin = is_fabric_admin(client).await;
    let setting = setting_for_command(path);
    FabioError::with_typed_hint(
        ErrorCode::Forbidden,
        build_message(setting.as_ref()),
        build_hint(admin, setting.as_ref(), path),
        HintType::SemanticCorrection,
    )
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_feature_not_available_markers() {
        assert!(is_feature_disabled(&anyhow::anyhow!(
            "MCP initialize error -32003: The feature is not available"
        )));
        assert!(is_feature_disabled(&anyhow::anyhow!("FeatureNotAvailable")));
        assert!(is_feature_disabled(&anyhow::anyhow!(
            "TenantSwitchDisabled"
        )));
    }

    #[test]
    fn detects_marker_in_fabio_error_hint() {
        // The forbidden_hint path puts "not enabled in the tenant" in the hint.
        let e: anyhow::Error = FabioError::with_hint(
            ErrorCode::Forbidden,
            "MCP initialize failed: HTTP 403 Forbidden",
            "This feature is not enabled in the tenant. Contact your Fabric administrator.",
        )
        .into();
        assert!(is_feature_disabled(&e));
    }

    #[test]
    fn does_not_flag_generic_rbac_403() {
        // A normal permission 403 must NOT be treated as a disabled feature.
        let e: anyhow::Error = FabioError::with_hint(
            ErrorCode::Forbidden,
            "Forbidden",
            "'lakehouse create' requires at least 'Contributor' role on the workspace.",
        )
        .into();
        assert!(!is_feature_disabled(&e));
        assert!(!is_feature_disabled(&anyhow::anyhow!("network timeout")));
    }

    #[test]
    fn registry_maps_known_commands() {
        assert_eq!(
            setting_for_command("semantic-model.generate-dax")
                .unwrap()
                .name,
            "PowerBIMCP"
        );
        assert_eq!(
            setting_for_command("report.copilot-metadata").unwrap().name,
            "PowerBIMCP"
        );
        // Group-level match.
        assert_eq!(
            setting_for_command("azure-databricks-storage.create")
                .unwrap()
                .name,
            "ArtifactDatabricksStoragePreview"
        );
        assert_eq!(
            setting_for_command("ontology.generate").unwrap().name,
            "OntologyPreview"
        );
        assert_eq!(
            setting_for_command("mirrored-catalog.create").unwrap().name,
            "ArtifactMirroredCatalogPreview"
        );
        // Every currently-registered mapping (coverage guard — keep in sync with
        // the AGENTS.md "Tenant-Feature Gates" list).
        assert_eq!(
            setting_for_command("semantic-model.copilot-schema")
                .unwrap()
                .name,
            "PowerBIMCP"
        );
        assert_eq!(
            setting_for_command("digital-twin-builder.create")
                .unwrap()
                .name,
            "DigitalOperationsPreview"
        );
        assert_eq!(
            setting_for_command("app-backend.create").unwrap().name,
            "AppBackendTenant"
        );
        assert_eq!(
            setting_for_command("item.create-external-data-share")
                .unwrap()
                .name,
            "AllowExternalDataSharingSwitch"
        );
        assert_eq!(
            setting_for_command("item.get-invitation").unwrap().name,
            "AllowExternalDataSharingReceiverSwitch"
        );
        assert_eq!(
            setting_for_command("report.publish-to-web").unwrap().name,
            "PublishToWeb"
        );
        assert!(setting_for_command("workspace.list").is_none());
    }

    #[test]
    fn hint_admin_known_setting_gives_enable_command() {
        let s = setting_for_command("semantic-model.generate-dax");
        let hint = build_hint(Some(true), s.as_ref(), "semantic-model.generate-dax");
        assert!(hint.contains("PowerBIMCP"));
        assert!(hint.contains("HAVE Fabric-admin"));
        assert!(hint.contains("admin update-tenant-setting --setting-name PowerBIMCP"));
        // Feature-specific fallback is included.
        assert!(hint.contains("query --dax"));
    }

    #[test]
    fn hint_non_admin_known_setting_no_command() {
        let s = setting_for_command("azure-databricks-storage.create");
        let hint = build_hint(Some(false), s.as_ref(), "azure-databricks-storage.create");
        assert!(hint.contains("NOT a Fabric administrator"));
        assert!(hint.contains("ArtifactDatabricksStoragePreview"));
        assert!(!hint.contains("update-tenant-setting"));
    }

    #[test]
    fn hint_unknown_setting_admin_lists_settings() {
        let hint = build_hint(Some(true), None, "mirrored-warehouse.create");
        assert!(hint.contains("list-tenant-settings"));
        assert!(hint.contains("HAVE Fabric-admin"));
    }

    #[test]
    fn hint_unknown_setting_uncertain_is_non_committal() {
        let hint = build_hint(None, None, "some.command");
        assert!(hint.contains("If you have Fabric-admin rights"));
        assert!(hint.contains("otherwise ask your admin"));
    }
}
