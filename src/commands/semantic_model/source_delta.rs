//! Conservative classic-Delta metadata checks, not a Delta snapshot engine.
use crate::commands::diagnostics::{Check, Status};
use serde_json::{Value, json};
use std::collections::HashSet;

pub(super) fn commit_version(name: &str) -> Option<u64> {
    let version = name.strip_suffix(".json")?;
    (version.len() == 20 && version.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| version.parse().ok())
        .flatten()
}
pub(super) fn checkpoint_version(name: &str) -> Option<u64> {
    let (version, _) = name.split_once(".checkpoint")?;
    (version.len() == 20
        && version.bytes().all(|byte| byte.is_ascii_digit())
        && std::path::Path::new(name).extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("parquet") || ext.eq_ignore_ascii_case("json")
        }))
    .then(|| version.parse().ok())
    .flatten()
}

#[derive(Default)]
pub(super) struct Snapshot {
    latest: Option<u64>,
    expected: Option<u64>,
    protocol: Option<(u64, Value)>,
    metadata: Option<(u64, Value)>,
    files: usize,
    deletion_vectors: usize,
}
impl Snapshot {
    pub fn push(&mut self, version: u64, bytes: &[u8]) -> std::result::Result<(), &'static str> {
        if self.files > 0 && self.expected != Some(version) {
            return Err("non_contiguous_commit_suffix");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "non_utf8_commit")?;
        let mut protocol = None;
        let mut metadata = None;
        let mut actions = 0;
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let value: Value = serde_json::from_str(line).map_err(|_| "malformed_commit_json")?;
            let object = value
                .as_object()
                .filter(|object| object.len() == 1)
                .ok_or("malformed_commit_action")?;
            let (key, body) = object.iter().next().expect("one action");
            if !body.is_object() {
                return Err("malformed_commit_action");
            }
            match key.as_str() {
                "protocol" => {
                    if protocol.replace(body.clone()).is_some() {
                        return Err("conflicting_protocol_action");
                    }
                }
                "metaData" => {
                    if metadata.replace(body.clone()).is_some() {
                        return Err("conflicting_metadata_action");
                    }
                }
                "add" | "remove" => {
                    if let Some(vector) =
                        body.get("deletionVector").filter(|value| !value.is_null())
                    {
                        validate_deletion_vector(vector)?;
                        self.deletion_vectors += 1;
                    }
                }
                "txn" | "commitInfo" | "cdc" | "domainMetadata" => {}
                "sidecar" | "checkpointMetadata" => return Err("checkpoint_replay_required"),
                _ => return Err("unrecognized_delta_action"),
            }
            actions += 1;
        }
        if actions == 0 {
            return Err("empty_delta_commit");
        }
        if self.latest.is_none() {
            self.latest = Some(version);
        }
        if self.protocol.is_none() {
            self.protocol = protocol.map(|value| (version, value));
        }
        if self.metadata.is_none() {
            self.metadata = metadata.map(|value| (version, value));
        }
        self.expected = version.checked_sub(1);
        self.files += 1;
        Ok(())
    }
    pub const fn complete(&self) -> bool {
        self.protocol.is_some() && self.metadata.is_some()
    }
    pub fn coverage(&self) -> Value {
        json!({"latestObservedCommit":self.latest,"protocolCommit":self.protocol.as_ref().map(|(version,_)|version),"schemaCommit":self.metadata.as_ref().map(|(version,_)|version),"jsonFilesInspected":self.files,"deletionVectorDescriptorsObserved":self.deletion_vectors,"atomicSnapshot":false})
    }
    pub fn protocol(&self, next: String) -> Check {
        const WRITER_ONLY: &[&str] = &[
            "appendOnly",
            "invariants",
            "checkConstraints",
            "generatedColumns",
            "allowColumnDefaults",
            "changeDataFeed",
            "identityColumns",
            "rowTracking",
            "domainMetadata",
            "icebergCompatV1",
            "icebergCompatV2",
            "clustering",
            "inCommitTimestamp",
            "materializePartitionColumns",
        ];
        let body = &self.protocol.as_ref().expect("complete snapshot").1;
        let reader = body["minReaderVersion"].as_u64();
        let writer = body["minWriterVersion"].as_u64();
        // Delta's protocol explicitly distinguishes writer-only features from
        // reader requirements. Writer v7 does not imply reader v3. This closed
        // list comes from delta-io/delta PROTOCOL.md (valid feature names).
        let writers = feature_set(body.get("writerFeatures"));
        let readers = feature_set(body.get("readerFeatures"));
        let writer_only: HashSet<_> = WRITER_ONLY.iter().copied().collect();
        let recognized: Vec<_> = WRITER_ONLY
            .iter()
            .copied()
            .filter(|name| writers.as_ref().is_some_and(|values| values.contains(name)))
            .collect();
        let reader_flags: Vec<_> = [
            "columnMapping",
            "deletionVectors",
            "timestampNtz",
            "typeWidening",
            "v2Checkpoint",
            "vacuumProtocolCheck",
        ]
        .into_iter()
        .filter(|name| readers.as_ref().is_some_and(|values| values.contains(name)))
        .collect();
        let reader_known = match reader {
            Some(1 | 2) => readers.as_ref().is_some_and(HashSet::is_empty),
            Some(3) => {
                writer == Some(7)
                    && body.get("readerFeatures").is_some()
                    && readers.as_ref().is_some_and(|values| {
                        values
                            .iter()
                            .all(|feature| matches!(*feature, "columnMapping" | "deletionVectors"))
                    })
            }
            _ => false,
        };
        let required: HashSet<_> = if reader == Some(2) {
            HashSet::from(["columnMapping"])
        } else if reader == Some(3) {
            readers.clone().unwrap_or_default()
        } else {
            HashSet::new()
        };
        let writer_known = match writer {
            Some(1..=6) => {
                reader != Some(3)
                    && body.get("writerFeatures").is_none()
                    && (reader != Some(2) || writer.is_some_and(|version| version >= 5))
            }
            Some(7) => {
                body.get("writerFeatures").is_some()
                    && writers.as_ref().is_some_and(|values| {
                        required.is_subset(values)
                            && values.iter().all(|feature| {
                                writer_only.contains(feature) || required.contains(feature)
                            })
                    })
            }
            _ => false,
        };
        let known = reader_known
            && writer_known
            && (self.deletion_vectors == 0 || required.contains("deletionVectors"));
        Check::new(
            "delta_protocol",
            if known {
                Status::Ready
            } else {
                Status::Unknown
            },
            "Recognized metadata profiles cover basic, column-mapping and deletion-vector reader declarations with required writer features. Vector files, row masks and data files are not validated; other/future features remain unknown.",
            json!({"minReaderVersion":reader,"minWriterVersion":writer,"protocolProfileRecognized":known,"columnMappingReader":required.contains("columnMapping"),"deletionVectorReader":required.contains("deletionVectors"),"recognizedReaderFeatures":reader_flags,"recognizedWriterOnlyFeatures":recognized,"writerFeatureCount":body["writerFeatures"].as_array().map(Vec::len)}),
            next,
        )
    }
    pub fn schema(&self, next: String) -> Check {
        let body = &self.metadata.as_ref().expect("complete snapshot").1;
        let result = schema_summary(body, &self.protocol.as_ref().expect("complete snapshot").1);
        let (status, evidence) = match result {
            Ok((count, unsupported, unknown)) => (
                if unsupported > 0 {
                    Status::NeedsAction
                } else if unknown > 0 {
                    Status::Unknown
                } else {
                    Status::Ready
                },
                json!({"columnCount":count,"unsupportedComplexOrBinaryColumns":unsupported,"unrecognizedColumnTypes":unknown,"namesAndValuesIncluded":false}),
            ),
            Err(reason) => (Status::Unknown, json!({"reason":reason})),
        };
        Check::new(
            "delta_schema",
            status,
            "Primitive schema evidence only. Direct Lake does not support complex/binary columns; no column is silently dropped or converted. Unrecognized types/configuration remain unknown. Values, precision, statistics and service inference are not validated.",
            evidence,
            next,
        )
    }
}

fn feature_set(value: Option<&Value>) -> Option<HashSet<&str>> {
    let Some(value) = value else {
        return Some(HashSet::new());
    };
    let values = value.as_array()?;
    let features: HashSet<_> = values.iter().map(Value::as_str).collect::<Option<_>>()?;
    (features.len() == values.len()).then_some(features)
}

fn validate_deletion_vector(value: &Value) -> std::result::Result<(), &'static str> {
    let storage = value["storageType"]
        .as_str()
        .filter(|kind| matches!(*kind, "i" | "u" | "p"))
        .ok_or("unrecognized_deletion_vector_storage")?;
    value["pathOrInlineDv"]
        .as_str()
        .filter(|path| !path.is_empty() && !path.chars().any(char::is_control))
        .ok_or("malformed_deletion_vector_reference")?;
    value["sizeInBytes"]
        .as_u64()
        .filter(|value| i32::try_from(*value).is_ok())
        .ok_or("malformed_deletion_vector_size")?;
    value["cardinality"]
        .as_u64()
        .filter(|value| i64::try_from(*value).is_ok())
        .ok_or("malformed_deletion_vector_cardinality")?;
    if let Some(offset) = value.get("offset").filter(|value| !value.is_null())
        && (storage == "i"
            || offset
                .as_u64()
                .is_none_or(|value| i32::try_from(value).is_err()))
    {
        return Err("malformed_deletion_vector_offset");
    }
    Ok(())
}

fn schema_summary(
    body: &Value,
    protocol: &Value,
) -> std::result::Result<(usize, usize, usize), &'static str> {
    if body["format"]["provider"] != "parquet" || !body["configuration"].is_object() {
        return Err("unrecognized_delta_metadata_format");
    }
    let config = &body["configuration"];
    let mapping = match config
        .get("delta.columnMapping.mode")
        .and_then(Value::as_str)
    {
        None if config.get("delta.columnMapping.mode").is_none() => false,
        Some("none") => false,
        Some("name" | "id") => true,
        _ => return Err("unrecognized_column_mapping_mode"),
    };
    let reader = protocol["minReaderVersion"].as_u64();
    if mapping
        && !(reader == Some(2)
            || (reader == Some(3)
                && protocol["readerFeatures"]
                    .as_array()
                    .is_some_and(|values| values.iter().any(|value| value == "columnMapping"))))
    {
        return Err("column_mapping_protocol_mismatch");
    }
    match config.get("delta.enableDeletionVectors") {
        None => {}
        Some(Value::String(value)) if value == "false" => {}
        Some(Value::String(value))
            if value == "true"
                && protocol["readerFeatures"].as_array().is_some_and(|values| {
                    values.iter().any(|value| value == "deletionVectors")
                }) => {}
        _ => return Err("unrecognized_deletion_vector_configuration"),
    }
    let schema: Value = serde_json::from_str(
        body["schemaString"]
            .as_str()
            .ok_or("missing_schema_string")?,
    )
    .map_err(|_| "malformed_schema_string")?;
    if schema["type"] != "struct" {
        return Err("unrecognized_schema_root");
    }
    let fields = schema["fields"]
        .as_array()
        .filter(|fields| !fields.is_empty() && fields.len() <= 1000)
        .ok_or("missing_or_unbounded_schema_fields")?;
    if mapping {
        validate_mapping(fields, config)?;
    }
    let mut names = HashSet::new();
    let mut unsupported = 0;
    let mut unknown = 0;
    for field in fields {
        let name = field["name"]
            .as_str()
            .filter(|name| {
                !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
            })
            .ok_or("malformed_column_name")?;
        if !names.insert(name.to_ascii_lowercase()) || !field["nullable"].is_boolean() {
            return Err("conflicting_or_malformed_column");
        }
        if field["type"].is_object()
            && !matches!(
                field["type"]["type"].as_str(),
                Some("array" | "map" | "struct")
            )
        {
            return Err("unrecognized_type_object");
        }
        match field["type"]
            .as_str()
            .or_else(|| field["type"]["type"].as_str())
        {
            Some(
                "string" | "boolean" | "long" | "integer" | "int" | "short" | "byte" | "double"
                | "float" | "date" | "timestamp",
            ) => {}
            Some("binary" | "array" | "map" | "struct") => unsupported += 1,
            Some(_) => unknown += 1,
            None => return Err("malformed_column_type"),
        }
    }
    let partitions = body["partitionColumns"]
        .as_array()
        .ok_or("malformed_partition_metadata")?;
    if partitions.iter().any(|value| {
        value
            .as_str()
            .is_none_or(|name| !names.contains(&name.to_ascii_lowercase()))
    }) {
        return Err("partition_column_missing_from_schema");
    }
    Ok((fields.len(), unsupported, unknown))
}

fn validate_mapping(fields: &[Value], config: &Value) -> std::result::Result<(), &'static str> {
    let mut ids = HashSet::new();
    let mut physical_names = HashSet::new();
    let mut max_id = 0;
    for field in fields {
        let id = field["metadata"]["delta.columnMapping.id"]
            .as_u64()
            .filter(|id| *id > 0 && i64::try_from(*id).is_ok())
            .ok_or("column_mapping_id_missing")?;
        let physical = field["metadata"]["delta.columnMapping.physicalName"]
            .as_str()
            .filter(|name| {
                !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
            })
            .ok_or("column_mapping_physical_name_missing")?;
        if !ids.insert(id) || !physical_names.insert(physical.to_ascii_lowercase()) {
            return Err("conflicting_column_mapping");
        }
        max_id = max_id.max(id);
    }
    if config["delta.columnMapping.maxColumnId"]
        .as_str()
        .and_then(|value| value.parse::<u64>().ok())
        .is_none_or(|value| value < max_id)
    {
        return Err("column_mapping_max_id_invalid");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn commit(kind: &str) -> Vec<u8> {
        let schema=json!({"type":"struct","fields":[{"name":"private","type":kind,"nullable":true,"metadata":{}}]}).to_string();
        format!("{}\n{}\n",json!({"protocol":{"minReaderVersion":1,"minWriterVersion":2}}),json!({"metaData":{"format":{"provider":"parquet"},"schemaString":schema,"configuration":{},"partitionColumns":[]}})).into_bytes()
    }
    #[test]
    fn commit_suffix_requires_contiguity_and_never_exposes_values() {
        let mut snapshot = Snapshot::default();
        snapshot
            .push(1, br#"{"commitInfo":{"secret":"private"}}"#)
            .unwrap();
        assert_eq!(snapshot.push(0, &commit("long")), Ok(()));
        assert!(snapshot.complete());
        assert_eq!(snapshot.protocol("inspect".into()).status, Status::Ready);
        let check = snapshot.schema("inspect".into());
        assert_eq!(check.status, Status::Ready);
        assert!(!check.evidence.to_string().contains("private"));
        let mut snapshot = Snapshot::default();
        snapshot.push(2, br#"{"commitInfo":{}}"#).unwrap();
        assert_eq!(
            snapshot.push(0, &commit("long")),
            Err("non_contiguous_commit_suffix")
        );
    }
    #[test]
    fn unsupported_and_new_schema_types_are_not_silently_accepted() {
        for (kind, status) in [
            ("binary", Status::NeedsAction),
            ("variant", Status::Unknown),
            ("decimal(10,2)", Status::Unknown),
        ] {
            let mut snapshot = Snapshot::default();
            snapshot.push(0, &commit(kind)).unwrap();
            assert_eq!(snapshot.schema("inspect".into()).status, status);
        }
        assert_eq!(commit_version("00000000000000000001.json"), Some(1));
        assert_eq!(commit_version("../1.json"), None);
        assert_eq!(
            checkpoint_version("00000000000000000010.checkpoint.parquet"),
            Some(10)
        );
    }

    #[test]
    fn writer_only_features_are_not_confused_with_reader_requirements() {
        for (reader, writer, features, expected) in [
            (1, 7, json!(["appendOnly", "invariants"]), Status::Ready),
            (1, 7, json!(["private_unknown"]), Status::Unknown),
            (1, 7, json!(["appendOnly", "appendOnly"]), Status::Unknown),
            (3, 7, json!(["appendOnly", "invariants"]), Status::Unknown),
        ] {
            let mut snapshot = Snapshot::default();
            snapshot.push(0, &commit("long")).unwrap();
            snapshot.protocol = Some((
                0,
                json!({"minReaderVersion":reader,"minWriterVersion":writer,"writerFeatures":features}),
            ));
            let check = snapshot.protocol("inspect".into());
            assert_eq!(check.status, expected);
            assert!(!check.evidence.to_string().contains("private"));
        }
    }

    #[test]
    fn column_mapping_profile_requires_consistent_private_mapping_metadata() {
        let mut snapshot = Snapshot::default();
        snapshot.push(0, &commit("long")).unwrap();
        snapshot.protocol = Some((
            0,
            json!({"minReaderVersion":3,"minWriterVersion":7,"readerFeatures":["columnMapping"],"writerFeatures":["columnMapping","appendOnly","invariants"]}),
        ));
        let metadata = &mut snapshot.metadata.as_mut().unwrap().1;
        metadata["configuration"] =
            json!({"delta.columnMapping.mode":"name","delta.columnMapping.maxColumnId":"1"});
        let mut schema: Value =
            serde_json::from_str(metadata["schemaString"].as_str().unwrap()).unwrap();
        schema["fields"][0]["metadata"] = json!({"delta.columnMapping.id":1,"delta.columnMapping.physicalName":"private_physical"});
        metadata["schemaString"] = json!(schema.to_string());
        assert_eq!(snapshot.protocol("inspect".into()).status, Status::Ready);
        let check = snapshot.schema("inspect".into());
        assert_eq!(check.status, Status::Ready);
        assert!(!check.evidence.to_string().contains("private"));
        schema["fields"][0]["metadata"] = json!({});
        snapshot.metadata.as_mut().unwrap().1["schemaString"] = json!(schema.to_string());
        assert_eq!(snapshot.schema("inspect".into()).status, Status::Unknown);
    }

    #[test]
    fn deletion_vector_metadata_is_structural_not_a_row_mask_or_file_scan() {
        let mut snapshot = Snapshot::default();
        snapshot.push(0, &commit("long")).unwrap();
        snapshot.protocol = Some((
            0,
            json!({"minReaderVersion":3,"minWriterVersion":7,"readerFeatures":["deletionVectors"],"writerFeatures":["deletionVectors","appendOnly","invariants"]}),
        ));
        snapshot.metadata.as_mut().unwrap().1["configuration"] =
            json!({"delta.enableDeletionVectors":"true"});
        assert_eq!(snapshot.protocol("inspect".into()).status, Status::Ready);
        assert_eq!(snapshot.schema("inspect".into()).status, Status::Ready);
        let vector = json!({"storageType":"u","pathOrInlineDv":"private_reference","sizeInBytes":10,"cardinality":2});
        assert_eq!(validate_deletion_vector(&vector), Ok(()));
        let mut bad = vector.clone();
        bad["sizeInBytes"] = json!(-1);
        assert!(validate_deletion_vector(&bad).is_err());
        let mut snapshot = Snapshot::default();
        snapshot
            .push(
                1,
                json!({"add":{"deletionVector":vector}})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
        snapshot.push(0, &commit("long")).unwrap();
        assert_eq!(snapshot.protocol("inspect".into()).status, Status::Unknown);
        assert!(!snapshot.coverage().to_string().contains("private"));
    }
}
