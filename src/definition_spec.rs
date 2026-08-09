//! Source-of-truth for Fabric item-definition part requirements.
//!
//! Powers three agent-facing capabilities that must never drift from each other:
//! - `fabio item validate-definition` (offline structural validation of a
//!   definition envelope or a folder of parts, BEFORE the API round-trip);
//! - definition-authoring error hints (enumerate the required part paths and
//!   point at `fabio context schema <Type>`);
//! - `fabio context schema <Type>` (surfaces the canonical parts).
//!
//! Canonical part paths are what `getDefinition` / Git-integration export return
//! (and what `deploy` round-trips). The Fabric `updateDefinition` API is often
//! LENIENT and tolerates alias filenames (e.g. `dataflow.json` for a Dataflow,
//! `CopyJobV1.json` for a `CopyJob`), so a MISSING canonical part is a WARNING, not
//! an error, unless the caller opts into `--strict`. Universal envelope rules
//! (parts array shape, `path`/`payload`/`payloadType`, base64 decodability, JSON
//! validity of `.json` parts) are ALWAYS errors — those are deterministic and
//! catch the most common agent mistakes.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFINITION_REQUIREMENTS: &str =
    include_str!("commands/context/data/agent/definition_requirements.json");

/// Per-item-type definition part requirements.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypeSpec {
    /// Part paths that must ALL be present (canonical export paths).
    #[serde(default)]
    pub required_parts: Vec<String>,
    /// Groups where at least one path from each group must be present.
    #[serde(default)]
    pub required_one_of: Vec<Vec<String>>,
    /// Additional known-good part paths (informational; not required).
    #[serde(default)]
    pub optional_parts: Vec<String>,
    /// Alternate filenames the API also accepts for a required part.
    #[serde(default)]
    pub alias_parts: Vec<String>,
    /// The `definitionFormat` value for this type, when it has one.
    #[serde(default)]
    pub format: Option<String>,
    /// Human-readable authoring note.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RequirementsFile {
    types: BTreeMap<String, TypeSpec>,
}

static SPECS: LazyLock<BTreeMap<String, TypeSpec>> = LazyLock::new(|| {
    let parsed: RequirementsFile = serde_json::from_str(DEFINITION_REQUIREMENTS)
        .expect("definition_requirements.json must be valid JSON matching RequirementsFile");
    parsed.types
});

/// Normalize a type name for case/-/_-insensitive lookup.
fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '-' && *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

/// Return the canonical type key (as written in the data file) for a fuzzy input.
#[must_use]
pub fn canonical_type_name(item_type: &str) -> Option<&'static str> {
    let n = normalize(item_type);
    SPECS.keys().find(|k| normalize(k) == n).map(String::as_str)
}

/// Look up the definition spec for an item type (case/-/_-insensitive).
#[must_use]
pub fn spec_for(item_type: &str) -> Option<&'static TypeSpec> {
    let n = normalize(item_type);
    SPECS
        .iter()
        .find(|(k, _)| normalize(k) == n)
        .map(|(_, v)| v)
}

/// All item-type names that have a definition spec (sorted).
#[must_use]
pub fn known_types() -> Vec<&'static str> {
    SPECS.keys().map(String::as_str).collect()
}

/// Build a rich, agent-oriented hint for a "definition input is missing/invalid"
/// error. Enumerates the required part path(s) and the `definitionFormat`, shows
/// the envelope shape, and points at the offline validator and `context schema`.
#[must_use]
pub fn definition_input_hint(item_type: &str, group: &str, command: &str) -> String {
    let canonical = canonical_type_name(item_type).unwrap_or(item_type);
    let mut parts_desc = String::new();
    if let Some(spec) = spec_for(item_type) {
        let mut names: Vec<String> = spec.required_parts.clone();
        for group in &spec.required_one_of {
            if !group.is_empty() {
                names.push(format!("one of [{}]", group.join(", ")));
            }
        }
        if !names.is_empty() {
            parts_desc = format!(" Required part(s): {}.", names.join(", "));
        }
        if let Some(fmt) = &spec.format {
            use std::fmt::Write as _;
            let _ = write!(parts_desc, " definitionFormat: {fmt}.");
        }
    }
    let first_part = spec_for(item_type)
        .and_then(|s| {
            s.required_parts
                .first()
                .or_else(|| s.required_one_of.first().and_then(|g| g.first()))
        })
        .map_or("<part-path>", String::as_str);

    format!(
        "Provide the definition with --file (a single raw part, base64-encoded for you) \
         or --content/--definition (the full envelope: \
         {{\"definition\":{{\"parts\":[{{\"path\":\"{first_part}\",\"payload\":\"<base64>\",\"payloadType\":\"InlineBase64\"}}]}}}}).{parts_desc} \
         Validate offline before sending: fabio item validate-definition --type {canonical} --file <envelope.json>. \
         See the full schema: fabio context schema {canonical}. \
         Example: fabio {group} {command} --workspace <WS> --id <ID> --file <file>."
    )
}

/// Build a Fabric `updateDefinition`/`create`-compatible body from raw input,
/// aligning fabio's emitted part paths with Fabric's canonical structure.
///
/// The `raw` input is interpreted as follows (in order):
/// 1. If it parses to JSON containing a `definition.parts` array, it is a full
///    envelope — passed through verbatim (normalized to `{"definition":{"parts":...}}`).
///    This lets agents round-trip a multi-part definition captured from
///    `get-definition` (the Fabric-aligned path for types like Dataflow that
///    REQUIRE multiple parts, e.g. `queryMetadata.json` + `mashup.pq`).
/// 2. If it parses to JSON with a top-level `parts` array, same treatment.
/// 3. Otherwise `raw` is treated as the raw bytes of a SINGLE part and wrapped
///    (base64-encoded) under `default_part_path` — the type's canonical part.
#[must_use]
pub fn build_update_definition_body(raw: &str, default_part_path: &str) -> Value {
    if let Ok(parsed) = serde_json::from_str::<Value>(raw) {
        let definition = parsed.get("definition");
        let parts = definition
            .and_then(|d| d.get("parts"))
            .or_else(|| parsed.get("parts"));
        if let Some(parts) = parts.and_then(Value::as_array) {
            let mut def = serde_json::json!({ "parts": parts });
            // Preserve a `format` (definitionFormat) if the caller supplied one
            // — e.g. a notebook `ipynb` envelope. Dropping it makes the server
            // misinterpret the payload (notebook-content.py treated as raw .py).
            if let Some(format) = definition
                .and_then(|d| d.get("format"))
                .or_else(|| parsed.get("format"))
            {
                def["format"] = format.clone();
            }
            return serde_json::json!({ "definition": def });
        }
    }
    serde_json::json!({
        "definition": {
            "parts": [{
                "path": default_part_path,
                "payload": BASE64.encode(raw.as_bytes()),
                "payloadType": "InlineBase64",
            }]
        }
    })
}

// ─── Offline validation ──────────────────────────────────────────────────────

/// Severity of a validation finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// A single validation finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub code: String,
    pub severity: Severity,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<String>,
}

impl Finding {
    fn error(code: &str, message: impl Into<String>, part: Option<String>) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Error,
            message: message.into(),
            part,
        }
    }

    fn warning(code: &str, message: impl Into<String>, part: Option<String>) -> Self {
        Self {
            code: code.to_string(),
            severity: Severity::Warning,
            message: message.into(),
            part,
        }
    }
}

/// Extract the `parts` array from a definition envelope. Accepts both
/// `{"definition":{"parts":[...]}}` and a bare `{"parts":[...]}`.
fn extract_parts(envelope: &Value) -> Option<&Vec<Value>> {
    envelope
        .get("definition")
        .and_then(|d| d.get("parts"))
        .or_else(|| envelope.get("parts"))
        .and_then(Value::as_array)
}

/// Does `paths` contain `wanted` (or, when `aliases` are given, any alias)?
fn part_present(paths: &[String], wanted: &str, aliases: &[String]) -> bool {
    if wanted.contains('<') {
        // Template placeholder like "<displayName>.rdl" — cannot match literally.
        return true;
    }
    paths.iter().any(|p| p == wanted) || aliases.iter().any(|a| paths.iter().any(|p| p == a))
}

/// Validate a definition envelope offline. `item_type`, when supplied, enables
/// per-type canonical-part checks (emitted as warnings). Returns all findings;
/// the caller decides validity (any `Error`, or any `Warning` under `--strict`).
#[must_use]
pub fn validate_definition(item_type: Option<&str>, envelope: &Value) -> Vec<Finding> {
    let mut findings = Vec::new();

    let Some(parts) = extract_parts(envelope) else {
        findings.push(Finding::error(
            "MISSING_PARTS",
            "Definition has no parts array. Expected {\"definition\":{\"parts\":[...]}} \
             (or {\"parts\":[...]}), where each part is \
             {\"path\":\"...\",\"payload\":\"<base64>\",\"payloadType\":\"InlineBase64\"}.",
            None,
        ));
        return findings;
    };

    if parts.is_empty() {
        findings.push(Finding::error(
            "EMPTY_PARTS",
            "The parts array is empty. A definition must contain at least one part.",
            None,
        ));
    }

    let mut present_paths: Vec<String> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        if let Some(path) = validate_part(i, part, &present_paths, &mut findings) {
            present_paths.push(path);
        }
    }

    // Per-type canonical-part checks (warnings — the API is often lenient).
    if let Some(item_type) = item_type {
        validate_type_requirements(item_type, &present_paths, &mut findings);
    }

    findings
}

/// Validate a single definition part. Returns the part's `path` when present
/// (so the caller can track the set of paths for per-type checks).
fn validate_part(
    index: usize,
    part: &Value,
    seen_paths: &[String],
    findings: &mut Vec<Finding>,
) -> Option<String> {
    let Some(path) = part
        .get("path")
        .and_then(Value::as_str)
        .filter(|p| !p.is_empty())
    else {
        findings.push(Finding::error(
            "MISSING_PART_PATH",
            format!("parts[{index}] is missing a non-empty \"path\"."),
            Some(format!("parts[{index}]")),
        ));
        return None;
    };

    if seen_paths.iter().any(|p| p == path) {
        findings.push(Finding::error(
            "DUPLICATE_PART",
            format!("Duplicate part path \"{path}\". Each part path must be unique."),
            Some(path.to_string()),
        ));
    }

    match part.get("payloadType").and_then(Value::as_str) {
        Some("InlineBase64") => {}
        Some(other) => {
            findings.push(Finding::error(
                "INVALID_PAYLOAD_TYPE",
                format!(
                    "{path}: payloadType \"{other}\" is not supported. Valid value: InlineBase64."
                ),
                Some(path.to_string()),
            ));
            return Some(path.to_string());
        }
        None => {
            findings.push(Finding::error(
                "MISSING_PAYLOAD_TYPE",
                format!("{path}: missing \"payloadType\". Valid value: InlineBase64."),
                Some(path.to_string()),
            ));
            return Some(path.to_string());
        }
    }

    let Some(payload) = part.get("payload").and_then(Value::as_str) else {
        findings.push(Finding::error(
            "MISSING_PAYLOAD",
            format!("{path}: missing \"payload\" (base64-encoded part content)."),
            Some(path.to_string()),
        ));
        return Some(path.to_string());
    };

    let Ok(decoded) = BASE64.decode(payload.as_bytes()) else {
        findings.push(Finding::error(
            "INVALID_BASE64",
            format!(
                "{path}: payload is not valid base64. With payloadType InlineBase64 the \
                 payload must be the standard base64 encoding of the raw part bytes."
            ),
            Some(path.to_string()),
        ));
        return Some(path.to_string());
    };

    validate_part_content(path, &decoded, findings);
    Some(path.to_string())
}

/// Content-level checks for known text part formats (JSON validity, `.platform`).
fn validate_part_content(path: &str, decoded: &[u8], findings: &mut Vec<Finding>) {
    let is_json = std::path::Path::new(path)
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if !is_json && path != ".platform" {
        return;
    }
    match serde_json::from_slice::<Value>(decoded) {
        Ok(json) => {
            if path == ".platform" {
                let meta = json.get("metadata");
                let has_type = meta
                    .and_then(|m| m.get("type"))
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty());
                let has_name = meta
                    .and_then(|m| m.get("displayName"))
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty());
                if !has_type || !has_name {
                    findings.push(Finding::warning(
                        "PLATFORM_MISSING_METADATA",
                        ".platform should contain metadata.type and metadata.displayName."
                            .to_string(),
                        Some(path.to_string()),
                    ));
                }
            }
        }
        Err(e) => {
            findings.push(Finding::error(
                "INVALID_JSON_PART",
                format!("{path}: decoded payload is not valid JSON: {e}"),
                Some(path.to_string()),
            ));
        }
    }
}

/// Per-type canonical-part checks (all warnings — Fabric tolerates aliases).
fn validate_type_requirements(
    item_type: &str,
    present_paths: &[String],
    findings: &mut Vec<Finding>,
) {
    let Some(spec) = spec_for(item_type) else {
        findings.push(Finding::warning(
            "UNKNOWN_ITEM_TYPE",
            format!(
                "No definition spec for item type \"{item_type}\". \
                 List valid types with: fabio item create --help, \
                 or omit --type to validate only the envelope structure."
            ),
            None,
        ));
        return;
    };
    let canonical = canonical_type_name(item_type).unwrap_or(item_type);
    for required in &spec.required_parts {
        if !part_present(present_paths, required, &spec.alias_parts) {
            findings.push(Finding::warning(
                "MISSING_CANONICAL_PART",
                format!(
                    "Expected canonical part \"{required}\" for type {canonical} \
                     was not found (found: [{}]).",
                    present_paths.join(", ")
                ),
                Some(required.clone()),
            ));
        }
    }
    for group in &spec.required_one_of {
        let satisfied = group
            .iter()
            .any(|g| part_present(present_paths, g, &spec.alias_parts));
        if !satisfied {
            findings.push(Finding::warning(
                "MISSING_ONE_OF",
                format!(
                    "Type {canonical} expects at least one of [{}] (found: [{}]).",
                    group.join(", "),
                    present_paths.join(", ")
                ),
                None,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn b64(s: &str) -> String {
        BASE64.encode(s.as_bytes())
    }

    #[test]
    fn requirements_file_parses() {
        // Forces the LazyLock and asserts a representative set exists.
        assert!(spec_for("Notebook").is_some());
        assert!(spec_for("SparkJobDefinition").is_some());
        assert!(known_types().len() > 20);
    }

    #[test]
    fn graph_model_definition_is_five_parts_not_graphmodel_json() {
        // Regression: a graph model definition is graphType.json + dataSources.json +
        // graphDefinition.json (+ optional styling/settings), NOT a single GraphModel.json.
        let spec = spec_for("GraphModel").expect("GraphModel spec");
        assert!(spec.required_parts.contains(&"graphType.json".to_string()));
        assert!(
            spec.required_parts
                .contains(&"dataSources.json".to_string())
        );
        assert!(
            spec.required_parts
                .contains(&"graphDefinition.json".to_string())
        );
        assert!(
            !spec.required_parts.contains(&"GraphModel.json".to_string()),
            "the bogus single-part GraphModel.json must be gone"
        );
        // stylingConfiguration.json is REQUIRED by updateDefinition (live-verified:
        // omitting it fails with GraphItemDefinitionIncomplete), so it must be a
        // required part; graphSettings.json is genuinely optional (4-part push OK).
        assert!(
            spec.required_parts
                .contains(&"stylingConfiguration.json".to_string()),
            "stylingConfiguration.json must be required (API rejects a definition without it)"
        );
        assert!(
            spec.optional_parts
                .contains(&"graphSettings.json".to_string()),
            "graphSettings.json is optional"
        );
        // The note must warn about the portal-init load gate.
        assert!(
            spec.note.as_deref().unwrap_or("").contains("PORTAL-GATED"),
            "note must document the portal-init load limitation"
        );
    }

    #[test]
    fn lookup_is_case_and_separator_insensitive() {
        assert_eq!(
            canonical_type_name("spark-job-definition"),
            Some("SparkJobDefinition")
        );
        assert_eq!(
            canonical_type_name("SPARKJOBDEFINITION"),
            Some("SparkJobDefinition")
        );
        assert_eq!(canonical_type_name("data_pipeline"), Some("DataPipeline"));
        assert_eq!(canonical_type_name("NotARealType"), None);
    }

    #[test]
    fn spark_job_definition_part_is_v1_filename_v2_format() {
        let spec = spec_for("SparkJobDefinition").unwrap();
        assert_eq!(spec.required_parts, vec!["SparkJobDefinitionV1.json"]);
        assert_eq!(spec.format.as_deref(), Some("SparkJobDefinitionV2"));
    }

    #[test]
    fn hint_enumerates_parts_and_points_at_schema() {
        let hint = definition_input_hint(
            "spark-job-definition",
            "spark-job-definition",
            "update-definition",
        );
        assert!(hint.contains("SparkJobDefinitionV1.json"));
        assert!(hint.contains("definitionFormat: SparkJobDefinitionV2"));
        assert!(hint.contains("fabio context schema SparkJobDefinition"));
        assert!(hint.contains("fabio item validate-definition --type SparkJobDefinition"));
    }

    #[test]
    fn valid_notebook_definition_has_no_errors() {
        let envelope = json!({
            "definition": {"parts": [
                {"path": "notebook-content.py", "payload": b64("# hi"), "payloadType": "InlineBase64"},
                {"path": ".platform", "payload": b64(r#"{"metadata":{"type":"Notebook","displayName":"nb"}}"#), "payloadType": "InlineBase64"}
            ]}
        });
        let findings = validate_definition(Some("Notebook"), &envelope);
        assert!(
            findings.iter().all(|f| f.severity == Severity::Warning),
            "no errors expected: {findings:?}"
        );
        assert!(
            !findings.iter().any(|f| f.severity == Severity::Warning),
            "no warnings expected either: {findings:?}"
        );
    }

    #[test]
    fn missing_parts_array_is_error() {
        let findings = validate_definition(None, &json!({"foo": "bar"}));
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].code, "MISSING_PARTS");
        assert_eq!(findings[0].severity, Severity::Error);
    }

    #[test]
    fn empty_parts_is_error() {
        let findings = validate_definition(None, &json!({"definition": {"parts": []}}));
        assert!(findings.iter().any(|f| f.code == "EMPTY_PARTS"));
    }

    #[test]
    fn invalid_base64_is_error() {
        let envelope = json!({"parts": [
            {"path": "a.json", "payload": "not base64 !!!", "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(None, &envelope);
        assert!(findings.iter().any(|f| f.code == "INVALID_BASE64"));
    }

    #[test]
    fn invalid_json_part_is_error() {
        let envelope = json!({"parts": [
            {"path": "pipeline-content.json", "payload": b64("this is not json"), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(None, &envelope);
        assert!(findings.iter().any(|f| f.code == "INVALID_JSON_PART"));
    }

    #[test]
    fn bad_payload_type_enumerates_valid_value() {
        let envelope = json!({"parts": [
            {"path": "a.json", "payload": b64("{}"), "payloadType": "Base64"}
        ]});
        let findings = validate_definition(None, &envelope);
        let f = findings
            .iter()
            .find(|f| f.code == "INVALID_PAYLOAD_TYPE")
            .unwrap();
        assert!(f.message.contains("InlineBase64"));
    }

    #[test]
    fn duplicate_part_is_error() {
        let envelope = json!({"parts": [
            {"path": "a.json", "payload": b64("{}"), "payloadType": "InlineBase64"},
            {"path": "a.json", "payload": b64("{}"), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(None, &envelope);
        assert!(findings.iter().any(|f| f.code == "DUPLICATE_PART"));
    }

    #[test]
    fn missing_canonical_part_is_warning_not_error() {
        // A DataPipeline whose only part uses the wrong filename: envelope is
        // structurally valid (warning only), because Fabric tolerates aliases.
        let envelope = json!({"parts": [
            {"path": "wrong-name.json", "payload": b64("{\"properties\":{\"activities\":[]}}"), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(Some("DataPipeline"), &envelope);
        let f = findings
            .iter()
            .find(|f| f.code == "MISSING_CANONICAL_PART")
            .unwrap();
        assert_eq!(f.severity, Severity::Warning);
        assert!(f.message.contains("pipeline-content.json"));
        assert!(!findings.iter().any(|f| f.severity == Severity::Error));
    }

    #[test]
    fn copyjob_alias_satisfies_canonical() {
        // CopyJobV1.json is an accepted alias for copyjob-content.json.
        let envelope = json!({"parts": [
            {"path": "CopyJobV1.json", "payload": b64("{\"properties\":{\"jobMode\":\"Batch\"},\"activities\":[]}"), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(Some("CopyJob"), &envelope);
        assert!(!findings.iter().any(|f| f.code == "MISSING_CANONICAL_PART"));
    }

    #[test]
    fn unknown_type_is_warning() {
        let envelope = json!({"parts": [
            {"path": "a.json", "payload": b64("{}"), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(Some("Bogus"), &envelope);
        assert!(findings.iter().any(|f| f.code == "UNKNOWN_ITEM_TYPE"));
    }

    #[test]
    fn platform_missing_metadata_is_warning() {
        let envelope = json!({"parts": [
            {"path": ".platform", "payload": b64(r#"{"config":{}}"#), "payloadType": "InlineBase64"}
        ]});
        let findings = validate_definition(None, &envelope);
        assert!(
            findings
                .iter()
                .any(|f| f.code == "PLATFORM_MISSING_METADATA")
        );
    }

    #[test]
    fn build_body_wraps_raw_single_part_under_canonical_path() {
        let body = build_update_definition_body(
            r#"{"properties":{"jobMode":"Batch"},"activities":[]}"#,
            "copyjob-content.json",
        );
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "copyjob-content.json");
        assert_eq!(parts[0]["payloadType"], "InlineBase64");
        // The raw content must be base64-encoded, not passed through literally.
        let decoded = BASE64
            .decode(parts[0]["payload"].as_str().unwrap())
            .unwrap();
        assert!(String::from_utf8(decoded).unwrap().contains("jobMode"));
    }

    #[test]
    fn build_body_passes_through_full_envelope() {
        let raw = r#"{"definition":{"parts":[
            {"path":"queryMetadata.json","payload":"e30=","payloadType":"InlineBase64"},
            {"path":"mashup.pq","payload":"c2VjdGlvbg==","payloadType":"InlineBase64"}
        ]}}"#;
        let body = build_update_definition_body(raw, "dataflow.json");
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0]["path"], "queryMetadata.json");
        assert_eq!(parts[1]["path"], "mashup.pq");
    }

    #[test]
    fn build_body_passes_through_bare_parts_array() {
        let raw = r#"{"parts":[{"path":"a.json","payload":"e30=","payloadType":"InlineBase64"}]}"#;
        let body = build_update_definition_body(raw, "fallback.json");
        let parts = body["definition"]["parts"].as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["path"], "a.json");
    }

    #[test]
    fn build_body_preserves_definition_format() {
        // A notebook ipynb envelope carries `definition.format` — it must NOT be
        // dropped, or the server misreads the payload as raw .py.
        let raw = r#"{"definition":{"format":"ipynb","parts":[
            {"path":"notebook-content.py","payload":"e30=","payloadType":"InlineBase64"}
        ]}}"#;
        let body = build_update_definition_body(raw, "notebook-content.py");
        assert_eq!(body["definition"]["format"], "ipynb");
        assert_eq!(body["definition"]["parts"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn build_body_preserves_top_level_format() {
        let raw = r#"{"format":"ipynb","parts":[
            {"path":"notebook-content.py","payload":"e30=","payloadType":"InlineBase64"}
        ]}"#;
        let body = build_update_definition_body(raw, "notebook-content.py");
        assert_eq!(body["definition"]["format"], "ipynb");
    }

    #[test]
    fn build_body_omits_format_when_absent() {
        let raw = r#"{"definition":{"parts":[{"path":"a.json","payload":"e30=","payloadType":"InlineBase64"}]}}"#;
        let body = build_update_definition_body(raw, "fallback.json");
        assert!(body["definition"].get("format").is_none());
    }
}
