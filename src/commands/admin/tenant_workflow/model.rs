use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use serde::{Deserialize, Serialize, de};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::client::{is_secure_or_loopback, validate_trusted_url, validate_uuid};
use crate::errors::{ErrorCode, FabioError};

pub(super) const VERSION: u32 = 1;
pub(super) const WARNING: &str = "Tenant-wide governance changes may remove access or weaken security. Review every change; there is no automatic rollback or transactional/conditional update guarantee.";

pub(super) fn invalid(message: impl Into<String>) -> anyhow::Error {
    FabioError::with_hint(
        ErrorCode::InvalidInput,
        message,
        "Use a fresh 'fabio admin export-tenant-settings --out settings.yaml', edit only supported values, then create and review a new plan.",
    )
    .into()
}

pub(super) fn digest(value: &impl Serialize) -> Result<String> {
    fn sort(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.sort_keys();
                for value in map.values_mut() {
                    sort(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    sort(value);
                }
            }
            _ => {}
        }
    }
    let mut value = serde_json::to_value(value)?;
    sort(&mut value);
    Ok(bytes_digest(&serde_json::to_vec(&value)?))
}

pub(super) fn bytes_digest(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut result = String::with_capacity(64);
    for byte in Sha256::digest(bytes) {
        write!(result, "{byte:02x}").expect("writing to a string cannot fail");
    }
    result
}

// Deserialize through a duplicate-checking tree before typed validation. In
// particular, duplicate keys inside free-form API metadata must not be lost.
struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: de::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("an untagged YAML value with unique string mapping keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(n.into()))
                    .ok_or_else(|| E::custom("non-finite number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(StrictValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(StrictValue(values.into()))
            }
            fn visit_map<A: de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = BTreeMap::new();
                while let Some(key) = map.next_key::<String>()? {
                    let StrictValue(value) = map.next_value()?;
                    if values.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!("duplicate YAML key: {key}")));
                    }
                }
                Ok(StrictValue(
                    serde_json::to_value(values).map_err(de::Error::custom)?,
                ))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

pub(super) fn parse<T: de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let StrictValue(value) =
        serde_yaml::from_slice(bytes).map_err(|e| invalid(format!("Invalid YAML: {e}")))?;
    serde_json::from_value(value).map_err(|e| invalid(format!("Invalid document: {e}")))
}

fn present<'de, D: de::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Group {
    pub graph_id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub name: Option<String>,
}

impl PartialEq for Group {
    fn eq(&self, other: &Self) -> bool {
        self.graph_id == other.graph_id
    }
}
impl Eq for Group {}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Values {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub enabled: Option<bool>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub enabled_security_groups: Option<Vec<Group>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub excluded_security_groups: Option<Vec<Group>>,
}

impl Values {
    pub fn normalize(&mut self) -> Result<()> {
        for groups in [
            &mut self.enabled_security_groups,
            &mut self.excluded_security_groups,
        ]
        .into_iter()
        .flatten()
        {
            let mut ids = BTreeSet::new();
            for group in groups.iter_mut() {
                validate_uuid(&group.graph_id, "graphId")?;
                group.graph_id.make_ascii_lowercase();
                if !ids.insert(group.graph_id.clone()) {
                    return Err(invalid("Duplicate security-group graphId"));
                }
            }
            groups.sort_by(|a, b| a.graph_id.cmp(&b.graph_id));
        }
        if let (Some(enabled), Some(excluded)) = (
            &self.enabled_security_groups,
            &self.excluded_security_groups,
        ) {
            let ids: BTreeSet<_> = enabled.iter().map(|g| &g.graph_id).collect();
            if excluded.iter().any(|g| ids.contains(&g.graph_id)) {
                return Err(invalid(
                    "The same security group cannot be both enabled and excluded",
                ));
            }
        }
        Ok(())
    }

    pub fn merge(&self, desired: &Self) -> Self {
        Self {
            enabled: desired.enabled.or(self.enabled),
            enabled_security_groups: desired
                .enabled_security_groups
                .clone()
                .or_else(|| self.enabled_security_groups.clone()),
            excluded_security_groups: desired
                .excluded_security_groups
                .clone()
                .or_else(|| self.excluded_security_groups.clone()),
        }
    }

    pub fn changes(&self, after: &Self) -> BTreeMap<String, Change> {
        let mut changes = BTreeMap::new();
        if self.enabled != after.enabled {
            changes.insert(
                "enabled".into(),
                Change {
                    before: json!(self.enabled),
                    after: json!(after.enabled),
                },
            );
        }
        for (name, before, after) in [
            (
                "enabledSecurityGroups",
                &self.enabled_security_groups,
                &after.enabled_security_groups,
            ),
            (
                "excludedSecurityGroups",
                &self.excluded_security_groups,
                &after.excluded_security_groups,
            ),
        ] {
            if before != after {
                changes.insert(
                    name.into(),
                    Change {
                        before: json!(before),
                        after: json!(after),
                    },
                );
            }
        }
        changes
    }
}

pub(super) fn setting_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 256
        || !name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err(invalid(
            "settingName must contain only letters, digits, underscores or hyphens",
        ));
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[allow(clippy::struct_field_names)]
pub(super) struct Setting {
    pub setting_name: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, Value>,
    #[serde(default)]
    pub values: Values,
}

impl Setting {
    pub fn from_api(value: Value) -> Result<Self> {
        let Value::Object(mut raw) = value else {
            return Err(FabioError::api_error("Expected a tenant setting object").into());
        };
        let name = raw
            .remove("settingName")
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| {
                FabioError::api_error("Tenant setting response is missing settingName")
            })?;
        setting_name(&name)?;
        let mut writable = serde_json::Map::new();
        for key in ["enabled", "enabledSecurityGroups", "excludedSecurityGroups"] {
            if let Some(value) = raw.remove(key) {
                writable.insert(key.into(), value);
            }
        }
        let mut values: Values = serde_json::from_value(writable.into())
            .map_err(|e| FabioError::api_error(format!("Invalid values for {name}: {e}")))?;
        if values.enabled.is_none() {
            return Err(FabioError::api_error(format!("Missing enabled for {name}")).into());
        }
        values.normalize()?;
        Ok(Self {
            setting_name: name,
            metadata: raw.into_iter().collect(),
            values,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Desired {
    pub schema_version: u32,
    pub kind: String,
    pub tenant_id: String,
    pub endpoint: String,
    pub settings: Vec<Setting>,
}

pub(super) fn binding(version: u32, tenant: &str, endpoint: &str) -> Result<()> {
    if version != VERSION {
        return Err(invalid(format!(
            "Unsupported schemaVersion {version}; expected {VERSION}"
        )));
    }
    validate_uuid(tenant, "tenantId")?;
    validate_trusted_url(endpoint, "endpoint")?;
    let url = reqwest::Url::parse(endpoint).map_err(|_| invalid("Invalid endpoint"))?;
    if !is_secure_or_loopback(endpoint)
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || endpoint.ends_with('/')
    {
        return Err(invalid(
            "Endpoint must be an HTTPS API base URL without credentials, query, fragment or trailing slash (HTTP is allowed only on loopback)",
        ));
    }
    Ok(())
}

impl Desired {
    pub fn validate(&mut self) -> Result<()> {
        binding(self.schema_version, &self.tenant_id, &self.endpoint)?;
        if self.kind != "tenant-settings" {
            return Err(invalid("Expected kind: tenant-settings"));
        }
        let mut names = BTreeSet::new();
        for setting in &mut self.settings {
            setting_name(&setting.setting_name)?;
            if !names.insert(&setting.setting_name) {
                return Err(invalid(format!(
                    "Duplicate setting: {}",
                    setting.setting_name
                )));
            }
            setting.values.normalize()?;
        }
        self.settings
            .sort_by(|a, b| a.setting_name.cmp(&b.setting_name));
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Change {
    before: Value,
    after: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Update {
    pub setting_name: String,
    pub before: Values,
    pub after: Values,
    pub changes: BTreeMap<String, Change>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Plan {
    pub schema_version: u32,
    pub kind: String,
    pub tenant_id: String,
    pub endpoint: String,
    pub created_at: String,
    pub source_digest: String,
    pub destructive: bool,
    pub warnings: Vec<String>,
    pub unchanged: usize,
    pub updates: Vec<Update>,
    pub digest: String,
}

impl Plan {
    pub fn seal(&mut self) -> Result<()> {
        self.digest.clear();
        self.digest = digest(self)?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        binding(self.schema_version, &self.tenant_id, &self.endpoint)?;
        if self.kind != "tenant-settings-plan"
            || chrono::DateTime::parse_from_rfc3339(&self.created_at).is_err()
        {
            return Err(invalid("Invalid plan kind or creation time"));
        }
        if self.source_digest.len() != 64
            || !self.source_digest.bytes().all(|c| c.is_ascii_hexdigit())
        {
            return Err(invalid("Invalid sourceDigest"));
        }
        let mut copy = self.clone();
        copy.seal()?;
        if copy.digest != self.digest {
            return Err(invalid(
                "Plan integrity mismatch; regenerate the plan after editing desired settings",
            ));
        }
        if self.destructive == self.updates.is_empty()
            || self.warnings
                != if self.destructive {
                    vec![WARNING.to_owned()]
                } else {
                    vec![]
                }
        {
            return Err(invalid(
                "Plan safety markers are inconsistent with its updates",
            ));
        }
        let mut last: Option<&str> = None;
        for update in &self.updates {
            setting_name(&update.setting_name)?;
            if last.is_some_and(|n| n >= update.setting_name.as_str()) {
                return Err(invalid(
                    "Plan updates must have unique, sorted setting names",
                ));
            }
            last = Some(&update.setting_name);
            for values in [&update.before, &update.after] {
                let mut normalized = values.clone();
                normalized.normalize()?;
                if values.enabled.is_none() || normalized != *values {
                    return Err(invalid(
                        "Plan values require enabled and sorted security groups",
                    ));
                }
            }
            if update.before == update.after
                || update.before.changes(&update.after) != update.changes
            {
                return Err(invalid(
                    "Plan changes are inconsistent with before/after values",
                ));
            }
            // A full update payload must not silently drop a captured optional scope.
            if (update.before.enabled_security_groups.is_some()
                && update.after.enabled_security_groups.is_none())
                || (update.before.excluded_security_groups.is_some()
                    && update.after.excluded_security_groups.is_none())
            {
                return Err(invalid(
                    "Plan after values omit a baseline scope; use an explicit empty collection to clear it",
                ));
            }
        }
        Ok(())
    }
}

pub(super) fn build_plan(
    desired: &Desired,
    snapshot: &[Setting],
    source_digest: String,
) -> Result<Plan> {
    let current: BTreeMap<_, _> = snapshot
        .iter()
        .map(|s| (s.setting_name.as_str(), s))
        .collect();
    let mut plan = Plan {
        schema_version: VERSION,
        kind: "tenant-settings-plan".into(),
        tenant_id: desired.tenant_id.clone(),
        endpoint: desired.endpoint.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        source_digest,
        destructive: false,
        warnings: Vec::new(),
        unchanged: 0,
        updates: Vec::new(),
        digest: String::new(),
    };
    for setting in &desired.settings {
        let Some(live) = current.get(setting.setting_name.as_str()) else {
            return Err(invalid(format!(
                "Unknown tenant setting: {}",
                setting.setting_name
            )));
        };
        for (key, value) in &setting.metadata {
            if live.metadata.get(key) != Some(value) {
                return Err(invalid(format!(
                    "{}: metadata.{key} is read-only or has changed; re-export before planning",
                    setting.setting_name
                )));
            }
        }
        let mut after = live.values.merge(&setting.values);
        after.normalize()?;
        validate_capabilities(live, &after)?;
        if live.values == after {
            plan.unchanged += 1;
        } else {
            plan.updates.push(Update {
                setting_name: setting.setting_name.clone(),
                before: live.values.clone(),
                changes: live.values.changes(&after),
                after,
            });
        }
    }
    plan.destructive = !plan.updates.is_empty();
    if plan.destructive {
        plan.warnings.push(WARNING.to_owned());
    }
    plan.seal()?;
    plan.validate()?;
    Ok(plan)
}

pub(super) fn validate_capabilities(live: &Setting, after: &Values) -> Result<()> {
    if live.values == *after {
        return Ok(());
    }
    if live.metadata.get("isReadOnly").and_then(Value::as_bool) == Some(true)
        || live.metadata.get("canUpdate").and_then(Value::as_bool) == Some(false)
    {
        return Err(invalid(format!("{} is not writable", live.setting_name)));
    }
    if (live.values.enabled_security_groups != after.enabled_security_groups
        || live.values.excluded_security_groups != after.excluded_security_groups)
        && live
            .metadata
            .get("canSpecifySecurityGroups")
            .and_then(Value::as_bool)
            != Some(true)
    {
        return Err(invalid(format!(
            "{} does not advertise security-group scope support",
            live.setting_name
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(name: &str, enabled: bool) -> Setting {
        Setting::from_api(
            json!({"settingName":name,"enabled":enabled,"canSpecifySecurityGroups":true}),
        )
        .unwrap()
    }

    fn desired(settings: Vec<Setting>) -> Desired {
        Desired {
            schema_version: VERSION,
            kind: "tenant-settings".into(),
            tenant_id: uuid::Uuid::nil().to_string(),
            endpoint: "https://api.fabric.microsoft.com/v1".into(),
            settings,
        }
    }

    #[test]
    fn test_strict_yaml_and_nulls() {
        for text in [
            "enabled: true\nenabled: false",
            "metadata: {x: 1, x: 2}",
            "enabled: null",
            "enabled: 'true'",
            "enabledSecurityGroups: null",
            "unknown: true",
            "enabled: !custom true",
        ] {
            assert!(parse::<Values>(text.as_bytes()).is_err(), "{text}");
        }
        assert_eq!(parse::<Values>(b"{}").unwrap(), Values::default());
        assert_eq!(
            parse::<Values>(b"enabledSecurityGroups: []")
                .unwrap()
                .enabled_security_groups,
            Some(vec![])
        );
    }

    #[test]
    fn test_export_roundtrip_partial_and_noop() {
        let snapshot = vec![setting("A", true), setting("B", false)];
        let input = desired(snapshot.clone());
        let yaml = serde_yaml::to_string(&input).unwrap();
        let mut parsed: Desired = parse(yaml.as_bytes()).unwrap();
        parsed.validate().unwrap();
        assert_eq!(
            build_plan(&parsed, &snapshot, bytes_digest(yaml.as_bytes()))
                .unwrap()
                .updates
                .len(),
            0
        );
        parsed.settings.truncate(1);
        parsed.settings[0].values.enabled = None;
        assert_eq!(
            build_plan(&parsed, &snapshot, bytes_digest(b"x"))
                .unwrap()
                .unchanged,
            1
        );
    }

    #[test]
    fn test_group_order_and_names_are_not_changes() {
        let a = uuid::Uuid::from_u128(1).to_string();
        let b = uuid::Uuid::from_u128(2).to_string();
        let mut left: Values = parse(
            format!("enabledSecurityGroups: [{{graphId: {a}, name: Old}}, {{graphId: {b}}}]")
                .as_bytes(),
        )
        .unwrap();
        let mut right: Values = parse(
            format!("enabledSecurityGroups: [{{graphId: {b}}}, {{graphId: {a}, name: New}}]")
                .as_bytes(),
        )
        .unwrap();
        left.normalize().unwrap();
        right.normalize().unwrap();
        assert_eq!(left, right);
        assert_eq!(left.changes(&right).len(), 0);
        right.enabled_security_groups = Some(vec![]);
        assert_eq!(left.changes(&right).len(), 1);
    }

    #[test]
    fn test_plan_integrity_and_unsupported_edits() {
        let snapshot = vec![setting("A", false)];
        let mut input = desired(snapshot.clone());
        input.settings[0].values.enabled = Some(true);
        let plan = build_plan(&input, &snapshot, bytes_digest(b"x")).unwrap();
        assert_eq!(plan.updates.len(), 1);
        plan.validate().unwrap();
        let mut changed = plan;
        changed.updates[0].after.enabled = Some(false);
        assert!(changed.validate().is_err());
        changed.seal().unwrap();
        assert!(changed.validate().is_err());
        input.settings[0]
            .metadata
            .insert("canSpecifySecurityGroups".into(), false.into());
        assert!(build_plan(&input, &snapshot, bytes_digest(b"x")).is_err());
        input.settings[0].setting_name = "Unknown".into();
        assert!(build_plan(&input, &snapshot, bytes_digest(b"x")).is_err());
    }

    #[test]
    fn test_duplicate_settings_versions_and_unsafe_names() {
        let mut input = desired(vec![setting("A", false), setting("A", true)]);
        assert!(input.validate().is_err());
        input.settings.clear();
        input.schema_version = 2;
        assert!(input.validate().is_err());
        for name in ["", "*", "../A", "A?x=y", "A/B"] {
            assert!(setting_name(name).is_err());
        }
    }

    #[test]
    fn test_scope_validation_and_readonly_settings() {
        let id = uuid::Uuid::new_v4().to_string();
        for value in [
            json!({"enabledSecurityGroups":[{"graphId":id},{"graphId":id}]}),
            json!({"enabledSecurityGroups":[{"graphId":id}],"excludedSecurityGroups":[{"graphId":id}]}),
            json!({"enabledSecurityGroups":[{"graphId":"not-a-uuid"}]}),
        ] {
            let mut values: Values = serde_json::from_value(value).unwrap();
            assert!(values.normalize().is_err());
        }
        let mut live = setting("A", false);
        live.metadata
            .insert("canSpecifySecurityGroups".into(), false.into());
        let scoped: Values =
            parse(format!("enabled: true\nenabledSecurityGroups: [{{graphId: {id}}}]").as_bytes())
                .unwrap();
        assert!(validate_capabilities(&live, &scoped).is_err());
        live.metadata.insert("isReadOnly".into(), true.into());
        assert!(
            validate_capabilities(
                &live,
                &Values {
                    enabled: Some(true),
                    ..Values::default()
                }
            )
            .is_err()
        );
        assert!(validate_capabilities(&live, &live.values).is_ok());
    }
}
