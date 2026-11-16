//! Authored ALM references are symbolic; runtime topology contains resolved IDs.
use std::collections::BTreeSet;

use anyhow::Result;
use serde_json::Value;

use crate::errors::{ErrorCode, FabioError};

const FIELDS: [&str; 4] = [
    "connectionReference",
    "itemReference",
    "tableNameReference",
    "tableNamesReference",
];

pub fn parse_reference(value: &str) -> Result<(&str, &str)> {
    let invalid = || {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Expected a symbolic variable-library reference",
            "Use the literal form $(/**/LibraryName/VariableName), with one nonempty library and variable name. Quote JSON so the shell does not expand it.",
        )
    };
    let body = value
        .strip_prefix("$(/**/")
        .and_then(|v| v.strip_suffix(')'))
        .ok_or_else(invalid)?;
    let (library, variable) = body.split_once('/').ok_or_else(invalid)?;
    if [library, variable].iter().any(|name| {
        name.trim().is_empty()
            || matches!(*name, "." | "..")
            || name.contains(['/', '\\', '(', ')'])
            || name.chars().any(char::is_control)
    }) {
        return Err(invalid().into());
    }
    Ok((library, variable))
}

fn supported(kind: &str, component: &str, field: &str) -> bool {
    let source = kind == "source";
    let is = |names: &[&str]| {
        names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(component))
    };
    match field {
        "connectionReference" => {
            source
                && is(&[
                    "AmazonKinesis",
                    "AmazonMSKKafka",
                    "ApacheKafka",
                    "AzureCosmosDBCDC",
                    "AzureDataExplorer",
                    "AzureEventHub",
                    "AzureEventHubExtended",
                    "AzureIoTHub",
                    "AzureIoTHubExtended",
                    "AzureSQLDBCDC",
                    "AzureSQLMIDBCDC",
                    "AzureServiceBus",
                    "ConfluentCloud",
                    "GooglePubSub",
                    "Http",
                    "MongoDBCDC",
                    "Mqtt",
                    "MySQLCDC",
                    "OracleDBCDC",
                    "PostgreSQLCDC",
                    "SQLServerOnVMDBCDC",
                    "SolacePubSub",
                ])
        }
        "itemReference" => {
            if source {
                is(&[
                    "BusinessEvents",
                    "FabricAnomalyDetectionEvents",
                    "LakehouseChangeFeed",
                    "MirroredDatabaseChangeFeed",
                    "ReferenceLakehouse",
                ])
            } else {
                is(&[
                    "Activator",
                    "BusinessEvents",
                    "Eventhouse",
                    "Lakehouse",
                    "Notebook",
                ])
            }
        }
        "tableNameReference" => {
            if source {
                is(&[
                    "AzureSQLDBCDC",
                    "AzureSQLMIDBCDC",
                    "MySQLCDC",
                    "PostgreSQLCDC",
                    "SQLServerOnVMDBCDC",
                ])
            } else {
                is(&["Eventhouse"])
            }
        }
        "tableNamesReference" => {
            source
                && is(&[
                    "AzureDataExplorer",
                    "LakehouseChangeFeed",
                    "MirroredDatabaseChangeFeed",
                    "OracleDBCDC",
                ])
        }
        _ => false,
    }
}

pub fn validate_properties(kind: &str, component: &str, props: &Value) -> Result<()> {
    if !props.is_object() {
        return Err(
            FabioError::invalid_input("Eventstream properties must be a JSON object").into(),
        );
    }
    for field in FIELDS {
        let Some(value) = props.get(field) else {
            continue;
        };
        if !supported(kind, component, field) {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput,
                format!("{field} is not in this Eventstream {kind} contract"),
                "Variable support is property- and connector-specific. Inspect context best-practices eventstream-variables; CustomEndpoint, Cribl and SAPDatasphere do not expose variable references in the published matrix.").into());
        }
        parse_reference(value.as_str().ok_or_else(|| {
            FabioError::invalid_input("Variable references must be symbolic strings")
        })?)?;
        let concrete: &[&str] = match field {
            "connectionReference" => &["dataConnectionId"],
            "itemReference" => &["workspaceId", "itemId"],
            "tableNameReference" => &["tableName"],
            "tableNamesReference" => &["tableNames"],
            _ => &[],
        };
        if concrete
            .iter()
            .any(|key| props.get(*key).is_some_and(|value| !value.is_null()))
        {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput,
                format!("{field} cannot be combined with its resolved identifiers"),
                "Author symbolic references in the definition, or use explicit identifiers, not both. Runtime topology is resolved and is not a portable authored definition.").into());
        }
    }
    Ok(())
}

/// Extract only documented reference properties, not arbitrary strings containing `$(`.
pub fn references(value: &Value) -> Result<BTreeSet<(String, String)>> {
    let mut result = BTreeSet::new();
    for (field, kind) in [("sources", "source"), ("destinations", "destination")] {
        for node in value
            .get(field)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(props) = node.get("properties") else {
                continue;
            };
            if FIELDS.iter().any(|field| props.get(*field).is_some()) {
                let component = node["type"].as_str().ok_or_else(|| {
                    FabioError::invalid_input("Referenced Eventstream node is missing its type")
                })?;
                validate_properties(kind, component, props)?;
                for field in FIELDS {
                    if let Some(value) = props.get(field) {
                        let (library, variable) =
                            parse_reference(value.as_str().expect("validated reference"))?;
                        result.insert((library.to_string(), variable.to_string()));
                    }
                }
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn references_are_literal_exact_paths_not_shell_expressions() {
        assert_eq!(
            parse_reference("$(/**/Library/Target)").unwrap(),
            ("Library", "Target")
        );
        for invalid in [
            "",
            "$(Library/Target)",
            "$(/**//Target)",
            "$(/**/Library/)",
            "$(/**/../Target)",
            "$(/**/Library/a/b)",
            "$(/**/Library/$(run))",
        ] {
            assert!(parse_reference(invalid).is_err());
        }
    }

    #[test]
    fn property_matrix_rejects_unsupported_and_ambiguous_reference_shapes() {
        assert!(
            validate_properties(
                "source",
                "AzureEventHub",
                &json!({"connectionReference":"$(/**/L/C)"})
            )
            .is_ok()
        );
        assert!(
            validate_properties(
                "destination",
                "Eventhouse",
                &json!({"itemReference":"$(/**/L/I)","tableNameReference":"$(/**/L/T)"})
            )
            .is_ok()
        );
        for (kind, component, props) in [
            (
                "source",
                "CustomEndpoint",
                json!({"connectionReference":"$(/**/L/C)"}),
            ),
            (
                "source",
                "Cribl",
                json!({"connectionReference":"$(/**/L/C)"}),
            ),
            (
                "destination",
                "Eventhouse",
                json!({"itemReference":"$(/**/L/I)","itemId":"ambiguous"}),
            ),
            (
                "source",
                "AzureEventHub",
                json!({"connectionReference":null}),
            ),
        ] {
            assert!(validate_properties(kind, component, &props).is_err());
        }
    }

    #[test]
    fn extraction_is_deduplicated_and_preserves_symbolic_text() {
        let value = json!({"sources":[{"type":"AzureEventHub","properties":{"connectionReference":"$(/**/L/C)"}}],"destinations":[{"type":"Lakehouse","properties":{"itemReference":"$(/**/L/I)"}}],"note":"$(not a reference)"});
        assert_eq!(
            references(&value).unwrap(),
            BTreeSet::from([
                ("L".to_string(), "C".to_string()),
                ("L".to_string(), "I".to_string())
            ])
        );
        assert_eq!(
            value["sources"][0]["properties"]["connectionReference"],
            "$(/**/L/C)"
        );
    }
}
