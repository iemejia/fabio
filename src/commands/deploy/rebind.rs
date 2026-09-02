//! Offline environment-rebinding for local `.platform` definition files.
//!
//! `deploy rebind` rewrites environment-specific IDs (workspace/lakehouse GUIDs,
//! connection IDs, etc.) directly in the on-disk definition files, in place. It is
//! the offline counterpart to `deploy apply`'s parameter substitution.
//!
//! ## Why this exists
//!
//! Fabric's **Branch out** feature creates a feature workspace that is *Git-synced*
//! to a feature branch. `deploy apply` (which pushes item definitions via the Fabric
//! REST API) MUST NOT target a Git-synced workspace — it causes workspace drift that
//! Git sync later overwrites or conflicts with. So the environment-specific IDs that a
//! branched-out repo still carries (the dev workspace's Semantic Model Direct Lake URL,
//! Notebook `default_lakehouse` metadata, Variable Library value-set GUIDs) must be
//! rewritten in the **local files**, committed, and synced via *Update from Git* — not
//! pushed via the API. `deploy rebind` does exactly that rewrite, reusing the same
//! parameter-file format as `deploy apply`.
//!
//! ## Symmetry
//!
//! `rebind --from-env dev --to-env feature-x` swaps dev IDs → feature IDs. Before
//! opening a PR back to dev, run the reverse: `rebind --from-env feature-x --to-env dev`.
//! `deploy validate --pr-ready --expect-env dev` verifies the revert was complete.
//!
//! Only **literal** per-environment values (and `$ENV:VAR` expansions) are rewritten.
//! Deploy-time dynamic variables (`$workspace.id`, `$items.Type.Name.id`) cannot be
//! resolved offline and are skipped with a warning — they are resolved by `deploy apply`
//! against the live target workspace instead.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use regex::Regex;
use serde_json::json;

use super::params::{FindReplaceRule, parse_parameters, replace_capture_group};
use crate::cli::Cli;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

/// A discovered Fabric item directory (a folder containing a `.platform` file).
struct ItemDir {
    /// Absolute/relative path to the item directory.
    dir: PathBuf,
    /// `metadata.type` from `.platform`.
    item_type: String,
    /// `metadata.displayName` from `.platform`.
    display_name: String,
}

/// The result of resolving an environment-keyed replacement value for offline use.
enum ResolvedValue {
    /// A concrete literal string (possibly expanded from `$ENV:VAR`).
    Literal(String),
    /// A deploy-time dynamic variable that cannot be resolved offline.
    Dynamic,
    /// No value for the requested environment (and no `_ALL_` fallback).
    Missing,
}

/// Resolve an environment-keyed replacement value to a literal for offline rewriting.
///
/// Looks up the value for `env` (case-insensitive), falling back to `_ALL_`. Expands
/// `$ENV:VAR`. Flags `$workspace`/`$items` dynamic variables as [`ResolvedValue::Dynamic`].
fn resolve_env_literal(
    replace_value: &std::collections::HashMap<String, String>,
    env: &str,
) -> ResolvedValue {
    let raw = replace_value
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(env))
        .map(|(_, v)| v)
        .or_else(|| {
            replace_value
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("_ALL_"))
                .map(|(_, v)| v)
        });

    let Some(raw) = raw else {
        return ResolvedValue::Missing;
    };

    if let Some(var_name) = raw.strip_prefix("$ENV:") {
        return std::env::var(var_name).map_or(ResolvedValue::Dynamic, ResolvedValue::Literal);
    }

    if raw.starts_with("$workspace") || raw.starts_with("$items") {
        return ResolvedValue::Dynamic;
    }

    ResolvedValue::Literal(raw.clone())
}

/// Does a `find_replace` rule apply to the given item (by type/name scope)?
fn rule_matches_item(rule: &FindReplaceRule, item_type: &str, display_name: &str) -> bool {
    if let Some(types) = rule.item_type.as_ref()
        && !types.contains(item_type)
    {
        return false;
    }
    if let Some(names) = rule.item_name.as_ref()
        && !names.contains(display_name)
    {
        return false;
    }
    true
}

/// Does a `find_replace` rule apply to the given file (by `file_path` scope)?
///
/// `rel_path` is the path relative to the item directory, using forward slashes.
fn rule_matches_file(rule: &FindReplaceRule, rel_path: &str) -> bool {
    rule.file_path
        .as_ref()
        .is_none_or(|paths| paths.contains(rel_path))
}

/// Recursively discover item directories (folders containing a `.platform` file).
fn discover_item_dirs(source: &Path) -> Result<Vec<ItemDir>> {
    let mut items = Vec::new();
    discover_item_dirs_inner(source, &mut items)?;
    items.sort_by(|a, b| a.dir.cmp(&b.dir));
    Ok(items)
}

fn discover_item_dirs_inner(dir: &Path, items: &mut Vec<ItemDir>) -> Result<()> {
    let platform = dir.join(".platform");
    if platform.is_file() {
        let (item_type, display_name) = read_platform_identity(&platform)?;
        items.push(ItemDir {
            dir: dir.to_path_buf(),
            item_type,
            display_name,
        });
        // Items do not nest — do not recurse further for more roots.
        return Ok(());
    }

    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            discover_item_dirs_inner(&path, items)?;
        }
    }
    Ok(())
}

/// Read `metadata.type` and `metadata.displayName` from a `.platform` file.
fn read_platform_identity(platform: &Path) -> Result<(String, String)> {
    let content = std::fs::read_to_string(platform)
        .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", platform.display()))?;
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| anyhow::anyhow!("Invalid JSON in {}: {e}", platform.display()))?;
    let metadata = parsed
        .get("metadata")
        .ok_or_else(|| anyhow::anyhow!("Missing 'metadata' in {}", platform.display()))?;
    let item_type = metadata
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_owned();
    let display_name = metadata
        .get("displayName")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown")
        .to_owned();
    Ok((item_type, display_name))
}

/// Recursively collect all files under an item directory as `(abs_path, rel_path)`
/// pairs, where `rel_path` is relative to the item directory using forward slashes.
fn collect_item_files(item_dir: &Path) -> Result<Vec<(PathBuf, String)>> {
    let mut files = Vec::new();
    collect_item_files_inner(item_dir, item_dir, &mut files)?;
    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}

fn collect_item_files_inner(
    root: &Path,
    dir: &Path,
    files: &mut Vec<(PathBuf, String)>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| anyhow::anyhow!("Failed to read directory {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_item_files_inner(root, &path, files)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((path, rel));
        }
    }
    Ok(())
}

/// Apply a single (find, replace) rule to file text, returning the new text and the
/// number of replacements made.
fn apply_rule_to_text(
    rule: &FindReplaceRule,
    find: &str,
    replace: &str,
    text: &str,
) -> Result<(String, usize)> {
    if rule.is_regex {
        let re = Regex::new(find).map_err(|e| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                format!("Invalid regex in find_value '{find}': {e}"),
                "Fix the regex pattern in the parameter file, or set is_regex to false.",
            )
        })?;
        let count = re.captures_iter(text).count();
        let new_text = replace_capture_group(&re, text, replace);
        Ok((new_text, count))
    } else {
        let count = text.matches(find).count();
        Ok((text.replace(find, replace), count))
    }
}

/// A resolved literal find→replace pair for one rule under the requested env swap.
struct ResolvedRule<'a> {
    rule: &'a FindReplaceRule,
    find: String,
    replace: String,
}

/// Compute the literal find→replace pairs for a from→to env swap, collecting warnings
/// for rules that cannot be applied offline.
fn resolve_rules<'a>(
    rules: &'a [FindReplaceRule],
    from_env: &str,
    to_env: &str,
    warnings: &mut Vec<String>,
) -> Vec<ResolvedRule<'a>> {
    let mut resolved = Vec::new();
    for (i, rule) in rules.iter().enumerate() {
        let from = resolve_env_literal(&rule.replace_value, from_env);
        let to = resolve_env_literal(&rule.replace_value, to_env);
        match (from, to) {
            (ResolvedValue::Literal(find), ResolvedValue::Literal(replace)) => {
                if find == replace {
                    continue; // no-op
                }
                resolved.push(ResolvedRule {
                    rule,
                    find,
                    replace,
                });
            }
            (ResolvedValue::Dynamic, _) | (_, ResolvedValue::Dynamic) => {
                warnings.push(format!(
                    "find_replace rule #{} skipped: value for '{from_env}' or '{to_env}' is a deploy-time dynamic variable ($workspace/$items) that cannot be resolved offline. It is resolved by 'deploy apply' against the live workspace.",
                    i + 1
                ));
            }
            (ResolvedValue::Missing, _) | (_, ResolvedValue::Missing) => {
                warnings.push(format!(
                    "find_replace rule #{} skipped: no value for env '{from_env}' or '{to_env}' (and no _ALL_ fallback).",
                    i + 1
                ));
            }
        }
    }
    resolved
}

/// Execute `deploy rebind`.
pub(super) fn execute_rebind(
    cli: &Cli,
    source: &Path,
    parameters: &Path,
    from_env: &str,
    to_env: &str,
) -> Result<()> {
    if from_env.eq_ignore_ascii_case(to_env) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("--from-env and --to-env are the same ('{from_env}')"),
            "Provide two different environments, e.g. --from-env dev --to-env feature-x.",
        )
        .into());
    }

    if !source.exists() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Source directory does not exist: {}", source.display()),
            "Point --source at the local repo directory containing .platform item folders.",
        )
        .into());
    }

    let params = parse_parameters(parameters)?;
    let mut warnings: Vec<String> = Vec::new();
    let resolved = resolve_rules(&params.find_replace, from_env, to_env, &mut warnings);

    let items = discover_item_dirs(source)?;
    if items.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("No items (.platform files) found under: {}", source.display()),
            "Ensure --source points at a Fabric Git-integration directory with <Name>.<Type>/ item folders.",
        )
        .into());
    }

    // (rel_file_display, occurrences) accumulator, sorted for stable output.
    let mut file_changes: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_replacements = 0_usize;

    for item in &items {
        let files = collect_item_files(&item.dir)?;
        for (abs_path, rel_path) in &files {
            // Applicable rules for this item/file.
            let applicable: Vec<&ResolvedRule<'_>> = resolved
                .iter()
                .filter(|r| {
                    rule_matches_item(r.rule, &item.item_type, &item.display_name)
                        && rule_matches_file(r.rule, rel_path)
                })
                .collect();
            if applicable.is_empty() {
                continue;
            }

            let Ok(original) = std::fs::read_to_string(abs_path) else {
                continue; // binary / non-UTF-8 file — skip
            };

            let mut text = original.clone();
            let mut file_reps = 0_usize;
            for r in &applicable {
                let (new_text, count) = apply_rule_to_text(r.rule, &r.find, &r.replace, &text)?;
                if count > 0 {
                    text = new_text;
                    file_reps += count;
                }
            }

            if file_reps > 0 && text != original {
                let display = item.dir.join(rel_path).to_string_lossy().replace('\\', "/");
                file_changes.insert(display, file_reps);
                total_replacements += file_reps;
                if !cli.dry_run {
                    std::fs::write(abs_path, &text).map_err(|e| {
                        anyhow::anyhow!("Failed to write {}: {e}", abs_path.display())
                    })?;
                }
            }
        }
    }

    let changed_files: Vec<serde_json::Value> = file_changes
        .iter()
        .map(|(file, count)| json!({ "file": file, "replacements": count }))
        .collect();

    let status = if cli.dry_run { "dry_run" } else { "rebound" };
    let output_data = json!({
        "status": status,
        "dry_run": cli.dry_run,
        "source": source.display().to_string(),
        "from_env": from_env,
        "to_env": to_env,
        "files_changed": file_changes.len(),
        "replacements": total_replacements,
        "changes": changed_files,
        "rules_applied": resolved.len(),
        "warnings": warnings,
        "hint": if cli.dry_run {
            "Remove --dry-run to write the changes. After rebinding, commit and sync the feature workspace via Update from Git (do NOT use 'deploy apply' on a Git-synced workspace)."
        } else {
            "Commit and push the rewritten files, then sync the feature workspace via Update from Git. Before opening a PR back to the source env, run: deploy rebind --from-env <to> --to-env <from>, then deploy validate --pr-ready."
        },
    });

    output::render_object(cli, &output_data, "status");
    Ok(())
}

/// A UTF-8 definition file read for PR-readiness scanning.
struct FileEntry {
    item_type: String,
    display_name: String,
    rel_path: String,
    content: String,
}

/// Run environment-hygiene / PR-readiness checks on a source directory (offline).
///
/// Returns `(errors, warnings, checks)`. Called from `deploy validate --pr-ready`.
///
/// Checks:
/// 1. **Expected-env binding** — every rule's `expect_env` literal value appears in the
///    scoped files (the repo is bound to the expected environment).
/// 2. **Foreign-env absence** — no OTHER environment's literal value appears anywhere.
/// 3. **Stray value-set files** — no `valueSets/*.json` file exists whose stem is not in
///    the `allow_value_sets` allowlist.
#[allow(clippy::too_many_lines)]
pub(super) fn run_pr_ready_checks(
    source: &Path,
    parameters: Option<&Path>,
    expect_env: Option<&str>,
    allow_value_sets: &[String],
) -> Result<(Vec<String>, Vec<String>, Vec<serde_json::Value>)> {
    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut checks: Vec<serde_json::Value> = Vec::new();

    let expect = expect_env.ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--pr-ready requires --expect-env",
            "Specify the environment the repo should be bound to, e.g. --expect-env dev.",
        )
    })?;

    let items = discover_item_dirs(source)?;

    // Gather all definition file contents once (UTF-8 only).
    let mut file_entries: Vec<FileEntry> = Vec::new();
    for item in &items {
        for (abs_path, rel_path) in &collect_item_files(&item.dir)? {
            if let Ok(content) = std::fs::read_to_string(abs_path) {
                file_entries.push(FileEntry {
                    item_type: item.item_type.clone(),
                    display_name: item.display_name.clone(),
                    rel_path: rel_path.clone(),
                    content,
                });
            }
        }
    }

    // --- Env-binding checks (require a parameters file) ---
    if let Some(params_path) = parameters {
        let params = parse_parameters(params_path)?;

        // Collect every environment key present across all rules (excluding _ALL_).
        let mut all_envs: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for rule in &params.find_replace {
            for k in rule.replace_value.keys() {
                if !k.eq_ignore_ascii_case("_ALL_") {
                    all_envs.insert(k.clone());
                }
            }
        }

        // 1. Expected-env values must be present.
        let mut expected_present = 0_usize;
        let mut expected_missing = 0_usize;
        // 2. Foreign-env values must be absent.
        let foreign_envs: Vec<&String> = all_envs
            .iter()
            .filter(|e| !e.eq_ignore_ascii_case(expect))
            .collect();
        let mut foreign_found = 0_usize;

        for rule in &params.find_replace {
            // Expected value present?
            if let ResolvedValue::Literal(expected_val) =
                resolve_env_literal(&rule.replace_value, expect)
            {
                let present = file_entries.iter().any(|fe| {
                    rule_matches_item(rule, &fe.item_type, &fe.display_name)
                        && rule_matches_file(rule, &fe.rel_path)
                        && fe.content.contains(&expected_val)
                });
                if present {
                    expected_present += 1;
                } else {
                    // Only an error if the value is scoped to items that exist AND the
                    // foreign value is also absent (nothing to check) — treat as warning.
                    expected_missing += 1;
                    warnings.push(format!(
                        "expected env '{expect}' value '{expected_val}' not found in any matching definition (rule scope may not match any item)"
                    ));
                }
            }

            // Foreign values absent?
            for foreign in &foreign_envs {
                if let ResolvedValue::Literal(foreign_val) =
                    resolve_env_literal(&rule.replace_value, foreign)
                    && !foreign_val.is_empty()
                {
                    for fe in &file_entries {
                        if rule_matches_item(rule, &fe.item_type, &fe.display_name)
                            && rule_matches_file(rule, &fe.rel_path)
                            && fe.content.contains(&foreign_val)
                        {
                            foreign_found += 1;
                            errors.push(format!(
                                "foreign env '{foreign}' value '{foreign_val}' found in '{}' — run: deploy rebind --from-env {foreign} --to-env {expect}",
                                fe.rel_path
                            ));
                        }
                    }
                }
            }
        }

        checks.push(json!({
            "check": "env_binding",
            "expect_env": expect,
            "expected_values_present": expected_present,
            "expected_values_missing": expected_missing,
            "foreign_values_found": foreign_found,
        }));
    } else {
        warnings.push(
            "--pr-ready without --parameters: env-binding checks skipped (only stray value-set check runs)".to_owned(),
        );
    }

    // --- 3. Stray value-set file check ---
    let mut stray_value_sets: Vec<String> = Vec::new();
    for item in &items {
        for (_, rel_path) in &collect_item_files(&item.dir)? {
            // valueSets/<name>.json under a VariableLibrary item.
            let lower = rel_path.to_ascii_lowercase();
            let is_json = Path::new(rel_path)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("json"));
            if lower.starts_with("valuesets/") && is_json {
                let stem = Path::new(rel_path)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
                let allowed = allow_value_sets
                    .iter()
                    .any(|a| a.eq_ignore_ascii_case(&stem));
                if !allowed {
                    stray_value_sets.push(format!("{}/{}", item.display_name, rel_path));
                    errors.push(format!(
                        "stray variable-library value-set file '{rel_path}' (value set '{stem}') is not in the --allow-value-set allowlist — remove it before merging"
                    ));
                }
            }
        }
    }
    checks.push(json!({
        "check": "stray_value_sets",
        "allow_value_sets": allow_value_sets,
        "stray_found": stray_value_sets.len(),
        "stray_files": stray_value_sets,
    }));

    Ok((errors, warnings, checks))
}

/// Render the standalone PR-ready result and set a non-zero exit on failure.
pub(super) fn render_pr_ready(
    cli: &Cli,
    source: &Path,
    errors: &[String],
    warnings: &[String],
    checks: &[serde_json::Value],
) -> Result<()> {
    let ok = errors.is_empty();
    let output_data = json!({
        "status": if ok { "pr_ready" } else { "not_pr_ready" },
        "source": source.display().to_string(),
        "pr_ready": ok,
        "errors": errors,
        "warnings": warnings,
        "checks": checks,
        "summary": { "errors": errors.len(), "warnings": warnings.len() },
    });
    output::render_object(cli, &output_data, "status");
    if !ok {
        bail!("PR-readiness check failed with {} error(s)", errors.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::Path;

    use super::*;
    use crate::commands::deploy::params::FindReplaceRule;

    fn rule(find: &str, values: &[(&str, &str)]) -> FindReplaceRule {
        let map: HashMap<String, String> = values
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        FindReplaceRule {
            find_value: find.to_owned(),
            replace_value: map,
            is_regex: false,
            item_type: None,
            item_name: None,
            file_path: None,
        }
    }

    #[test]
    fn resolve_env_literal_exact_and_all_fallback() {
        let map: HashMap<String, String> = [("dev".to_owned(), "d".to_owned())].into();
        assert!(matches!(
            resolve_env_literal(&map, "dev"),
            ResolvedValue::Literal(v) if v == "d"
        ));
        assert!(matches!(
            resolve_env_literal(&map, "prod"),
            ResolvedValue::Missing
        ));

        let all: HashMap<String, String> = [("_ALL_".to_owned(), "x".to_owned())].into();
        assert!(matches!(
            resolve_env_literal(&all, "anything"),
            ResolvedValue::Literal(v) if v == "x"
        ));
    }

    #[test]
    fn resolve_env_literal_flags_deploy_time_dynamics() {
        let map: HashMap<String, String> =
            [("dev".to_owned(), "$items.Lakehouse.LH.id".to_owned())].into();
        assert!(matches!(
            resolve_env_literal(&map, "dev"),
            ResolvedValue::Dynamic
        ));
        let ws: HashMap<String, String> = [("dev".to_owned(), "$workspace.id".to_owned())].into();
        assert!(matches!(
            resolve_env_literal(&ws, "dev"),
            ResolvedValue::Dynamic
        ));
    }

    #[test]
    #[allow(unsafe_code)]
    fn resolve_env_literal_expands_env_var() {
        let var = "FABIO_TEST_REBIND_ENVVAR_XYZ";
        // SAFETY: single-threaded test-local unique var name.
        unsafe { std::env::set_var(var, "resolved-123") };
        let map: HashMap<String, String> = [("dev".to_owned(), format!("$ENV:{var}"))].into();
        assert!(matches!(
            resolve_env_literal(&map, "dev"),
            ResolvedValue::Literal(v) if v == "resolved-123"
        ));
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn rule_scope_matching() {
        let mut r = rule("x", &[("dev", "a")]);
        r.item_type = Some(crate::commands::deploy::params::StringOrVec::Single(
            "Notebook".to_owned(),
        ));
        assert!(rule_matches_item(&r, "Notebook", "Any"));
        assert!(!rule_matches_item(&r, "Lakehouse", "Any"));

        r.file_path = Some(crate::commands::deploy::params::StringOrVec::Single(
            "definition/expressions.tmdl".to_owned(),
        ));
        assert!(rule_matches_file(&r, "definition/expressions.tmdl"));
        assert!(!rule_matches_file(&r, "notebook-content.py"));
    }

    #[test]
    fn apply_rule_literal_and_regex() {
        let lit = rule("old", &[]);
        let (out, n) = apply_rule_to_text(&lit, "old", "new", "old old x").unwrap();
        assert_eq!(out, "new new x");
        assert_eq!(n, 2);

        let mut re = rule(r#"id="([0-9]+)""#, &[]);
        re.is_regex = true;
        let (out, n) = apply_rule_to_text(&re, r#"id="([0-9]+)""#, "999", r#"id="123""#).unwrap();
        assert_eq!(out, r#"id="999""#);
        assert_eq!(n, 1);
    }

    #[test]
    fn resolve_rules_skips_noop_dynamic_and_missing() {
        let mut warnings = Vec::new();
        let rules = vec![
            rule("a", &[("dev", "same"), ("feat", "same")]), // no-op (equal)
            rule("b", &[("dev", "d1"), ("feat", "f1")]),     // real swap
            rule("c", &[("dev", "$items.Lakehouse.LH.id"), ("feat", "f2")]), // dynamic
            rule("d", &[("dev", "d3")]),                     // missing feat
        ];
        let resolved = resolve_rules(&rules, "dev", "feat", &mut warnings);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].find, "d1");
        assert_eq!(resolved[0].replace, "f1");
        assert_eq!(warnings.len(), 2); // dynamic + missing
    }

    fn write_item(root: &Path, dir: &str, item_type: &str, name: &str, files: &[(&str, &str)]) {
        let item = root.join(dir);
        std::fs::create_dir_all(&item).unwrap();
        std::fs::write(
            item.join(".platform"),
            format!(r#"{{"metadata":{{"type":"{item_type}","displayName":"{name}"}}}}"#),
        )
        .unwrap();
        for (rel, content) in files {
            let path = item.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(path, content).unwrap();
        }
    }

    #[test]
    fn discover_items_and_collect_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_item(
            tmp.path(),
            "MySM.SemanticModel",
            "SemanticModel",
            "MySM",
            &[("definition/expressions.tmdl", "dev-lake")],
        );
        let items = discover_item_dirs(tmp.path()).unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_type, "SemanticModel");
        let files = collect_item_files(&items[0].dir).unwrap();
        let rels: Vec<&str> = files.iter().map(|(_, r)| r.as_str()).collect();
        assert!(rels.contains(&"definition/expressions.tmdl"));
        assert!(rels.contains(&".platform"));
    }

    #[test]
    fn pr_ready_detects_foreign_env_and_stray_value_set() {
        let tmp = tempfile::TempDir::new().unwrap();
        // Semantic model still bound to feature-x (foreign value present)
        write_item(
            tmp.path(),
            "MySM.SemanticModel",
            "SemanticModel",
            "MySM",
            &[("definition/expressions.tmdl", "feat-lake-9999")],
        );
        // Variable library with a stray feature value set
        write_item(
            tmp.path(),
            "MyVL.VariableLibrary",
            "VariableLibrary",
            "MyVL",
            &[
                ("valueSets/dev.json", "{}"),
                ("valueSets/feature-x.json", "{}"),
            ],
        );
        let params_path = tmp.path().join("params.json");
        std::fs::write(
            &params_path,
            r#"{"find_replace":[{"find_value":"placeholder","replace_value":{"dev":"dev-lake-1111","feature-x":"feat-lake-9999"}}]}"#,
        )
        .unwrap();

        let (errors, _warnings, checks) = run_pr_ready_checks(
            tmp.path(),
            Some(&params_path),
            Some("dev"),
            &["dev".to_owned()],
        )
        .unwrap();

        // Foreign value found + stray value set = at least 2 errors
        assert!(
            errors.iter().any(|e| e.contains("foreign env 'feature-x'")),
            "expected foreign-env error, got {errors:?}"
        );
        assert!(
            errors.iter().any(|e| e.contains("stray")),
            "expected stray value-set error, got {errors:?}"
        );
        let stray = checks
            .iter()
            .find(|c| c["check"] == "stray_value_sets")
            .unwrap();
        assert_eq!(stray["stray_found"], 1);
    }

    #[test]
    fn pr_ready_passes_when_clean() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_item(
            tmp.path(),
            "MySM.SemanticModel",
            "SemanticModel",
            "MySM",
            &[("definition/expressions.tmdl", "dev-lake-1111")],
        );
        write_item(
            tmp.path(),
            "MyVL.VariableLibrary",
            "VariableLibrary",
            "MyVL",
            &[("valueSets/dev.json", "{}")],
        );
        let params_path = tmp.path().join("params.json");
        std::fs::write(
            &params_path,
            r#"{"find_replace":[{"find_value":"placeholder","replace_value":{"dev":"dev-lake-1111","feature-x":"feat-lake-9999"}}]}"#,
        )
        .unwrap();

        let (errors, _warnings, _checks) = run_pr_ready_checks(
            tmp.path(),
            Some(&params_path),
            Some("dev"),
            &["dev".to_owned()],
        )
        .unwrap();
        assert!(errors.is_empty(), "expected no errors, got {errors:?}");
    }
}
