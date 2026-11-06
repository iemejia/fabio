//! Bounded source observations shared by standalone checks and creation gates.
//! Ready is scoped evidence, not a promise that native creation or DAX will work.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::Cli;
use crate::client::{FabricClient, MetadataPage};
use crate::commands::diagnostics::{
    Check, PageCursor, ReadBudget, Status, error_evidence, id, summary, uuid_target,
};
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

#[path = "source_delta.rs"]
mod delta;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Native,
    Generated,
}

#[derive(Debug, Default, Args)]
pub struct Limits {
    /// Total seconds for preflight reads, not generator SQL discovery or creation (default 30)
    #[arg(long="source-timeout",value_parser=clap::value_parser!(u64).range(1..=300))]
    timeout: Option<u64>,
    /// Total non-recursive `OneLake` directory pages across all sources (default 3)
    #[arg(long="source-page-limit",value_parser=clap::value_parser!(u8).range(1..=100))]
    page_limit: Option<u8>,
    /// Total Delta JSON commit files across all sources (default 5)
    #[arg(long="source-log-limit",value_parser=clap::value_parser!(u8).range(1..=100))]
    log_limit: Option<u8>,
    /// Total `OneLake` metadata body-byte budget; no Parquet/row scans (default 1048576)
    #[arg(long="source-byte-limit",value_parser=clap::value_parser!(u64).range(1024..=8_388_608))]
    byte_limit: Option<u64>,
}

impl Limits {
    pub(super) fn validate_creation(&self, enabled: bool) -> Result<()> {
        if !enabled
            && (self.timeout.is_some()
                || self.page_limit.is_some()
                || self.log_limit.is_some()
                || self.byte_limit.is_some())
        {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Source-check limits require --check-source on creation.","Use the explicit --check-source option with selected native or generated sources, or run semantic-model check-source without creating an item.").into());
        }
        Ok(())
    }
    fn bounds(&self) -> Bounds {
        Bounds {
            timeout_seconds: self.timeout.unwrap_or(30),
            page_limit: self.page_limit.unwrap_or(3),
            log_limit: self.log_limit.unwrap_or(5),
            one_lake_body_byte_limit: usize::try_from(self.byte_limit.unwrap_or(1_048_576))
                .expect("bounded byte limit"),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Bounds {
    timeout_seconds: u64,
    page_limit: u8,
    log_limit: u8,
    one_lake_body_byte_limit: usize,
}

pub(super) struct Table {
    workspace: Uuid,
    item: Uuid,
    path: String,
    name: String,
    schema_qualified: bool,
}

impl Table {
    pub(super) fn parse(value: &str) -> Result<Self> {
        let invalid = || {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Invalid canonical OneLake table URL.",
                "Use https://onelake.dfs.fabric.microsoft.com/{workspaceId}/{itemId}/Tables/{tableName} or .../Tables/{schemaName}/{tableName}, with non-zero UUIDs and no credentials, query, fragment, traversal or encoded separators.",
            )
        };
        if value.len() > 8192 || value.trim() != value || value.chars().any(char::is_control) {
            return Err(invalid().into());
        }
        let parsed = reqwest::Url::parse(value).map_err(|_| invalid())?;
        if parsed.scheme() != "https"
            || parsed.host_str() != Some("onelake.dfs.fabric.microsoft.com")
            || parsed.port().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(invalid().into());
        }
        let raw = value
            .split_once("://")
            .and_then(|(_, rest)| rest.split_once('/'))
            .map(|(_, path)| path)
            .ok_or_else(invalid)?;
        if raw.split('/').take(2).any(|part| part.len() != 36) {
            return Err(invalid().into());
        }
        let parts: Vec<String> = raw
            .split('/')
            .map(|part| {
                urlencoding::decode(part)
                    .map(std::borrow::Cow::into_owned)
                    .map_err(|_| anyhow::Error::from(invalid()))
            })
            .collect::<Result<_>>()?;
        if !matches!(parts.len(), 4 | 5)
            || parts[0].len() != 36
            || parts[1].len() != 36
            || parts[2] != "Tables"
            || parts.iter().any(|part| {
                part.is_empty()
                    || part.len() > 256
                    || matches!(part.as_str(), "." | "..")
                    || part.contains(['/', '\\'])
                    || part.chars().any(char::is_control)
            })
        {
            return Err(invalid().into());
        }
        Ok(Self {
            workspace: uuid_target(&parts[0], "OneLake source workspace")?,
            item: uuid_target(&parts[1], "OneLake source item")?,
            path: parts[2..].join("/"),
            name: parts.last().expect("validated path").clone(),
            schema_qualified: parts.len() == 5,
        })
    }
    fn inspect(&self) -> String {
        format!(
            "fabio --wrap-untrusted --readonly item show --workspace {} --id {}",
            self.workspace, self.item
        )
    }
    fn log_dir(&self) -> String {
        format!("{}/_delta_log", self.path)
    }
}

pub(super) fn selected(urls: &[String]) -> Result<Vec<Table>> {
    if urls.is_empty() || urls.len() > 100 {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Source checks require between 1 and 100 explicit OneLake table URLs.","Repeat --one-lake-table for the exact sources to inspect; no workspace-wide discovery is performed.").into());
    }
    let tables: Vec<_> = urls
        .iter()
        .map(|value| Table::parse(value))
        .collect::<Result<_>>()?;
    let mut seen = HashSet::new();
    if tables
        .iter()
        .any(|table| !seen.insert((table.workspace, table.item, table.path.clone())))
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Source selection contains duplicate table references.","Select each exact OneLake table once; table-name collisions between distinct sources are reported separately.").into());
    }
    Ok(tables)
}

#[derive(Clone)]
struct Metadata {
    kind: &'static str,
    layout: Option<bool>,
}

async fn metadata(
    client: &FabricClient,
    budget: &ReadBudget,
    table: &Table,
) -> std::result::Result<Metadata, Value> {
    let body = budget
        .get(
            client,
            &format!("/workspaces/{}/items/{}", table.workspace, table.item),
        )
        .await
        .map_err(|error| error_evidence(&error))?;
    if id(&body["id"]) != Some(table.item)
        || body
            .get("workspaceId")
            .is_some_and(|value| id(value) != Some(table.workspace))
    {
        return Err(json!({"reason":"source_identity_mismatch"}));
    }
    let kind = match body["type"].as_str() {
        Some("Lakehouse") => "Lakehouse",
        Some("Warehouse") => "Warehouse",
        _ => "Other",
    };
    if kind != "Lakehouse" {
        return Ok(Metadata {
            kind,
            layout: if kind == "Warehouse" {
                Some(true)
            } else {
                None
            },
        });
    }
    let body = budget
        .get(
            client,
            &format!("/workspaces/{}/lakehouses/{}", table.workspace, table.item),
        )
        .await
        .map_err(|error| error_evidence(&error))?;
    if id(&body["id"]) != Some(table.item)
        || body
            .get("workspaceId")
            .is_some_and(|value| id(value) != Some(table.workspace))
        || !body["properties"].is_object()
    {
        return Err(json!({"reason":"lakehouse_metadata_malformed"}));
    }
    let layout = match body["properties"].get("defaultSchema") {
        None => Some(false),
        Some(Value::String(schema)) if !schema.is_empty() => Some(true),
        _ => None,
    };
    Ok(Metadata { kind, layout })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Usage {
    pages: u8,
    log_files: u8,
    body_bytes_charged: usize,
    byte_usage_exact: bool,
}

struct Budget {
    reads: ReadBudget,
    bounds: Bounds,
    usage: Usage,
}

impl Budget {
    const fn remaining(&self) -> usize {
        self.bounds
            .one_lake_body_byte_limit
            .saturating_sub(self.usage.body_bytes_charged)
    }
    const fn failed(&mut self) {
        // A failed bounded read may have consumed an unknown body prefix. Charge
        // the entire remaining budget rather than issue more unaccounted reads.
        self.usage.body_bytes_charged = self.bounds.one_lake_body_byte_limit;
        self.usage.byte_usage_exact = false;
    }
    async fn page(
        &mut self,
        client: &FabricClient,
        table: &Table,
        token: Option<&str>,
    ) -> Result<MetadataPage> {
        if self.usage.pages >= self.bounds.page_limit {
            return Err(unavailable("source_page_limit"));
        }
        if self.remaining() == 0 {
            return Err(unavailable("source_byte_limit"));
        }
        self.usage.pages += 1;
        let result = self
            .reads
            .run(client.list_onelake_metadata_page(
                &table.workspace.to_string(),
                &table.item.to_string(),
                &table.log_dir(),
                token,
                25,
                self.remaining(),
            ))
            .await;
        match result {
            Ok(page) => {
                self.usage.body_bytes_charged += page.bytes_read;
                Ok(page)
            }
            Err(error) => {
                self.failed();
                Err(error)
            }
        }
    }
    async fn file(
        &mut self,
        client: &FabricClient,
        table: &Table,
        version: u64,
    ) -> Result<Vec<u8>> {
        if self.usage.log_files >= self.bounds.log_limit {
            return Err(unavailable("source_log_limit"));
        }
        if self.remaining() == 0 {
            return Err(unavailable("source_byte_limit"));
        }
        self.usage.log_files += 1;
        let path = format!("{}/{version:020}.json", table.log_dir());
        let result = self
            .reads
            .run(client.download_onelake_metadata_file(
                &table.workspace.to_string(),
                &table.item.to_string(),
                &path,
                self.remaining(),
            ))
            .await;
        match result {
            Ok(bytes) => {
                self.usage.body_bytes_charged += bytes.len();
                Ok(bytes)
            }
            Err(error) => {
                self.failed();
                Err(error)
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Source evidence unavailable: {0}")]
struct EvidenceUnavailable(&'static str);

fn unavailable(reason: &'static str) -> anyhow::Error {
    EvidenceUnavailable(reason).into()
}

#[derive(Default)]
struct Commits {
    versions: HashSet<u64>,
    checkpoint: Option<u64>,
    cursor: PageCursor,
}

impl Commits {
    fn page(&mut self, page: MetadataPage, table: &Table) -> Result<Option<String>> {
        let mut body = page.body;
        let rows = body["paths"]
            .as_array()
            .ok_or_else(|| unavailable("malformed_directory_page"))?;
        if rows.len() > 25 {
            return Err(unavailable("directory_row_limit_exceeded"));
        }
        let prefix = format!("{}/{}/", table.item, table.log_dir());
        for row in rows {
            let name = row["name"]
                .as_str()
                .and_then(|name| name.strip_prefix(&prefix))
                .filter(|name| !name.is_empty() && !name.contains('/'))
                .ok_or_else(|| unavailable("directory_scope_mismatch"))?;
            if let Some(version) = delta::commit_version(name) {
                if matches!(&row["isDirectory"], Value::Bool(true))
                    || row["isDirectory"] == "true"
                    || !self.versions.insert(version)
                {
                    return Err(unavailable("conflicting_commit_listing"));
                }
            } else if let Some(version) = delta::checkpoint_version(name) {
                self.checkpoint = Some(self.checkpoint.map_or(version, |old| old.max(version)));
            }
        }
        if let Some(header) = page.continuation {
            if body
                .get("continuationToken")
                .is_some_and(|token| token.as_str() != Some(header.as_str()))
            {
                return Err(unavailable("conflicting_continuation"));
            }
            body["continuationToken"] = Value::from(header);
        }
        self.cursor.next(&body).map_err(unavailable)
    }
}

async fn delta_checks(client: &FabricClient, budget: &mut Budget, table: &Table) -> Vec<Check> {
    let mut commits = Commits::default();
    let mut token: Option<String> = None;
    let mut snapshot = delta::Snapshot::default();
    let result: Result<()> = async {
        loop {
            let page = budget.page(client, table, token.as_deref()).await?;
            token = commits.page(page, table)?;
            if token.is_none() {
                break;
            }
        }
        let mut versions: Vec<_> = commits.versions.iter().copied().collect();
        versions.sort_unstable_by(|a, b| b.cmp(a));
        let latest = versions
            .first()
            .copied()
            .ok_or_else(|| unavailable("no_json_commits_or_checkpoint_only"))?;
        if commits
            .checkpoint
            .is_some_and(|checkpoint| checkpoint > latest)
        {
            return Err(unavailable("checkpoint_newer_than_json"));
        }
        let mut expected = None;
        for version in versions {
            if expected.is_some_and(|next| next != version) {
                return Err(unavailable("non_contiguous_commit_suffix"));
            }
            let bytes = budget.file(client, table, version).await?;
            snapshot.push(version, &bytes).map_err(unavailable)?;
            if snapshot.complete() {
                return Ok(());
            }
            expected = version.checked_sub(1);
        }
        Err(unavailable("metadata_or_protocol_unavailable"))
    }
    .await;
    match result {
        Ok(()) => {
            let mut checks = vec![Check::new(
                "delta_coverage",
                Status::Ready,
                "Metadata/protocol evidence was found in a bounded contiguous JSON-commit suffix, not an atomic current snapshot or proof of loaded data.",
                snapshot.coverage(),
                table.inspect(),
            )];
            checks.push(snapshot.protocol(table.inspect()));
            checks.push(snapshot.schema(table.inspect()));
            checks
        }
        Err(error) => vec![error.downcast_ref::<EvidenceUnavailable>().map_or_else(||
                Check::error("delta_coverage",&error,"bounded OneLake Delta metadata",table.inspect()),|reason|
                Check::new(
                    "delta_coverage",
                    Status::Unknown,
                    "Bounded Delta evidence was incomplete; no compatibility or absence conclusion was inferred.",
                    json!({"reason":reason.0}),
                    table.inspect(),
                )),
        ],
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Observation {
    source_index: usize,
    workspace_id: String,
    item_id: String,
    schema_qualified: bool,
    checks: Vec<Check>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Report {
    pub status: Status,
    observed_at: String,
    partial: bool,
    scope: Value,
    usage: Usage,
    checks: Vec<Check>,
    sources: Vec<Observation>,
    not_checked: Vec<&'static str>,
}

pub(super) fn preview(tables: &[Table], limits: &Limits, mode: Mode) -> Value {
    let unique: HashSet<_> = tables
        .iter()
        .map(|table| (table.workspace, table.item))
        .collect();
    let bounds = limits.bounds();
    json!({"mutates":false,"creationMode":mode,"sourceCount":tables.len(),"uniqueSourceItems":unique.len(),"bounds":bounds,
        "maxLogicalReads":2*unique.len()+usize::from(bounds.page_limit)+usize::from(bounds.log_limit),
        "readsTableRows":false,"readsParquet":false,"status":"not_run"})
}

pub(super) async fn inspect(
    client: &FabricClient,
    tables: &[Table],
    limits: &Limits,
    mode: Mode,
) -> Report {
    let mut scope = preview(tables, limits, mode);
    scope["status"] = json!("observed");
    let bounds = limits.bounds();
    let mut budget = Budget {
        reads: ReadBudget::new(bounds.timeout_seconds),
        bounds,
        usage: Usage {
            pages: 0,
            log_files: 0,
            body_bytes_charged: 0,
            byte_usage_exact: true,
        },
    };
    let mut names: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, table) in tables.iter().enumerate() {
        names
            .entry(table.name.to_ascii_lowercase())
            .or_default()
            .push(index);
    }
    let collisions: Vec<_> = names
        .into_values()
        .filter(|indexes| indexes.len() > 1)
        .collect();
    let checks=vec![Check{key:"inferred_name_collisions",status:if collisions.is_empty(){Status::Ready}else{Status::NeedsAction},message:"Different source tables with the same basename may collide during model inference. No table is dropped or renamed automatically.".into(),evidence:json!({"sourceIndexGroups":collisions}),next_steps:Vec::new()}];
    let mut cache: HashMap<(Uuid, Uuid), std::result::Result<Metadata, Value>> = HashMap::new();
    let mut sources = Vec::new();
    for (index, table) in tables.iter().enumerate() {
        let key = (table.workspace, table.item);
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(key) {
            entry.insert(metadata(client, &budget.reads, table).await);
        }
        let mut found = Vec::new();
        match cache.get(&key).expect("cached metadata") {
            Ok(meta)=>{
                found.push(Check::new("source_metadata",Status::Ready,"The selected source item metadata is visible; this does not establish row access for other identities.",json!({"itemType":meta.kind}),table.inspect()));
                let status=match (mode,meta.kind,meta.layout) {
                    (Mode::Native,"Lakehouse",Some(false))=>Status::NeedsAction,
                    (_,"Lakehouse"|"Warehouse",Some(_))=>Status::Ready,
                    _=>Status::Unknown,
                };
                found.push(Check::new("source_layout",status,"Schema-less native creation has a known dbo-assumption compatibility risk; generated mode is a separate explicit choice. Unrecognized layouts stay unknown, never cause an automatic mode change.",json!({"schemaEnabled":meta.layout,"nativeSchemaLessRisk":mode==Mode::Native && meta.layout==Some(false)}),table.inspect()));
                found.extend(delta_checks(client,&mut budget,table).await);
            },
            Err(error)=>found.push(Check::new("source_metadata",Status::Unknown,"Source metadata could not be established; storage probes were skipped, not interpreted as absence.",json!({"error":error}),table.inspect())),
        }
        sources.push(Observation {
            source_index: index,
            workspace_id: table.workspace.to_string(),
            item_id: table.item.to_string(),
            schema_qualified: table.schema_qualified,
            checks: found,
        });
    }
    let all = || {
        checks
            .iter()
            .chain(sources.iter().flat_map(|source| source.checks.iter()))
    };
    let status = summary(all());
    let partial = all().any(|check| check.status == Status::Unknown);
    Report {
        status,
        observed_at: chrono::Utc::now().to_rfc3339(),
        partial,
        scope,
        usage: budget.usage,
        checks,
        sources,
        not_checked: vec![
            "Parquet/table rows, row counts, statistics values and loaded data",
            "data freshness or an atomic snapshot",
            "native service acceptance, complete type mapping and precision",
            "target workspace permissions, capacity and cross-region alignment",
            "model relationships, framing and DAX readiness",
            "SQL security, storage/fallback mode and grants for other identities",
            "checkpoint/sidecar replay and unsupported Delta features",
            "deletion-vector files and row-mask correctness",
        ],
    }
}

pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    urls: &[String],
    limits: &Limits,
    mode: Mode,
    strict: bool,
) -> Result<()> {
    let tables = selected(urls)?;
    if cli.continuation_token.is_some() {
        return Err(FabioError::new(
            ErrorCode::InvalidInput,
            "Aggregate source checks cannot resume a list cursor",
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        "semantic-model check-source",
        &preview(&tables, limits, mode),
    ) {
        return Ok(());
    }
    let report = inspect(client, &tables, limits, mode).await;
    output::render_object(cli, &serde_json::to_value(&report)?, "status");
    if strict && report.status != Status::Ready {
        return Err(failed_gate());
    }
    Ok(())
}

pub(super) fn failed_gate() -> anyhow::Error {
    FabioError::with_typed_hint(ErrorCode::InvalidInput,"Source checks did not establish every requested observation; no model was created.","Inspect sourceCheck/status, bounds and notChecked. Resolve source findings or review an alternative explicitly; do not infer compatibility, drop checks or switch binding modes automatically.",HintType::SemanticCorrection).into()
}

/// A creation gate, not a bypass: every non-ready result stops before POST.
pub(super) async fn required(
    cli: &Cli,
    client: &FabricClient,
    tables: &[Table],
    limits: &Limits,
    mode: Mode,
) -> Result<Report> {
    if client.is_readonly() {
        return Err(FabioError::with_hint(ErrorCode::ReadonlyMode,"Model creation is blocked in readonly mode.","Run semantic-model check-source for read-only inspection, or preview creation with --dry-run.").into());
    }
    let report = inspect(client, tables, limits, mode).await;
    if report.status != Status::Ready {
        output::render_object(
            cli,
            &json!({"status":"not_created","sourceCheck":report}),
            "status",
        );
        return Err(failed_gate());
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    const WS: &str = "aaaaaaaa-1111-2222-3333-444444444444";
    const ITEM: &str = "bbbbbbbb-1111-2222-3333-444444444444";
    #[test]
    fn source_urls_are_canonical_and_cannot_redirect_or_normalize_scope() {
        let base = format!("https://onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables");
        let table = Table::parse(&format!("{base}/dbo/A%20B")).unwrap();
        assert_eq!(table.path, "Tables/dbo/A B");
        for suffix in [
            "/../Tables/T",
            "/dbo/../T",
            "/dbo/%2fT",
            "/dbo/%2e%2e",
            "/dbo/T?private=secret",
            "/dbo/T#secret",
        ] {
            assert!(Table::parse(&format!("{base}{suffix}")).is_err());
        }
        assert!(
            Table::parse(&format!(
                "https://private@onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/T"
            ))
            .is_err()
        );
        assert!(Table::parse(&format!("https://example.org/{WS}/{ITEM}/Tables/T")).is_err());
    }
}
