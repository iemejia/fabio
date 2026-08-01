//! Power BI Project (PBIP) / PBIR report-definition helpers.
//!
//! Implements offline validation of a report's on-disk definition (the format
//! Power BI Desktop and Fabric Git Integration produce, documented at
//! <https://learn.microsoft.com/power-bi/developer/projects/projects-report>) and
//! recursive gathering of a report `definition/` folder into Fabric definition
//! parts. This lets a coding agent generate PBIR files, validate them offline,
//! and create/deploy them with fabio.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Serialize;
use serde_json::Value;

/// A single validation finding (error or warning) tied to a file.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(super) struct Finding {
    /// File the finding relates to (relative to the report folder), or "" for folder-level.
    pub file: String,
    /// Stable machine-readable code (e.g. `MISSING_PBIR`, `INVALID_JSON`).
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
}

impl Finding {
    fn new(file: &str, code: &str, message: impl Into<String>) -> Self {
        Self {
            file: file.to_owned(),
            code: code.to_owned(),
            message: message.into(),
        }
    }
}

/// Structured result of validating one report definition.
#[derive(Debug, Clone, Serialize)]
pub(super) struct ReportValidation {
    /// The report folder that was validated.
    pub source: String,
    /// True when there are no blocking errors.
    pub valid: bool,
    /// `PBIR` (enhanced `definition/` folder) or `PBIR-Legacy` (`report.json`), if determinable.
    pub format: Option<String>,
    /// `byPath` or `byConnection`, if a datasetReference is present.
    pub dataset_reference: Option<String>,
    /// Number of structural checks performed.
    pub checks: usize,
    pub errors: Vec<Finding>,
    pub warnings: Vec<Finding>,
}

/// Read + parse a JSON file. Returns None and pushes an error on failure.
fn parse_json(path: &Path, label: &str, errors: &mut Vec<Finding>) -> Option<Value> {
    match std::fs::read_to_string(path) {
        Ok(s) => match serde_json::from_str::<Value>(&s) {
            Ok(v) => Some(v),
            Err(e) => {
                errors.push(Finding::new(
                    label,
                    "INVALID_JSON",
                    format!("invalid JSON: {e}"),
                ));
                None
            }
        },
        Err(e) => {
            errors.push(Finding::new(
                label,
                "UNREADABLE",
                format!("cannot read file: {e}"),
            ));
            None
        }
    }
}

/// Check a PBIR JSON file parses and carries a `$schema` (MS requires it; VS Code
/// uses it for validation/IntelliSense). Missing `$schema` is a warning.
fn check_pbir_json(
    path: &Path,
    label: &str,
    checks: &mut usize,
    errors: &mut Vec<Finding>,
    warnings: &mut Vec<Finding>,
) {
    *checks += 1;
    if let Some(v) = parse_json(path, label, errors)
        && v.get("$schema").and_then(Value::as_str).is_none()
    {
        warnings.push(Finding::new(
            label,
            "MISSING_SCHEMA",
            "PBIR file has no $schema (Microsoft's schema marks it required; add the developer.microsoft.com schema URL)",
        ));
    }
}

/// Validate the enhanced-format `definition/` folder of a report.
#[allow(clippy::similar_names, clippy::too_many_lines)]
fn validate_pbir_definition(
    def: &Path,
    checks: &mut usize,
    errors: &mut Vec<Finding>,
    warnings: &mut Vec<Finding>,
) {
    // Required top-level PBIR files.
    for req in ["report.json", "version.json"] {
        *checks += 1;
        let p = def.join(req);
        let label = format!("definition/{req}");
        if p.exists() {
            check_pbir_json(&p, &label, checks, errors, warnings);
        } else {
            errors.push(Finding::new(
                &label,
                "MISSING_REQUIRED",
                format!("required PBIR file `definition/{req}` is missing"),
            ));
        }
    }

    // pages/ folder with at least one page.json.
    *checks += 1;
    let pages = def.join("pages");
    if !pages.is_dir() {
        errors.push(Finding::new(
            "definition/pages",
            "MISSING_REQUIRED",
            "required PBIR `definition/pages/` folder is missing",
        ));
        return;
    }
    // pages.json is optional (page order / active page) but recommended.
    let pages_json = pages.join("pages.json");
    if pages_json.exists() {
        check_pbir_json(
            &pages_json,
            "definition/pages/pages.json",
            checks,
            errors,
            warnings,
        );
    } else {
        warnings.push(Finding::new(
            "definition/pages/pages.json",
            "MISSING_RECOMMENDED",
            "pages.json is missing (defines page order and active page)",
        ));
    }

    let mut page_count = 0usize;
    if let Ok(entries) = std::fs::read_dir(&pages) {
        for entry in entries.flatten() {
            let p = entry.path();
            if !p.is_dir() {
                continue;
            }
            page_count += 1;
            let page_name = p.file_name().unwrap_or_default().to_string_lossy();
            let page_json = p.join("page.json");
            *checks += 1;
            let label = format!("definition/pages/{page_name}/page.json");
            if page_json.exists() {
                check_pbir_json(&page_json, &label, checks, errors, warnings);
            } else {
                errors.push(Finding::new(
                    &label,
                    "MISSING_REQUIRED",
                    format!("page `{page_name}` is missing its required page.json"),
                ));
            }

            // visuals/<visual>/visual.json (each visual.json is required if the folder exists).
            let visuals = p.join("visuals");
            if visuals.is_dir()
                && let Ok(ventries) = std::fs::read_dir(&visuals)
            {
                for v in ventries.flatten() {
                    let vp = v.path();
                    if !vp.is_dir() {
                        continue;
                    }
                    let vname = vp.file_name().unwrap_or_default().to_string_lossy();
                    let vjson = vp.join("visual.json");
                    *checks += 1;
                    let vlabel =
                        format!("definition/pages/{page_name}/visuals/{vname}/visual.json");
                    if vjson.exists() {
                        check_pbir_json(&vjson, &vlabel, checks, errors, warnings);
                    } else {
                        errors.push(Finding::new(
                            &vlabel,
                            "MISSING_REQUIRED",
                            format!("visual `{vname}` is missing its required visual.json"),
                        ));
                    }
                }
            }
        }
    }
    if page_count == 0 {
        errors.push(Finding::new(
            "definition/pages",
            "NO_PAGES",
            "a PBIR report must contain at least one page folder",
        ));
    }

    // Optional: report-level measures.
    let ext = def.join("reportExtensions.json");
    if ext.exists() {
        check_pbir_json(
            &ext,
            "definition/reportExtensions.json",
            checks,
            errors,
            warnings,
        );
    }
}

/// Validate a single report folder (a `.Report` folder, or any folder containing
/// `definition.pbir`). Pure and offline.
#[allow(clippy::too_many_lines)]
pub(super) fn validate_report_folder(dir: &Path) -> ReportValidation {
    let mut errors: Vec<Finding> = Vec::new();
    let mut warnings: Vec<Finding> = Vec::new();
    let mut checks = 0usize;
    let mut format: Option<String> = None;
    let mut dataset_reference: Option<String> = None;

    // 1. definition.pbir is REQUIRED.
    checks += 1;
    let pbir_path = dir.join("definition.pbir");
    if !pbir_path.exists() {
        errors.push(Finding::new(
            "definition.pbir",
            "MISSING_PBIR",
            "required `definition.pbir` not found (is this a report folder?)",
        ));
        return ReportValidation {
            source: dir.to_string_lossy().into_owned(),
            valid: false,
            format,
            dataset_reference,
            checks,
            errors,
            warnings,
        };
    }

    let Some(pbir) = parse_json(&pbir_path, "definition.pbir", &mut errors) else {
        return ReportValidation {
            source: dir.to_string_lossy().into_owned(),
            valid: false,
            format,
            dataset_reference,
            checks,
            errors,
            warnings,
        };
    };

    // 2. $schema (warning — Fabric is lenient, but MS marks it required).
    checks += 1;
    match pbir.get("$schema").and_then(Value::as_str) {
        Some(s) if s.contains("report/definitionProperties") => {}
        Some(_) => warnings.push(Finding::new(
            "definition.pbir",
            "UNEXPECTED_SCHEMA",
            "$schema is not a report/definitionProperties URL",
        )),
        None => warnings.push(Finding::new(
            "definition.pbir",
            "MISSING_SCHEMA",
            "definition.pbir has no $schema (Microsoft marks it required)",
        )),
    }

    // 3. version.
    checks += 1;
    let version = pbir
        .get("version")
        .and_then(Value::as_str)
        .map(str::to_owned);
    if version.is_none() {
        errors.push(Finding::new(
            "definition.pbir",
            "MISSING_VERSION",
            "definition.pbir must have a `version`",
        ));
    }

    // 4. datasetReference: exactly one of byPath / byConnection.
    checks += 1;
    match pbir.get("datasetReference") {
        None => errors.push(Finding::new(
            "definition.pbir",
            "MISSING_DATASET_REFERENCE",
            "definition.pbir must have a `datasetReference`",
        )),
        Some(dr) => {
            let has_bypath = dr.get("byPath").is_some();
            let has_byconn = dr.get("byConnection").is_some();
            match (has_bypath, has_byconn) {
                (true, true) => errors.push(Finding::new(
                    "definition.pbir",
                    "AMBIGUOUS_DATASET_REFERENCE",
                    "datasetReference must have exactly one of byPath or byConnection, not both",
                )),
                (false, false) => errors.push(Finding::new(
                    "definition.pbir",
                    "MISSING_DATASET_REFERENCE",
                    "datasetReference must have one of byPath or byConnection",
                )),
                (true, false) => {
                    dataset_reference = Some("byPath".to_owned());
                    match dr
                        .get("byPath")
                        .and_then(|b| b.get("path"))
                        .and_then(Value::as_str)
                    {
                        Some(rel) => {
                            let target = dir.join(rel);
                            if !target.exists() {
                                warnings.push(Finding::new(
                                    "definition.pbir",
                                    "BYPATH_TARGET_NOT_FOUND",
                                    format!("byPath target `{rel}` does not exist relative to the report folder"),
                                ));
                            }
                            warnings.push(Finding::new(
                                "definition.pbir",
                                "BYPATH_NEEDS_BYCONNECTION",
                                "byPath requires a co-deployed semantic model; `fabio deploy` rewrites it to byConnection automatically, but `report create` needs byConnection — pass --dataset to bind by id",
                            ));
                        }
                        None => errors.push(Finding::new(
                            "definition.pbir",
                            "MISSING_BYPATH_PATH",
                            "byPath must have a `path`",
                        )),
                    }
                }
                (false, true) => {
                    dataset_reference = Some("byConnection".to_owned());
                    let bc = &dr["byConnection"];
                    if bc.get("connectionString").and_then(Value::as_str).is_none()
                        && bc.get("pbiModelDatabaseName").is_none()
                    {
                        warnings.push(Finding::new(
                            "definition.pbir",
                            "EMPTY_BYCONNECTION",
                            "byConnection has no connectionString / pbiModelDatabaseName",
                        ));
                    }
                }
            }
        }
    }

    // 5. Format detection: PBIR (definition/ folder) vs PBIR-Legacy (report.json).
    let def_folder = dir.join("definition");
    let report_json = dir.join("report.json");
    if def_folder.is_dir() {
        format = Some("PBIR".to_owned());
        if version.as_deref() == Some("1.0") {
            errors.push(Finding::new(
                "definition.pbir",
                "VERSION_FORMAT_MISMATCH",
                "a PBIR (enhanced `definition/` folder) report requires version 4.0 or higher, not 1.0",
            ));
        }
        validate_pbir_definition(&def_folder, &mut checks, &mut errors, &mut warnings);
    } else if report_json.exists() {
        format = Some("PBIR-Legacy".to_owned());
        checks += 1;
        parse_json(&report_json, "report.json", &mut errors);
    } else {
        errors.push(Finding::new(
            "",
            "NO_REPORT_BODY",
            "report has neither a `definition/` folder (PBIR) nor a `report.json` (PBIR-Legacy)",
        ));
    }

    ReportValidation {
        source: dir.to_string_lossy().into_owned(),
        valid: errors.is_empty(),
        format,
        dataset_reference,
        checks,
        errors,
        warnings,
    }
}

/// Resolve `--source` to one or more report folders and validate each.
///
/// Accepts: a report folder (containing `definition.pbir`), a `definition.pbir`
/// file, or a PBIP root (containing one or more `*.Report` subfolders).
pub(super) fn validate(source: &Path) -> Result<Vec<ReportValidation>> {
    if !source.exists() {
        bail!("source path does not exist: {}", source.display());
    }

    // A definition.pbir file → validate its parent folder.
    if source.is_file() {
        if source.file_name().and_then(|n| n.to_str()) == Some("definition.pbir") {
            let parent = source.parent().unwrap_or_else(|| Path::new("."));
            return Ok(vec![validate_report_folder(parent)]);
        }
        bail!(
            "source file must be a definition.pbir (got {})",
            source.display()
        );
    }

    // A folder directly containing definition.pbir → single report.
    if source.join("definition.pbir").exists() {
        return Ok(vec![validate_report_folder(source)]);
    }

    // A PBIP root → find *.Report subfolders that contain a definition.pbir.
    let mut reports: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(source) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() && p.join("definition.pbir").exists() {
                reports.push(p);
            }
        }
    }
    reports.sort();
    if reports.is_empty() {
        bail!(
            "no report definition found under {} (expected a definition.pbir, a report folder, or a PBIP root with *.Report subfolders)",
            source.display()
        );
    }
    Ok(reports.iter().map(|p| validate_report_folder(p)).collect())
}

/// Gather every file under a report folder into Fabric definition parts, with
/// paths relative to the folder root and normalized to forward slashes.
///
/// Excludes `.platform` (create takes displayName from --name), `.pbi/` (local
/// user state), `.children/`, and the deploy sidecar metadata files.
pub(super) fn gather_report_parts(dir: &Path) -> Result<Vec<Value>> {
    let mut parts: Vec<Value> = Vec::new();
    gather_recursive(dir, dir, &mut parts)?;
    if parts.is_empty() {
        bail!("no report definition files found in {}", dir.display());
    }
    Ok(parts)
}

fn gather_recursive(base: &Path, current: &Path, parts: &mut Vec<Value>) -> Result<()> {
    for entry in std::fs::read_dir(current)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        if path.is_dir() {
            if name == ".pbi" || name == ".children" {
                continue;
            }
            gather_recursive(base, &path, parts)?;
        } else {
            if name == ".platform"
                || name == "creationPayload.json"
                || name == "shortcuts.metadata.json"
                || name == "governance.metadata.json"
                || name == "schedules.metadata.json"
            {
                continue;
            }
            let rel = path
                .strip_prefix(base)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let content = std::fs::read(&path)?;
            parts.push(serde_json::json!({
                "path": rel,
                "payload": BASE64.encode(&content),
                "payloadType": "InlineBase64"
            }));
        }
    }
    Ok(())
}

/// Rewrite a gathered `definition.pbir` part's `datasetReference` to bind the
/// report by connection to `dataset_id` (used by `report create --definition
/// --dataset` so a generated report can be bound to a concrete model at create
/// time regardless of its on-disk byPath/byConnection reference).
pub(super) fn rebind_pbir_part(parts: &mut [Value], dataset_id: &str) -> Result<()> {
    for part in parts.iter_mut() {
        if part.get("path").and_then(Value::as_str) == Some("definition.pbir") {
            let raw = BASE64
                .decode(part["payload"].as_str().unwrap_or_default())
                .map_err(|e| anyhow::anyhow!("definition.pbir payload not valid base64: {e}"))?;
            let mut pbir: Value = serde_json::from_slice(&raw)
                .map_err(|e| anyhow::anyhow!("definition.pbir is not valid JSON: {e}"))?;
            pbir["datasetReference"] = serde_json::json!({
                "byConnection": {
                    "connectionString": null,
                    "pbiServiceModelId": null,
                    "pbiModelVirtualServerName": "sobe_wowvirtualserver",
                    "pbiModelDatabaseName": dataset_id,
                    "name": "EntityDataSource",
                    "connectionType": "pbiServiceXmlaStyleLive"
                }
            });
            part["payload"] = Value::from(BASE64.encode(pbir.to_string().as_bytes()));
            return Ok(());
        }
    }
    bail!("definition.pbir not found in the report folder — cannot apply --dataset binding");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write(path: &Path, content: &str) {
        if let Some(p) = path.parent() {
            fs::create_dir_all(p).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    const PBIR_BYCONN: &str = r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json","version":"4.0","datasetReference":{"byConnection":{"connectionString":"semanticmodelid=abc"}}}"#;

    fn make_pbir_report(dir: &Path) {
        write(&dir.join("definition.pbir"), PBIR_BYCONN);
        write(
            &dir.join("definition/report.json"),
            r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definition/report/1.0.0/schema.json"}"#,
        );
        write(
            &dir.join("definition/version.json"),
            r#"{"$schema":"x","version":"4.0"}"#,
        );
        write(
            &dir.join("definition/pages/pages.json"),
            r#"{"$schema":"x","pageOrder":["p1"]}"#,
        );
        write(
            &dir.join("definition/pages/p1/page.json"),
            r#"{"$schema":"x","name":"p1"}"#,
        );
        write(
            &dir.join("definition/pages/p1/visuals/v1/visual.json"),
            r#"{"$schema":"x","name":"v1"}"#,
        );
    }

    #[test]
    fn valid_pbir_report_passes() {
        let dir = TempDir::new().unwrap();
        make_pbir_report(dir.path());
        let r = validate_report_folder(dir.path());
        assert!(r.valid, "expected valid, errors: {:?}", r.errors);
        assert_eq!(r.format.as_deref(), Some("PBIR"));
        assert_eq!(r.dataset_reference.as_deref(), Some("byConnection"));
    }

    #[test]
    fn missing_pbir_is_error() {
        let dir = TempDir::new().unwrap();
        let r = validate_report_folder(dir.path());
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.code == "MISSING_PBIR"));
    }

    #[test]
    fn pbir_missing_version_json_is_error() {
        let dir = TempDir::new().unwrap();
        make_pbir_report(dir.path());
        fs::remove_file(dir.path().join("definition/version.json")).unwrap();
        let r = validate_report_folder(dir.path());
        assert!(!r.valid);
        assert!(
            r.errors
                .iter()
                .any(|e| e.code == "MISSING_REQUIRED" && e.file.ends_with("version.json"))
        );
    }

    #[test]
    fn pbir_with_no_pages_is_error() {
        let dir = TempDir::new().unwrap();
        make_pbir_report(dir.path());
        fs::remove_dir_all(dir.path().join("definition/pages")).unwrap();
        let r = validate_report_folder(dir.path());
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.code == "MISSING_REQUIRED"));
    }

    #[test]
    fn legacy_report_json_is_detected() {
        let dir = TempDir::new().unwrap();
        write(&dir.path().join("definition.pbir"), PBIR_BYCONN);
        write(&dir.path().join("report.json"), r#"{"sections":[]}"#);
        let r = validate_report_folder(dir.path());
        assert!(r.valid, "errors: {:?}", r.errors);
        assert_eq!(r.format.as_deref(), Some("PBIR-Legacy"));
    }

    #[test]
    fn bypath_produces_warning_not_error() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join("definition.pbir"),
            r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/2.0.0/schema.json","version":"4.0","datasetReference":{"byPath":{"path":"../Sales.SemanticModel"}}}"#,
        );
        write(&dir.path().join("report.json"), r#"{"sections":[]}"#);
        let r = validate_report_folder(dir.path());
        assert!(r.valid, "byPath should not be a hard error: {:?}", r.errors);
        assert_eq!(r.dataset_reference.as_deref(), Some("byPath"));
        assert!(
            r.warnings
                .iter()
                .any(|w| w.code == "BYPATH_NEEDS_BYCONNECTION")
        );
    }

    #[test]
    fn version1_with_definition_folder_is_mismatch() {
        let dir = TempDir::new().unwrap();
        make_pbir_report(dir.path());
        write(
            &dir.path().join("definition.pbir"),
            r#"{"$schema":"https://developer.microsoft.com/json-schemas/fabric/item/report/definitionProperties/1.0.0/schema.json","version":"1.0","datasetReference":{"byConnection":{"connectionString":"semanticmodelid=abc"}}}"#,
        );
        let r = validate_report_folder(dir.path());
        assert!(!r.valid);
        assert!(r.errors.iter().any(|e| e.code == "VERSION_FORMAT_MISMATCH"));
    }

    #[test]
    fn gather_parts_excludes_platform_and_pbi() {
        let dir = TempDir::new().unwrap();
        make_pbir_report(dir.path());
        write(&dir.path().join(".platform"), r#"{"metadata":{}}"#);
        write(&dir.path().join(".pbi/localSettings.json"), "{}");
        let parts = gather_report_parts(dir.path()).unwrap();
        let paths: Vec<&str> = parts.iter().map(|p| p["path"].as_str().unwrap()).collect();
        assert!(paths.contains(&"definition.pbir"));
        assert!(paths.contains(&"definition/pages/p1/visuals/v1/visual.json"));
        assert!(!paths.contains(&".platform"));
        assert!(!paths.iter().any(|p| p.contains(".pbi/")));
    }

    #[test]
    fn rebind_pbir_sets_byconnection_dataset() {
        let dir = TempDir::new().unwrap();
        write(
            &dir.path().join("definition.pbir"),
            r#"{"$schema":"x","version":"4.0","datasetReference":{"byPath":{"path":"../M.SemanticModel"}}}"#,
        );
        write(&dir.path().join("report.json"), "{}");
        let mut parts = gather_report_parts(dir.path()).unwrap();
        rebind_pbir_part(&mut parts, "model-123").unwrap();
        let pbir_part = parts
            .iter()
            .find(|p| p["path"] == "definition.pbir")
            .unwrap();
        let raw = BASE64
            .decode(pbir_part["payload"].as_str().unwrap())
            .unwrap();
        let pbir: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            pbir["datasetReference"]["byConnection"]["pbiModelDatabaseName"],
            "model-123"
        );
        assert!(pbir["datasetReference"].get("byPath").is_none());
    }
}
