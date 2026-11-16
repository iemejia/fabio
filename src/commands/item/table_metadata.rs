//! Common `OneLake` table metadata, not item tags or semantic-model descriptions.
use std::collections::HashSet;

use anyhow::Result;
use clap::Args;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError};
use crate::output;

#[derive(Debug, Args)]
pub struct Target {
    /// Workspace UUID
    #[arg(short, long, env = "FABIO_WORKSPACE")]
    workspace: String,
    /// Data item UUID (including a shortcut's containing item)
    #[arg(long)]
    id: String,
    /// Table schema
    #[arg(long, default_value = "dbo")]
    schema: String,
    /// Exact table name, not a path or wildcard
    #[arg(long)]
    table: String,
}

impl Target {
    fn path(&self) -> Result<String> {
        validate_uuid(&self.workspace, "--workspace")?;
        validate_uuid(&self.id, "--id")?;
        for (name, value) in [("schema", &self.schema), ("table", &self.table)] {
            if value.trim().is_empty()
                || matches!(value.as_str(), "." | ".." | "*")
                || value.contains(['/', '\\'])
                || value.chars().any(char::is_control)
            {
                return Err(FabioError::with_hint(ErrorCode::InvalidInput,
                    format!("Invalid {name}: expected an exact name, not a path or wildcard"),
                    "Select one schema and table explicitly. Metadata writes through a shortcut replace its target table's metadata.").into());
            }
        }
        Ok(format!(
            "common/v1/{}/{}/schemas/{}/tables/{}/additionalMetadata",
            self.workspace,
            self.id,
            urlencoding::encode(&self.schema),
            urlencoding::encode(&self.table)
        ))
    }
}

fn validate_fields(value: &Value) -> Result<()> {
    if !value.is_object() {
        return Err(FabioError::invalid_input("Metadata must be a JSON object").into());
    }
    if let Some(description) = value.get("description")
        && description
            .as_str()
            .is_none_or(|s| s.chars().count() > 1024)
    {
        return Err(FabioError::invalid_input(
            "Metadata description must be a string of at most 1024 characters",
        )
        .into());
    }
    if let Some(tags) = value.get("tags") {
        let tags = tags.as_object().ok_or_else(|| {
            FabioError::invalid_input("Metadata tags must be a string-to-string object")
        })?;
        if tags.iter().any(|(key, value)| {
            key.chars().count() > 128 || value.as_str().is_none_or(|s| s.chars().count() > 256)
        }) {
            return Err(FabioError::invalid_input(
                "Metadata tag keys allow 128 characters and string values allow 256 characters",
            )
            .into());
        }
    }
    Ok(())
}

fn validate_metadata(value: &Value, input: bool) -> Result<()> {
    validate_fields(value)?;
    if input
        && value.as_object().is_some_and(|m| {
            m.keys()
                .any(|k| !matches!(k.as_str(), "description" | "tags" | "columns"))
        })
    {
        return Err(FabioError::invalid_input(
            "Metadata input supports description, tags and columns only",
        )
        .into());
    }
    if let Some(columns) = value.get("columns") {
        let columns = columns
            .as_array()
            .ok_or_else(|| FabioError::invalid_input("Metadata columns must be an array"))?;
        let mut names = HashSet::new();
        for column in columns {
            validate_fields(column)?;
            if input && column.get("columnId").is_some() {
                return Err(FabioError::with_hint(ErrorCode::InvalidInput,
                    "Column metadata input must use columnName, not columnId or both identifiers",
                    "GET can return both resolved identifiers, but POST rejects both together. Supply only the intended columnName in your input. Fabio omits resolved columnId fields when preserving metadata from its own GET.").into());
            }
            let name = column
                .get("columnName")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| {
                    FabioError::invalid_input("Each metadata column needs an existing columnName")
                })?;
            if !names.insert(name) {
                return Err(FabioError::invalid_input("Duplicate metadata columnName").into());
            }
            if input
                && column.as_object().is_some_and(|m| {
                    m.keys()
                        .any(|k| !matches!(k.as_str(), "columnName" | "description" | "tags"))
                })
            {
                return Err(FabioError::invalid_input(
                    "Column metadata supports columnName, description and tags only",
                )
                .into());
            }
        }
    }
    Ok(())
}

fn merge_fields(target: &mut Value, patch: &Value) {
    if let Some(description) = patch.get("description") {
        target["description"] = description.clone();
    }
    if let Some(tags) = patch.get("tags").and_then(Value::as_object) {
        if target.get("tags").is_none() {
            target["tags"] = json!({});
        }
        target["tags"]
            .as_object_mut()
            .expect("validated tags")
            .extend(tags.clone());
    }
}

/// Reads resolve column IDs as well as names; writes must choose one identifier.
fn writable_metadata(value: &Value) -> Result<Value> {
    validate_metadata(value, false)?;
    let mut result = value.clone();
    if let Some(columns) = result.get_mut("columns").and_then(Value::as_array_mut) {
        for column in columns {
            // validate_metadata requires a columnName. Do not send its resolved ID too.
            column
                .as_object_mut()
                .expect("validated column")
                .remove("columnId");
        }
    }
    Ok(result)
}

fn build_metadata(before: &Value, patch: &Value, overwrite: bool) -> Result<Value> {
    validate_metadata(before, false)?;
    validate_metadata(patch, true)?;
    if overwrite {
        return Ok(patch.clone());
    }
    if patch.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err(FabioError::invalid_input(
            "Empty metadata patch changes nothing; provide description, tags or columns",
        )
        .into());
    }
    let mut result = writable_metadata(before)?;
    merge_fields(&mut result, patch);
    if let Some(columns) = patch.get("columns").and_then(Value::as_array) {
        if result.get("columns").is_none() {
            result["columns"] = json!([]);
        }
        let existing = result["columns"].as_array_mut().expect("validated columns");
        for column in columns {
            if let Some(target) = existing
                .iter_mut()
                .find(|c| c["columnName"] == column["columnName"])
            {
                merge_fields(target, column);
            } else {
                existing.push(column.clone());
            }
        }
    }
    Ok(result)
}

pub(super) async fn get(cli: &Cli, client: &FabricClient, target: &Target) -> Result<()> {
    let result = client
        .get_onelake_table_api(&target.path()?)
        .await
        .map_err(metadata_read_error)?;
    validate_metadata(&result, false)?;
    output::render_object(cli, &result, "table");
    Ok(())
}

fn metadata_read_error(mut error: anyhow::Error) -> anyhow::Error {
    if let Some(fabio) = error.downcast_mut::<FabioError>()
        && fabio
            .message
            .contains("directory for the Iceberg table is missing")
        && fabio.message.contains("metadata files")
    {
        fabio.hint = Some("Table metadata resolution failed; an internal shortcut or table-format metadata may not be supported in this environment. Inspect the source table and shortcut separately, read-only. Do not create, delete or rewrite metadata files, enable format conversion, or bypass the shortcut automatically. No metadata POST was attempted by this command.".to_string());
    }
    error
}

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    target: &Target,
    file: Option<&str>,
    content: Option<&str>,
    overwrite: bool,
) -> Result<()> {
    let path = target.path()?;
    let changes = super::read_json_input(file, content, "update-table-metadata")?;
    validate_metadata(&changes, true)?;
    if !overwrite && changes.as_object().is_some_and(serde_json::Map::is_empty) {
        return Err(FabioError::invalid_input("Empty metadata patch changes nothing").into());
    }
    let before = client
        .get_onelake_table_api(&path)
        .await
        .map_err(metadata_read_error)?;
    let after = build_metadata(&before, &changes, overwrite)?;
    let mut warnings = vec![
        "POST replaces all additional metadata. Updates through a shortcut affect its target table, not a separate shortcut copy.",
        "Read-modify-write is not atomic; coordinate writers. Do not retry an uncertain submission or treat a matching readback as permanent concurrency protection.",
        "Columns are addressed by columnName. Resolved columnId fields from GET are omitted from writes because the service rejects both identifiers together.",
    ];
    if overwrite {
        warnings.push("--overwrite is active: all omitted table/column descriptions and tags are removed. This replacement is irreversible without a separately retained baseline.");
    }
    let preview = json!({"workspaceId":target.workspace,"itemId":target.id,"schema":target.schema,"table":target.table,
        "before":before,"after":after,"overwrite":overwrite,"warnings":warnings});
    if output::dry_run_guard(cli, "item update-table-metadata", &preview) {
        return Ok(());
    }
    let acknowledgement = client.post_onelake_table_api(&path, &after).await?;
    let verification = match client.get_onelake_table_api(&path).await {
        Ok(value) => writable_metadata(&value).map(|normalized| (value, normalized)),
        Err(error) => Err(error),
    };
    let (readback, verified, verification_error) = match verification {
        Ok((value, normalized)) => (Some(value), normalized == after, None),
        Err(error) => (
            None,
            false,
            Some(
                error
                    .downcast_ref::<FabioError>()
                    .map_or(ErrorCode::ApiError, |e| e.code)
                    .to_string(),
            ),
        ),
    };
    let result = json!({"status":"acknowledged","destructive":true,"verified":verified,
        "metadata":acknowledgement,"readback":readback,"verificationError":verification_error,"retriable":false,"warnings":warnings});
    output::render_object(cli, &result, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_merge_preserves_unmentioned_fields_tags_and_columns() {
        let before = json!({"description":"before","tags":{"keep":"yes","owner":"old"},"future":"preserved",
            "columns":[{"columnName":"id","description":"identifier","tags":{"keep":"yes"}},{"columnName":"amount","description":"keep"}]});
        let patch =
            json!({"tags":{"owner":"new"},"columns":[{"columnName":"id","tags":{"role":"key"}}]});
        let after = build_metadata(&before, &patch, false).unwrap();
        assert_eq!(after["description"], "before");
        assert_eq!(after["tags"]["keep"], "yes");
        assert_eq!(after["tags"]["owner"], "new");
        assert_eq!(after["columns"][0]["description"], "identifier");
        assert_eq!(
            after["columns"][0]["tags"],
            json!({"keep":"yes","role":"key"})
        );
        assert_eq!(after["columns"][1], before["columns"][1]);
        assert_eq!(after["future"], "preserved");
        assert_eq!(
            build_metadata(&before, &json!({}), true).unwrap(),
            json!({})
        );
    }

    #[test]
    fn resolved_column_ids_are_not_replayed_as_second_write_identifiers() {
        let response = json!({"columns":[{"columnName":"id","columnId":1,"description":"keep"}]});
        let desired =
            json!({"description":"new","columns":[{"columnName":"id","description":"keep"}]});
        assert_eq!(
            build_metadata(&response, &json!({"description":"new"}), false).unwrap(),
            desired
        );
        assert!(
            validate_metadata(&response, true).is_err(),
            "ambiguous user-supplied identifiers are not silently corrected"
        );
        assert_eq!(
            writable_metadata(&response).unwrap()["columns"][0].get("columnId"),
            None
        );
    }

    #[test]
    fn shortcut_metadata_read_error_teaches_inspection_not_file_repair() {
        let error = metadata_read_error(FabioError::api_error("The 'metadata' directory for the Iceberg table is missing the expected metadata files.").into());
        let error = error.downcast_ref::<FabioError>().unwrap();
        assert_eq!(error.code, ErrorCode::ApiError);
        assert!(
            error
                .hint
                .as_deref()
                .unwrap()
                .contains("Do not create, delete or rewrite metadata files")
        );
    }

    #[test]
    fn metadata_rejects_bad_types_duplicates_and_limits() {
        for value in [
            json!([]),
            json!({"description":null}),
            json!({"description":"x".repeat(1025)}),
            json!({"tags":{"a":1}}),
            json!({"columns":[{"columnName":"a"},{"columnName":"a"}]}),
            json!({"columns":[{}]}),
            json!({"typo":"x"}),
            json!({"tags":{ "a":"x".repeat(257)}}),
            json!({"tags":{("k".repeat(129)):"v"}}),
        ] {
            assert!(validate_metadata(&value, true).is_err());
        }
        assert!(validate_metadata(&json!({"description":"é".repeat(1024)}), true).is_ok());
        assert!(build_metadata(&json!({}), &json!({}), false).is_err());
    }

    #[test]
    fn table_scope_is_exact_and_encodes_names() {
        let mut target = Target {
            workspace: uuid::Uuid::nil().to_string(),
            id: uuid::Uuid::nil().to_string(),
            schema: "schema name".to_string(),
            table: "sales #1".to_string(),
        };
        assert!(
            target
                .path()
                .unwrap()
                .contains("schemas/schema%20name/tables/sales%20%231/additionalMetadata")
        );
        for invalid in ["", "*", "../tables", ".", "a\\b", "\n"] {
            target.table = invalid.to_string();
            assert!(target.path().is_err());
        }
    }
}
