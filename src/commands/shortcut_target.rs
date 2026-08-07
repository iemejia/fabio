//! Shared shortcut target-configuration builder.
//!
//! Both Lakehouse `OneLake` shortcuts (`/items/{id}/shortcuts`) and KQL-database
//! table shortcuts (`/kqlDatabases/{id}/shortcuts`) accept the same
//! `{<discriminator>: {...}}` target object (Fabric's `CreatableShortcutTarget`
//! / `Target`), where the discriminator is one of the supported storage targets.
//! This module resolves the `(discriminator, targetBody)` pair from either a raw
//! `--target` JSON escape hatch or the typed per-target flags. Pure and unit-tested.

use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

/// Typed target-configuration flags for a shortcut `create` command.
///
/// Any combination is accepted; only the fields relevant to the resolved target
/// type are used, and required fields per type are validated in
/// [`build_shortcut_target`].
#[derive(Default)]
pub struct ShortcutTargetFlags<'a> {
    pub connection_id: Option<&'a str>,
    pub location: Option<&'a str>,
    pub subpath: Option<&'a str>,
    pub bucket: Option<&'a str>,
    pub target_workspace: Option<&'a str>,
    pub target_item: Option<&'a str>,
    pub target_path: Option<&'a str>,
    pub environment_domain: Option<&'a str>,
    pub delta_lake_folder: Option<&'a str>,
    pub update_sensitivity: bool,
}

/// The nine Fabric shortcut target discriminators, in canonical camelCase.
pub const VALID_TARGET_TYPES: &str = "OneLake, AdlsGen2, AmazonS3, AzureBlobStorage, \
GoogleCloudStorage, S3Compatible, Dataverse, ExternalDataShare, OneDriveSharePoint";

/// Normalize a user-supplied target type to the exact Fabric discriminator key.
///
/// Accepts common aliases and any case/separator style (e.g. `adls-gen2`,
/// `amazon_s3`, `gcs`, `sharepoint`). Pure for unit testing.
pub fn normalize_target_type(input: &str) -> Option<&'static str> {
    let key: String = input
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|c| *c != '-' && *c != '_' && *c != ' ')
        .collect();
    match key.as_str() {
        "onelake" => Some("oneLake"),
        "adlsgen2" | "adls" => Some("adlsGen2"),
        "amazons3" | "s3" => Some("amazonS3"),
        "azureblob" | "azureblobstorage" | "blob" => Some("azureBlobStorage"),
        "googlecloudstorage" | "gcs" => Some("googleCloudStorage"),
        "s3compatible" => Some("s3Compatible"),
        "dataverse" => Some("dataverse"),
        "externaldatashare" | "eds" => Some("externalDataShare"),
        "onedrivesharepoint" | "sharepoint" | "onedrive" => Some("oneDriveSharePoint"),
        _ => None,
    }
}

/// Require a typed flag to be present and non-empty for a given target type.
fn require<'a>(v: Option<&'a str>, flag: &str, disc: &str) -> Result<&'a str> {
    v.map(str::trim).filter(|s| !s.is_empty()).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("{flag} is required for shortcut target type '{disc}'"),
            format!("Provide {flag}, or pass the full target object as JSON with --target."),
        )
        .into()
    })
}

/// Resolve the `(discriminator, targetBody)` pair for a shortcut from either a
/// raw `--target` JSON escape hatch or the typed per-target flags. Pure.
pub fn build_shortcut_target(
    target_type: &str,
    target_json: Option<&str>,
    f: &ShortcutTargetFlags,
) -> Result<(String, Value)> {
    let disc = normalize_target_type(target_type).ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Unknown shortcut target type '{target_type}'"),
            format!("Valid target types: {VALID_TARGET_TYPES}."),
        )
    })?;

    // Escape hatch: a full target object as JSON overrides the typed flags.
    if let Some(json) = target_json {
        let body: Value = serde_json::from_str(json)
            .map_err(|e| FabioError::invalid_input(format!("Invalid --target JSON: {e}")))?;
        return Ok((disc.to_string(), body));
    }

    let body = match disc {
        "oneLake" => {
            let ws = require(f.target_workspace, "--target-workspace", disc)?;
            let item = require(f.target_item, "--target-item", disc)?;
            let path = require(f.target_path, "--target-path", disc)?;
            let mut o = serde_json::json!({"workspaceId": ws, "itemId": item, "path": path});
            if let Some(c) = f.connection_id.filter(|s| !s.is_empty()) {
                o["connectionId"] = Value::from(c);
            }
            o
        }
        "adlsGen2" | "amazonS3" | "azureBlobStorage" | "googleCloudStorage" => {
            let loc = require(f.location, "--location", disc)?;
            let conn = require(f.connection_id, "--connection-id", disc)?;
            let mut o = serde_json::json!({"location": loc, "connectionId": conn});
            if let Some(s) = f.subpath.filter(|s| !s.is_empty()) {
                o["subpath"] = Value::from(s);
            }
            o
        }
        "s3Compatible" => {
            let loc = require(f.location, "--location", disc)?;
            let conn = require(f.connection_id, "--connection-id", disc)?;
            let bucket = require(f.bucket, "--bucket", disc)?;
            let mut o =
                serde_json::json!({"location": loc, "connectionId": conn, "bucket": bucket});
            if let Some(s) = f.subpath.filter(|s| !s.is_empty()) {
                o["subpath"] = Value::from(s);
            }
            o
        }
        "dataverse" => {
            let conn = require(f.connection_id, "--connection-id", disc)?;
            let env = require(f.environment_domain, "--environment-domain", disc)?;
            let mut o = serde_json::json!({"connectionId": conn, "environmentDomain": env});
            if let Some(d) = f.delta_lake_folder.filter(|s| !s.is_empty()) {
                o["deltaLakeFolder"] = Value::from(d);
            }
            o
        }
        "externalDataShare" => {
            let conn = require(f.connection_id, "--connection-id", disc)?;
            serde_json::json!({ "connectionId": conn })
        }
        "oneDriveSharePoint" => {
            let loc = require(f.location, "--location", disc)?;
            let conn = require(f.connection_id, "--connection-id", disc)?;
            let mut o = serde_json::json!({"location": loc, "connectionId": conn});
            if let Some(s) = f.subpath.filter(|s| !s.is_empty()) {
                o["subpath"] = Value::from(s);
            }
            if f.update_sensitivity {
                o["updateFabricItemSensitivity"] = Value::Bool(true);
            }
            o
        }
        _ => unreachable!("normalize_target_type only yields known discriminators"),
    };
    Ok((disc.to_string(), body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_target_type_accepts_aliases_and_casing() {
        assert_eq!(normalize_target_type("OneLake"), Some("oneLake"));
        assert_eq!(normalize_target_type("adls-gen2"), Some("adlsGen2"));
        assert_eq!(normalize_target_type("ADLS"), Some("adlsGen2"));
        assert_eq!(normalize_target_type("amazon_s3"), Some("amazonS3"));
        assert_eq!(normalize_target_type("s3"), Some("amazonS3"));
        assert_eq!(
            normalize_target_type("azureBlobStorage"),
            Some("azureBlobStorage")
        );
        assert_eq!(normalize_target_type("blob"), Some("azureBlobStorage"));
        assert_eq!(normalize_target_type("gcs"), Some("googleCloudStorage"));
        assert_eq!(normalize_target_type("S3Compatible"), Some("s3Compatible"));
        assert_eq!(normalize_target_type("dataverse"), Some("dataverse"));
        assert_eq!(
            normalize_target_type("external-data-share"),
            Some("externalDataShare")
        );
        assert_eq!(
            normalize_target_type("sharepoint"),
            Some("oneDriveSharePoint")
        );
        assert_eq!(normalize_target_type("nope"), None);
    }

    #[test]
    fn build_target_raw_json_escape_hatch() {
        let f = ShortcutTargetFlags::default();
        let (disc, body) = build_shortcut_target(
            "adlsGen2",
            Some(r#"{"location":"x","connectionId":"c"}"#),
            &f,
        )
        .unwrap();
        assert_eq!(disc, "adlsGen2");
        assert_eq!(body["location"], "x");
    }

    #[test]
    fn build_target_onelake_from_flags() {
        let f = ShortcutTargetFlags {
            target_workspace: Some("ws"),
            target_item: Some("it"),
            target_path: Some("Tables/orders"),
            ..Default::default()
        };
        let (disc, body) = build_shortcut_target("onelake", None, &f).unwrap();
        assert_eq!(disc, "oneLake");
        assert_eq!(body["workspaceId"], "ws");
        assert_eq!(body["itemId"], "it");
        assert_eq!(body["path"], "Tables/orders");
        assert!(body.get("connectionId").is_none());
    }

    #[test]
    fn build_target_adls_requires_location_and_connection() {
        let f = ShortcutTargetFlags {
            location: Some("https://a.dfs.core.windows.net/c"),
            connection_id: Some("conn-1"),
            subpath: Some("/data"),
            ..Default::default()
        };
        let (disc, body) = build_shortcut_target("adls-gen2", None, &f).unwrap();
        assert_eq!(disc, "adlsGen2");
        assert_eq!(body["location"], "https://a.dfs.core.windows.net/c");
        assert_eq!(body["connectionId"], "conn-1");
        assert_eq!(body["subpath"], "/data");
    }

    #[test]
    fn build_target_adls_missing_connection_errors() {
        let f = ShortcutTargetFlags {
            location: Some("https://a"),
            ..Default::default()
        };
        let err = build_shortcut_target("adlsGen2", None, &f)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--connection-id"), "got: {err}");
    }

    #[test]
    fn build_target_s3compatible_needs_bucket() {
        let f = ShortcutTargetFlags {
            location: Some("https://s3"),
            connection_id: Some("c"),
            ..Default::default()
        };
        let err = build_shortcut_target("s3Compatible", None, &f)
            .unwrap_err()
            .to_string();
        assert!(err.contains("--bucket"), "got: {err}");
        let f2 = ShortcutTargetFlags {
            bucket: Some("my-bucket"),
            ..f
        };
        let (_, body) = build_shortcut_target("s3Compatible", None, &f2).unwrap();
        assert_eq!(body["bucket"], "my-bucket");
    }

    #[test]
    fn build_target_dataverse_and_eds_and_sharepoint() {
        let dv = ShortcutTargetFlags {
            connection_id: Some("c"),
            environment_domain: Some("https://org.crm.dynamics.com"),
            delta_lake_folder: Some("deltalake"),
            ..Default::default()
        };
        let (disc, body) = build_shortcut_target("dataverse", None, &dv).unwrap();
        assert_eq!(disc, "dataverse");
        assert_eq!(body["environmentDomain"], "https://org.crm.dynamics.com");
        assert_eq!(body["deltaLakeFolder"], "deltalake");

        let eds = ShortcutTargetFlags {
            connection_id: Some("c"),
            ..Default::default()
        };
        let (disc, body) = build_shortcut_target("externalDataShare", None, &eds).unwrap();
        assert_eq!(disc, "externalDataShare");
        assert_eq!(body["connectionId"], "c");

        let sp = ShortcutTargetFlags {
            connection_id: Some("c"),
            location: Some("https://contoso.sharepoint.com"),
            update_sensitivity: true,
            ..Default::default()
        };
        let (disc, body) = build_shortcut_target("sharepoint", None, &sp).unwrap();
        assert_eq!(disc, "oneDriveSharePoint");
        assert_eq!(body["updateFabricItemSensitivity"], true);
    }

    #[test]
    fn build_target_unknown_type_errors_with_enum() {
        let f = ShortcutTargetFlags::default();
        let err = build_shortcut_target("dropbox", None, &f)
            .unwrap_err()
            .to_string();
        assert!(err.contains("Unknown shortcut target type"), "got: {err}");
    }
}
