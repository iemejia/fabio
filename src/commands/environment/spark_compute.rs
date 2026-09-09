use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

use super::CustomLivePoolSupport;

const ONE_HOUR_SECONDS: u64 = 60 * 60;
const ONE_MINUTE_SECONDS: u64 = 60;
const ONE_DAY_SECONDS: u64 = 24 * ONE_HOUR_SECONDS;
const ONE_WEEK_SECONDS: u64 = 7 * ONE_DAY_SECONDS;
const TWENTY_MINUTES_SECONDS: u64 = 20 * ONE_MINUTE_SECONDS;
const THIRTY_MINUTES_SECONDS: u64 = 30 * ONE_MINUTE_SECONDS;
const TWENTY_FOUR_HOURS_SECONDS: u64 = ONE_DAY_SECONDS;

// ─── Staging Spark Compute ───────────────────────────────────────────────────

/// Parse a `KEY=VALUE` Spark property argument.
fn parse_spark_property(s: &str) -> Result<(String, String)> {
    let (key, value) = s.split_once('=').ok_or_else(|| {
        FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --spark-property '{s}': expected KEY=VALUE"),
            "Example: --spark-property spark.native.enabled=true",
        )
    })?;
    let key = key.trim();
    if key.is_empty() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Invalid --spark-property '{s}': key is empty"),
            "Example: --spark-property spark.native.enabled=true",
        )
        .into());
    }
    Ok((key.to_string(), value.to_string()))
}

pub(super) fn parse_cluster_idle_timeout(value: &str) -> std::result::Result<String, String> {
    validate_iso8601_duration_in_range(
        "--cluster-idle-timeout",
        value,
        TWENTY_MINUTES_SECONDS,
        TWENTY_FOUR_HOURS_SECONDS,
    )
    .map(|_| value.to_string())
}

pub(super) fn parse_custom_live_pool_lifespan(value: &str) -> std::result::Result<String, String> {
    validate_iso8601_duration_in_range(
        "--custom-live-pool-lifespan",
        value,
        THIRTY_MINUTES_SECONDS,
        TWENTY_FOUR_HOURS_SECONDS,
    )
    .map(|_| value.to_string())
}

fn validate_iso8601_duration_in_range(
    flag_name: &str,
    value: &str,
    min_seconds: u64,
    max_seconds: u64,
) -> std::result::Result<u64, String> {
    let seconds = parse_iso8601_duration_seconds(value).map_err(|message| {
        format!(
            "Invalid {flag_name} '{value}': {message}. Use an ISO 8601 duration such as PT20M, PT1H, PT1H30M, or P1D."
        )
    })?;
    if (min_seconds..=max_seconds).contains(&seconds) {
        return Ok(seconds);
    }

    Err(format!(
        "Invalid {flag_name} '{value}': duration must be between {} and {}.",
        format_iso_duration(min_seconds),
        format_iso_duration(max_seconds),
    ))
}

fn parse_iso8601_duration_seconds(value: &str) -> std::result::Result<u64, String> {
    let Some(rest) = value.strip_prefix('P') else {
        return Err("must start with 'P'".to_string());
    };
    if rest.is_empty() {
        return Err("must include at least one duration component".to_string());
    }

    if let Some(weeks) = rest.strip_suffix('W') {
        return parse_week_duration_seconds(weeks);
    }

    let mut digits = String::new();
    let mut total_seconds = 0_u64;
    let mut found_component = false;
    let mut in_time = false;
    let mut found_time_component = false;
    let mut date_order = 0_u8;
    let mut time_order = 0_u8;

    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
            continue;
        }
        if ch == 'T' {
            if in_time {
                return Err("duration contains more than one time designator".to_string());
            }
            if !digits.is_empty() {
                return Err("duration component is missing its unit before 'T'".to_string());
            }
            in_time = true;
            continue;
        }
        if digits.is_empty() {
            return Err("duration component is missing its numeric value".to_string());
        }

        let n = digits
            .parse::<u64>()
            .map_err(|_| "duration value is too large".to_string())?;
        digits.clear();

        let component_seconds = parse_duration_component_seconds(
            in_time,
            ch,
            n,
            &mut date_order,
            &mut time_order,
            &mut found_time_component,
        )?;

        total_seconds = total_seconds
            .checked_add(component_seconds)
            .ok_or_else(|| "duration value is too large".to_string())?;
        found_component = true;
    }

    if !digits.is_empty() {
        return Err("duration is missing a trailing unit (H, M, or S)".to_string());
    }
    if in_time && !found_time_component {
        return Err("time designator 'T' must be followed by a time component".to_string());
    }
    if found_component {
        Ok(total_seconds)
    } else {
        Err("must include at least one duration component".to_string())
    }
}

fn parse_week_duration_seconds(weeks: &str) -> std::result::Result<u64, String> {
    if weeks.is_empty() || weeks.contains('T') || !weeks.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("week duration must use the form PnW".to_string());
    }
    weeks
        .parse::<u64>()
        .map_err(|_| "duration value is too large".to_string())?
        .checked_mul(ONE_WEEK_SECONDS)
        .ok_or_else(|| "duration value is too large".to_string())
}

fn parse_duration_component_seconds(
    in_time: bool,
    unit: char,
    value: u64,
    date_order: &mut u8,
    time_order: &mut u8,
    found_time_component: &mut bool,
) -> std::result::Result<u64, String> {
    match (in_time, unit) {
        (false, 'Y') => zero_only_calendar_component(value, date_order, 1),
        (false, 'M') => zero_only_calendar_component(value, date_order, 2),
        (false, 'D') => {
            ensure_component_order(date_order, 3, "date")?;
            value
                .checked_mul(ONE_DAY_SECONDS)
                .ok_or_else(|| "duration value is too large".to_string())
        }
        (true, 'H') => parse_time_component(value, time_order, 1, ONE_HOUR_SECONDS),
        (true, 'M') => parse_time_component(value, time_order, 2, ONE_MINUTE_SECONDS),
        (true, 'S') => {
            ensure_component_order(time_order, 3, "time")?;
            *found_time_component = true;
            Ok(value)
        }
        _ => Err("unsupported duration component".to_string()),
    }
    .inspect(|_| {
        if in_time {
            *found_time_component = true;
        }
    })
}

fn zero_only_calendar_component(
    value: u64,
    date_order: &mut u8,
    current_order: u8,
) -> std::result::Result<u64, String> {
    ensure_component_order(date_order, current_order, "date")?;
    if value == 0 {
        Ok(0)
    } else {
        Err("calendar year and month components have variable elapsed time".to_string())
    }
}

fn parse_time_component(
    value: u64,
    time_order: &mut u8,
    current_order: u8,
    multiplier: u64,
) -> std::result::Result<u64, String> {
    ensure_component_order(time_order, current_order, "time")?;
    value
        .checked_mul(multiplier)
        .ok_or_else(|| "duration value is too large".to_string())
}

fn ensure_component_order(
    previous: &mut u8,
    current: u8,
    component_kind: &str,
) -> std::result::Result<(), String> {
    if current <= *previous {
        return Err(format!(
            "duplicate or out-of-order {component_kind} duration component"
        ));
    }
    *previous = current;
    Ok(())
}

fn format_iso_duration(seconds: u64) -> String {
    if seconds.is_multiple_of(ONE_HOUR_SECONDS) {
        return format!("PT{}H", seconds / ONE_HOUR_SECONDS);
    }
    if seconds.is_multiple_of(ONE_MINUTE_SECONDS) {
        return format!("PT{}M", seconds / ONE_MINUTE_SECONDS);
    }
    format!("PT{seconds}S")
}

fn sanitize_staging_spark_compute_body(body: &mut Value) {
    if let Some(instance_pool) = body
        .as_object_mut()
        .and_then(|obj| obj.get_mut("instancePool"))
        .and_then(Value::as_object_mut)
    {
        instance_pool.remove("maxClustersToHydrateLimit");
    }
}

/// Apply typed runtime-version / spark-property overrides onto an existing
/// staging sparkcompute object, preserving all other fields (and existing
/// `sparkProperties` keys that are not overridden). Pure function for testing.
#[allow(clippy::too_many_arguments)]
fn apply_spark_compute_overrides(
    mut current: Value,
    runtime_version: Option<&str>,
    spark_properties: &[(String, String)],
    custom_live_pool_support: Option<CustomLivePoolSupport>,
    max_clusters_to_hydrate: Option<i32>,
    cluster_idle_timeout: Option<&str>,
    custom_live_pool_lifespan: Option<&str>,
    clear_custom_live_pool_settings: bool,
) -> Value {
    if !current.is_object() {
        current = Value::Object(serde_json::Map::new());
    }
    let obj = current.as_object_mut().expect("ensured object above");
    if let Some(instance_pool) = obj.get_mut("instancePool").and_then(Value::as_object_mut) {
        instance_pool.remove("maxClustersToHydrateLimit");
    }
    if let Some(rv) = runtime_version {
        obj.insert("runtimeVersion".to_string(), Value::from(rv));
    }
    if !spark_properties.is_empty() {
        let props = obj
            .entry("sparkProperties".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !props.is_object() {
            *props = Value::Object(serde_json::Map::new());
        }
        let pobj = props.as_object_mut().expect("ensured object above");
        for (k, v) in spark_properties {
            pobj.insert(k.clone(), Value::from(v.as_str()));
        }
    }
    if let Some(support) = custom_live_pool_support {
        obj.insert(
            "customLivePoolSupport".to_string(),
            Value::from(support.as_str()),
        );
    }
    if clear_custom_live_pool_settings {
        obj.insert("customLivePoolSettings".to_string(), Value::Null);
    } else if max_clusters_to_hydrate.is_some()
        || cluster_idle_timeout.is_some()
        || custom_live_pool_lifespan.is_some()
    {
        let settings = obj
            .entry("customLivePoolSettings".to_string())
            .or_insert_with(|| Value::Object(serde_json::Map::new()));
        if !settings.is_object() {
            *settings = Value::Object(serde_json::Map::new());
        }
        let settings = settings.as_object_mut().expect("ensured object above");
        if let Some(max) = max_clusters_to_hydrate {
            settings.insert("maxClustersToHydrate".to_string(), Value::from(max));
        }
        if let Some(timeout) = cluster_idle_timeout {
            settings.insert("clusterIdleTimeout".to_string(), Value::from(timeout));
        }
        if let Some(lifespan) = custom_live_pool_lifespan {
            settings.insert("customLivePoolLifespan".to_string(), Value::from(lifespan));
        }
    }
    current
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
pub(super) async fn update_staging_spark_compute(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    file: Option<&str>,
    content: Option<&str>,
    runtime_version: Option<&str>,
    spark_property: &[String],
    custom_live_pool_support: Option<CustomLivePoolSupport>,
    max_clusters_to_hydrate: Option<i32>,
    cluster_idle_timeout: Option<&str>,
    custom_live_pool_lifespan: Option<&str>,
    clear_custom_live_pool_settings: bool,
) -> Result<()> {
    let path = format!("/workspaces/{workspace}/environments/{id}/staging/sparkcompute");

    // Two mutually-exclusive modes:
    //   (a) raw JSON body via --file/--content (full replace of the compute body)
    //   (b) typed overrides via --runtime-version/--spark-property (read-merge-write,
    //       preserving all other fields and existing sparkProperties)
    let raw_body: Option<(Value, usize)> = match (file, content) {
        (Some(path), _) => {
            let s = std::fs::read_to_string(path).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Failed to read file '{path}': {e}"),
                    "Verify the file path is correct and the file is readable.",
                )
            })?;
            let v: Value = serde_json::from_str(&s).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON: {e}"),
                    "Provide valid JSON content via --file or --content.",
                )
            })?;
            Some((v, s.len()))
        }
        (_, Some(c)) => {
            let v: Value = serde_json::from_str(c).map_err(|e| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    format!("Invalid JSON: {e}"),
                    "Provide valid JSON content via --file or --content.",
                )
            })?;
            Some((v, c.len()))
        }
        (None, None) => None,
    };

    let typed_props: Option<Vec<(String, String)>> = if raw_body.is_none()
        && (runtime_version.is_some()
            || !spark_property.is_empty()
            || custom_live_pool_support.is_some()
            || max_clusters_to_hydrate.is_some()
            || cluster_idle_timeout.is_some()
            || custom_live_pool_lifespan.is_some()
            || clear_custom_live_pool_settings)
    {
        Some(
            spark_property
                .iter()
                .map(|s| parse_spark_property(s))
                .collect::<Result<Vec<_>, _>>()?,
        )
    } else {
        None
    };

    if raw_body.is_none() && typed_props.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Provide either --file/--content or at least one typed Spark compute flag".to_string(),
            "Example: fabio environment update-staging-spark-compute --workspace <WS> --id <ID> --custom-live-pool-support Enabled --max-clusters-to-hydrate 4 --cluster-idle-timeout PT20M --custom-live-pool-lifespan PT1H".to_string(),
        ).into());
    }

    // Build a dry-run preview from the inputs (no network call for the typed path).
    let preview = if let Some((_, len)) = &raw_body {
        serde_json::json!({ "workspace": workspace, "id": id, "contentLength": len })
    } else {
        let mut preview = serde_json::json!({
            "workspace": workspace,
            "id": id,
        });
        if let Some(runtime_version) = runtime_version {
            preview["runtimeVersion"] = Value::from(runtime_version);
        }
        if let Some(typed_props) = &typed_props
            && !typed_props.is_empty()
        {
            preview["sparkProperties"] = Value::Object(
                typed_props
                    .iter()
                    .map(|(key, value)| (key.clone(), Value::from(value.as_str())))
                    .collect(),
            );
        }
        if let Some(support) = custom_live_pool_support {
            preview["customLivePoolSupport"] = Value::from(support.as_str());
        }
        if clear_custom_live_pool_settings {
            preview["customLivePoolSettings"] = Value::Null;
        } else {
            let mut settings = serde_json::Map::new();
            if let Some(max) = max_clusters_to_hydrate {
                settings.insert("maxClustersToHydrate".to_string(), Value::from(max));
            }
            if let Some(timeout) = cluster_idle_timeout {
                settings.insert("clusterIdleTimeout".to_string(), Value::from(timeout));
            }
            if let Some(lifespan) = custom_live_pool_lifespan {
                settings.insert("customLivePoolLifespan".to_string(), Value::from(lifespan));
            }
            if !settings.is_empty() {
                preview["customLivePoolSettings"] = Value::Object(settings);
            }
        }
        preview
    };

    if output::dry_run_guard(cli, "environment update-staging-spark-compute", &preview) {
        return Ok(());
    }

    let mut body = if let Some((v, _)) = raw_body {
        v
    } else {
        // read-merge-write: fetch current staging compute, apply overrides.
        let current = client.get(&path).await.map_err(|e| {
            enrich_forbidden(e, "environment update-staging-spark-compute", "Contributor")
        })?;
        apply_spark_compute_overrides(
            current,
            runtime_version,
            &typed_props.unwrap_or_default(),
            custom_live_pool_support,
            max_clusters_to_hydrate,
            cluster_idle_timeout,
            custom_live_pool_lifespan,
            clear_custom_live_pool_settings,
        )
    };
    sanitize_staging_spark_compute_body(&mut body);

    let data = client.patch(&path, &body).await.map_err(|e| {
        enrich_forbidden(e, "environment update-staging-spark-compute", "Contributor")
    })?;

    if data.is_null() || data.as_object().is_some_and(serde_json::Map::is_empty) {
        let obj = serde_json::json!({ "id": id, "status": "spark_compute_updated" });
        output::render_object(cli, &obj, "status");
    } else {
        output::render_object(cli, &data, "id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_spark_property_splits_key_value() {
        let (k, v) = parse_spark_property("spark.native.enabled=true").unwrap();
        assert_eq!(k, "spark.native.enabled");
        assert_eq!(v, "true");
    }

    #[test]
    fn parse_spark_property_trims_key_and_preserves_value() {
        // Value may itself contain '=' (only the first '=' splits).
        let (k, v) = parse_spark_property(" spark.conf.key = a=b=c ").unwrap();
        assert_eq!(k, "spark.conf.key");
        assert_eq!(v, " a=b=c ");
    }

    #[test]
    fn parse_spark_property_rejects_missing_equals() {
        assert!(parse_spark_property("spark.native.enabled").is_err());
    }

    #[test]
    fn parse_spark_property_rejects_empty_key() {
        assert!(parse_spark_property("=true").is_err());
        assert!(parse_spark_property("   =true").is_err());
    }

    #[test]
    fn parse_cluster_idle_timeout_enforces_iso_and_bounds() {
        assert!(parse_cluster_idle_timeout("PT20M").is_ok());
        assert!(parse_cluster_idle_timeout("PT24H").is_ok());
        assert!(parse_cluster_idle_timeout("P1D").is_ok());
        assert!(parse_cluster_idle_timeout("PT19M").is_err());
        assert!(parse_cluster_idle_timeout("PT24H1M").is_err());
        assert!(parse_cluster_idle_timeout("P1DT1S").is_err());
        assert!(parse_cluster_idle_timeout("foo").is_err());
    }

    #[test]
    fn parse_custom_live_pool_lifespan_enforces_iso_and_bounds() {
        assert!(parse_custom_live_pool_lifespan("PT30M").is_ok());
        assert!(parse_custom_live_pool_lifespan("PT24H").is_ok());
        assert!(parse_custom_live_pool_lifespan("P1D").is_ok());
        assert!(parse_custom_live_pool_lifespan("P0DT30M").is_ok());
        assert!(parse_custom_live_pool_lifespan("PT29M59S").is_err());
        assert!(parse_custom_live_pool_lifespan("PT25H").is_err());
        assert!(parse_custom_live_pool_lifespan("bar").is_err());
    }

    #[test]
    fn parse_iso8601_duration_seconds_supports_composite_values() {
        assert_eq!(parse_iso8601_duration_seconds("PT1H30M"), Ok(5400));
        assert_eq!(parse_iso8601_duration_seconds("PT45M15S"), Ok(2715));
        assert_eq!(parse_iso8601_duration_seconds("P1D"), Ok(86400));
        assert_eq!(parse_iso8601_duration_seconds("P1DT30M"), Ok(88200));
        assert_eq!(parse_iso8601_duration_seconds("P2W"), Ok(1_209_600));
        assert!(parse_iso8601_duration_seconds("PT1M1H").is_err());
        assert!(parse_iso8601_duration_seconds("P1M").is_err());
    }

    #[test]
    fn apply_overrides_sets_runtime_version_only() {
        let current = json!({
            "runtimeVersion": "1.3",
            "driverCores": 8,
            "sparkProperties": { "existing.key": "keep" }
        });
        let out =
            apply_spark_compute_overrides(current, Some("2.0"), &[], None, None, None, None, false);
        assert_eq!(out["runtimeVersion"], json!("2.0"));
        // Other fields preserved.
        assert_eq!(out["driverCores"], json!(8));
        // sparkProperties untouched when none supplied.
        assert_eq!(out["sparkProperties"]["existing.key"], json!("keep"));
    }

    #[test]
    fn apply_overrides_merges_spark_properties_preserving_existing() {
        let current = json!({
            "runtimeVersion": "1.3",
            "sparkProperties": { "existing.key": "keep", "override.me": "old" }
        });
        let props = vec![
            ("spark.native.enabled".to_string(), "true".to_string()),
            ("override.me".to_string(), "new".to_string()),
        ];
        let out =
            apply_spark_compute_overrides(current, None, &props, None, None, None, None, false);
        // runtimeVersion unchanged when not supplied.
        assert_eq!(out["runtimeVersion"], json!("1.3"));
        // existing preserved, new added, overridden replaced.
        assert_eq!(out["sparkProperties"]["existing.key"], json!("keep"));
        assert_eq!(
            out["sparkProperties"]["spark.native.enabled"],
            json!("true")
        );
        assert_eq!(out["sparkProperties"]["override.me"], json!("new"));
    }

    #[test]
    fn apply_overrides_creates_spark_properties_when_absent() {
        let current = json!({ "runtimeVersion": "2.0" });
        let props = vec![("k".to_string(), "v".to_string())];
        let out =
            apply_spark_compute_overrides(current, None, &props, None, None, None, None, false);
        assert_eq!(out["sparkProperties"]["k"], json!("v"));
    }

    #[test]
    fn apply_overrides_recovers_from_non_object_input() {
        let out = apply_spark_compute_overrides(
            json!("not-an-object"),
            Some("2.0"),
            &[],
            None,
            None,
            None,
            None,
            false,
        );
        assert_eq!(out["runtimeVersion"], json!("2.0"));
    }

    #[test]
    fn apply_overrides_merges_custom_live_pool_settings() {
        let current = json!({
            "instancePool": {
                "id": "pool-id",
                "maxClustersToHydrateLimit": 10
            },
            "customLivePoolSupport": "Disabled",
            "customLivePoolSettings": {
                "maxClustersToHydrate": 2,
                "clusterIdleTimeout": "PT30M"
            }
        });
        let out = apply_spark_compute_overrides(
            current,
            None,
            &[],
            Some(CustomLivePoolSupport::Enabled),
            Some(4),
            None,
            Some("PT1H"),
            false,
        );
        assert_eq!(out["customLivePoolSupport"], "Enabled");
        assert_eq!(out["customLivePoolSettings"]["maxClustersToHydrate"], 4);
        assert_eq!(out["customLivePoolSettings"]["clusterIdleTimeout"], "PT30M");
        assert_eq!(
            out["customLivePoolSettings"]["customLivePoolLifespan"],
            "PT1H"
        );
        assert_eq!(out["instancePool"]["id"], "pool-id");
        assert!(
            out["instancePool"]
                .get("maxClustersToHydrateLimit")
                .is_none()
        );
    }

    #[test]
    fn apply_overrides_can_clear_custom_live_pool_settings() {
        let current = json!({
            "customLivePoolSupport": "Enabled",
            "customLivePoolSettings": {"maxClustersToHydrate": 4}
        });
        let out = apply_spark_compute_overrides(
            current,
            None,
            &[],
            Some(CustomLivePoolSupport::Disabled),
            None,
            None,
            None,
            true,
        );
        assert_eq!(out["customLivePoolSupport"], "Disabled");
        assert!(out["customLivePoolSettings"].is_null());
    }

    #[test]
    fn sanitize_staging_spark_compute_body_removes_read_only_limit() {
        let mut body = json!({
            "instancePool": {
                "id": "pool-id",
                "maxClustersToHydrateLimit": 12
            }
        });
        sanitize_staging_spark_compute_body(&mut body);
        assert_eq!(body["instancePool"]["id"], "pool-id");
        assert!(
            body["instancePool"]
                .get("maxClustersToHydrateLimit")
                .is_none()
        );
    }
}
