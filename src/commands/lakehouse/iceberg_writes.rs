//! Native Iceberg catalog writes, not a data/manifest writer or virtual Delta editor.
use std::collections::HashSet;
use std::io::Read as _;

use anyhow::Result;
use clap::Args;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

const MAX_REQUEST_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Args)]
pub struct Target {
    /// Workspace UUID, not a name
    #[arg(short, long, env = "FABIO_WORKSPACE")]
    workspace: String,
    /// Lakehouse/data item UUID, not a name
    #[arg(long)]
    id: String,
    /// Exact one-level catalog namespace
    #[arg(long, default_value = "dbo")]
    namespace: String,
    /// Exact table name, not a path or wildcard
    #[arg(long)]
    table: String,
}

#[derive(Debug, Args)]
pub struct RequestInput {
    /// IRC request JSON (or @file); no table rows or hand-authored metadata files
    #[arg(long, conflicts_with = "file", required_unless_present = "file")]
    content: Option<String>,
    /// File containing an IRC request (maximum 16 MiB)
    #[arg(long)]
    file: Option<String>,
}

#[derive(Debug, Args)]
pub struct CreateArgs {
    #[command(flatten)]
    target: Target,
    #[command(flatten)]
    input: RequestInput,
}

#[derive(Debug, Args)]
pub struct CommitArgs {
    #[command(flatten)]
    target: Target,
    #[command(flatten)]
    input: RequestInput,
}

#[derive(Debug, Args)]
pub struct DeleteArgs {
    #[command(flatten)]
    target: Target,
    /// UUID observed during review; this client snapshot check is NOT atomic with DELETE
    #[arg(long)]
    expected_table_uuid: String,
}

impl Target {
    fn validate(&self) -> Result<()> {
        validate_uuid(&self.workspace, "--workspace")?;
        validate_uuid(&self.id, "--id")?;
        for value in [&self.namespace, &self.table] {
            if value.trim().is_empty()
                || matches!(value.as_str(), "." | ".." | "*")
                || value.contains(['/', '\\', '%'])
                || value.chars().any(char::is_control)
            {
                return Err(FabioError::with_hint(ErrorCode::InvalidInput,
                    "Select one exact namespace/table name, not a root, wildcard, path or URI escape",
                    "Inspect iceberg-namespaces/iceberg-tables and use exact names. Native writes cannot broaden scope or follow shortcuts.").into());
            }
        }
        Ok(())
    }

    fn collection_path(&self, prefix: &str) -> String {
        format!(
            "iceberg/v1/{prefix}/namespaces/{}/tables",
            urlencoding::encode(&self.namespace)
        )
    }

    fn table_path(&self, prefix: &str) -> String {
        format!(
            "{}/{}",
            self.collection_path(prefix),
            urlencoding::encode(&self.table)
        )
    }

    fn scope(&self) -> Value {
        json!({"workspaceId":self.workspace,"itemId":self.id,"namespace":self.namespace,"table":self.table})
    }
}

impl RequestInput {
    fn read(&self) -> Result<Value> {
        let input = self
            .file
            .as_ref()
            .map(|file| format!("@{file}"))
            .or_else(|| self.content.clone())
            .ok_or_else(|| {
                FabioError::invalid_input("Provide --file or --content with an IRC request")
            })?;
        let text = if let Some(path) = input.strip_prefix('@') {
            let file = std::fs::File::open(path)
                .map_err(|_| FabioError::invalid_input("Could not open IRC request file"))?;
            std::io::read_to_string(file.take((MAX_REQUEST_BYTES + 1) as u64))
                .map_err(|_| FabioError::invalid_input("IRC request must be UTF-8 JSON"))?
        } else {
            input
        };
        if text.len() > MAX_REQUEST_BYTES {
            return Err(FabioError::invalid_input("IRC request exceeds 16 MiB").into());
        }
        let value:Value=serde_json::from_str(&text).map_err(|_|FabioError::with_hint(ErrorCode::InvalidInput,
            "Invalid IRC request JSON","Use an Iceberg CreateTableRequest or engine-generated CommitTableRequest, not table rows or a raw table metadata file."))?;
        if !value.is_object() {
            return Err(FabioError::invalid_input("IRC request must be an object").into());
        }
        Ok(value)
    }
}

fn create_request(target: &Target, mut request: Value) -> Result<Value> {
    if request
        .get("name")
        .is_some_and(|name| name.as_str() != Some(target.table.as_str()))
        || request.get("location").is_some()
        || request
            .get("stage-create")
            .is_some_and(|value| value.as_bool() != Some(false))
    {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput,"Create request changes the selected name/location or stages a table",
            "Use a matching --table/name, omit location and use stage-create:false or omit it. OneLake allocates the native table location; fabio does not stage or register external tables.").into());
    }
    let fields = request
        .pointer("/schema/fields")
        .and_then(Value::as_array)
        .filter(|fields| !fields.is_empty())
        .ok_or_else(|| {
            FabioError::invalid_input(
                "Create request requires schema.fields with at least one field",
            )
        })?;
    if request.pointer("/schema/type").and_then(Value::as_str) != Some("struct")
        || fields.iter().any(|field| {
            field["name"]
                .as_str()
                .is_none_or(|name| name.trim().is_empty())
                || field["id"].as_u64().is_none_or(|id| id == 0)
                || !(field["type"].as_str().is_some_and(|name| !name.is_empty())
                    || field["type"]
                        .get("type")
                        .and_then(Value::as_str)
                        .is_some_and(|name| !name.is_empty()))
        })
    {
        return Err(FabioError::invalid_input(
            "Use an Iceberg struct schema with named positive-ID typed fields",
        )
        .into());
    }
    let ids: HashSet<_> = fields
        .iter()
        .filter_map(|field| field["id"].as_u64())
        .collect();
    let names: HashSet<_> = fields
        .iter()
        .filter_map(|field| field["name"].as_str())
        .collect();
    if ids.len() != fields.len() || names.len() != fields.len() {
        return Err(
            FabioError::invalid_input("Root schema field IDs and names must be unique").into(),
        );
    }
    request["name"] = json!(target.table);
    Ok(request)
}

fn table_metadata(value: &Value) -> &Value {
    value.get("metadata").unwrap_or(value)
}

fn native_table_uuid(target: &Target, response: &Value) -> Result<String> {
    let metadata = table_metadata(response);
    if let Some(marker) = metadata.pointer("/properties/XTABLE_METADATA") {
        let source = marker
            .as_str()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or_else(|| marker.clone());
        if source["sourceTableFormat"]
            .as_str()
            .is_none_or(|format| !format.eq_ignore_ascii_case("Iceberg"))
        {
            return Err(native_scope_error());
        }
    }
    let uuid = metadata["table-uuid"]
        .as_str()
        .ok_or_else(native_scope_error)?;
    if uuid.len() != 36 || uuid::Uuid::parse_str(uuid).is_err() {
        return Err(native_scope_error());
    }
    let location = metadata["location"]
        .as_str()
        .ok_or_else(native_scope_error)?;
    if !location_matches(target, location) {
        return Err(native_scope_error());
    }
    Ok(uuid.to_string())
}

fn native_scope_error() -> anyhow::Error {
    FabioError::with_hint(ErrorCode::InvalidInput,"Catalog writes require a native Iceberg table physically inside the exact selected item/namespace/table",
        "Virtualized Delta metadata and shortcuts are not writable native targets. Inspect iceberg-table read-only, use exact names and UUID-based native locations, and use a Delta-compatible writer for Delta tables. Do not broaden paths or enable conversion automatically.").into()
}

fn location_matches(target: &Target, location: &str) -> bool {
    if location.len() > 16_384 {
        return false;
    }
    let Ok(url) = reqwest::Url::parse(location) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    if !(host.ends_with(".dfs.fabric.microsoft.com")
        || host.ends_with(".blob.fabric.microsoft.com"))
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.port().is_some_and(|port| port != 443)
    {
        return false;
    }
    let Some(segments) = url.path_segments() else {
        return false;
    };
    let mut parts = Vec::new();
    for segment in segments {
        let Ok(value) = urlencoding::decode(segment) else {
            return false;
        };
        parts.push(value.into_owned());
    }
    if parts.last().is_some_and(String::is_empty) {
        parts.pop();
    }
    let path = match url.scheme() {
        "abfs" | "abfss" if url.username().eq_ignore_ascii_case(&target.workspace) => {
            parts.as_slice()
        }
        "https"
            if url.username().is_empty()
                && parts
                    .first()
                    .is_some_and(|value| value.eq_ignore_ascii_case(&target.workspace)) =>
        {
            &parts[1..]
        }
        _ => return false,
    };
    if !path
        .first()
        .is_some_and(|value| value.eq_ignore_ascii_case(&target.id))
    {
        return false;
    }
    let expected = ["Tables", target.namespace.as_str(), target.table.as_str()];
    path.get(1..)
        .is_some_and(|tail| tail.iter().map(String::as_str).eq(expected))
        || target.namespace == "dbo"
            && path.get(1..).is_some_and(|tail| {
                tail.iter()
                    .map(String::as_str)
                    .eq(["Tables", target.table.as_str()])
            })
}

fn validate_commit<'a>(target: &Target, request: &'a Value) -> Result<&'a str> {
    if let Some(identifier) = request.get("identifier")
        && (identifier["name"].as_str() != Some(target.table.as_str())
            || identifier["namespace"].as_array().is_none_or(|parts| {
                parts.len() != 1 || parts[0].as_str() != Some(target.namespace.as_str())
            }))
    {
        return Err(FabioError::invalid_input(
            "Commit identifier must match the exact --namespace/--table scope",
        )
        .into());
    }
    let requirements = request["requirements"].as_array().ok_or_else(|| {
        FabioError::invalid_input("Commit requires requirements and updates arrays")
    })?;
    let assertions: Vec<_> = requirements
        .iter()
        .filter(|value| value["type"] == "assert-table-uuid")
        .collect();
    if assertions.len() != 1 {
        return Err(FabioError::with_hint(ErrorCode::Conflict,"Commit must assert the exact reviewed native table UUID",
            "Keep engine-generated optimistic requirements unchanged. Reconcile read-only and regenerate/review a scoped commit; never drop assertions or substitute a new UUID automatically.").set_retriable(Some(false)).into());
    }
    let uuid = assertions[0]["uuid"]
        .as_str()
        .ok_or_else(|| FabioError::invalid_input("Commit UUID assertion must be a string"))?;
    validate_uuid(uuid, "commit table UUID")?;
    let updates = request["updates"]
        .as_array()
        .filter(|updates| !updates.is_empty())
        .ok_or_else(|| FabioError::invalid_input("Commit requires a nonempty updates array"))?;
    if requirements
        .iter()
        .any(|value| value["type"].as_str().is_none_or(str::is_empty))
        || updates
            .iter()
            .any(|value| value["action"].as_str().is_none_or(str::is_empty))
    {
        return Err(FabioError::invalid_input(
            "Every requirement needs type and every update needs action",
        )
        .into());
    }
    if updates.iter().any(|value| {
        matches!(
            value["action"].as_str(),
            Some("assign-uuid" | "set-location")
        )
    }) {
        return Err(FabioError::invalid_input(
            "CLI commits cannot assign a different table UUID or relocate storage",
        )
        .into());
    }
    Ok(uuid)
}

fn ensure_table_uuid(actual: &str, expected: &str) -> Result<()> {
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(FabioError::with_hint(ErrorCode::Conflict,"Table UUID changed; no catalog write was attempted",
            "Inspect the table read-only and review the new identity. Keep commit assertions or --expected-table-uuid unchanged; never substitute a new UUID or broaden the target automatically.").set_retriable(Some(false)).into());
    }
    Ok(())
}

async fn prefix(client: &FabricClient, target: &Target) -> Result<String> {
    let config = client
        .get_onelake_table_api(&format!(
            "iceberg/v1/config?warehouse={}/{}",
            target.workspace, target.id
        ))
        .await?;
    let value = config
        .pointer("/overrides/prefix")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            FabioError::api_error("Catalog configuration did not return a scoped prefix")
        })?;
    if !value.eq_ignore_ascii_case(&format!("{}/{}", target.workspace, target.id)) {
        return Err(FabioError::with_hint(ErrorCode::ApiError,"Catalog prefix does not match the selected UUID scope",
            "Inspect iceberg-config; do not construct alternate prefixes or follow a returned URL automatically.").into());
    }
    Ok(value.to_string())
}

fn write_error(mut error: anyhow::Error) -> anyhow::Error {
    if let Some(fabio) = error.downcast_mut::<FabioError>()
        && fabio.code == ErrorCode::Conflict
    {
        fabio.hint=Some("Catalog conflict: inspect iceberg-table read-only, coordinate writers, and review a new scoped request. Never overwrite an existing table, discard optimistic assertions, replace expected UUIDs, rebase updates or replay automatically.".to_string());
        fabio.hint_type = Some(HintType::SemanticCorrection);
        fabio.retriable = Some(false);
    }
    error
}

async fn observe_write(
    client: &FabricClient,
    target: &Target,
    route: &str,
    acknowledgement: &Value,
    expected: Option<&str>,
) -> Value {
    let expected = expected.or_else(|| table_metadata(acknowledgement)["table-uuid"].as_str());
    let observed = match client.get_onelake_table_api(route).await {
        Ok(value) => native_table_uuid(target, &value).map(|uuid| (value, uuid)),
        Err(error) => Err(error),
    };
    match observed {
        Ok((value, uuid)) => {
            json!({"tableUuid":uuid,"scopeVerified":expected.is_none_or(|expected|expected.eq_ignore_ascii_case(&uuid)),
            "metadataLocation":value.get("metadata-location"),"metadataLocationMatchesAcknowledgement":acknowledgement.get("metadata-location").map(|location|value.get("metadata-location")==Some(location)),"verificationError":Value::Null})
        }
        Err(error) => {
            json!({"tableUuid":expected,"scopeVerified":false,"verificationError":error.downcast_ref::<FabioError>().map_or(ErrorCode::ApiError,|e|e.code).to_string()})
        }
    }
}

pub(super) async fn create(cli: &Cli, client: &FabricClient, args: &CreateArgs) -> Result<()> {
    let target = &args.target;
    target.validate()?;
    let request = create_request(target, args.input.read()?)?;
    let preview = json!({"scope":target.scope(),"request":request,"note":"Creates an empty native catalog table; no rows, manifests or data files are written by fabio."});
    if output::dry_run_guard(cli, "lakehouse iceberg-create-table", &preview) {
        return Ok(());
    }
    let prefix = prefix(client, target).await?;
    let acknowledgement = client
        .post_onelake_table_api(&target.collection_path(&prefix), &request)
        .await
        .map_err(write_error)?;
    let observation = observe_write(
        client,
        target,
        &target.table_path(&prefix),
        &acknowledgement,
        None,
    )
    .await;
    output::render_object(
        cli,
        &json!({"status":"acknowledged","scope":target.scope(),"observation":observation,"destructive":false,"retriable":false}),
        "status",
    );
    Ok(())
}

pub(super) async fn commit(cli: &Cli, client: &FabricClient, args: &CommitArgs) -> Result<()> {
    let target = &args.target;
    target.validate()?;
    let request = args.input.read()?;
    let expected = validate_commit(target, &request)?;
    let prefix = prefix(client, target).await?;
    let route = target.table_path(&prefix);
    let before = client.get_onelake_table_api(&route).await?;
    let uuid = native_table_uuid(target, &before)?;
    ensure_table_uuid(&uuid, expected)?;
    let preview = json!({"scope":target.scope(),"tableUuid":uuid,"request":request,
        "warnings":["Metadata commits can remove schemas, snapshots or references. Preserve and review every optimistic requirement; fabio never writes manifests or data files and never rebases a stale commit."]});
    if output::dry_run_guard(cli, "lakehouse iceberg-commit-table", &preview) {
        return Ok(());
    }
    let acknowledgement = client
        .post_onelake_table_api(&route, &request)
        .await
        .map_err(write_error)?;
    let observation = observe_write(client, target, &route, &acknowledgement, Some(&uuid)).await;
    output::render_object(
        cli,
        &json!({"status":"acknowledged","scope":target.scope(),"tableUuid":uuid,"observation":observation,"destructive":true,"retriable":false}),
        "status",
    );
    Ok(())
}

pub(super) async fn delete(cli: &Cli, client: &FabricClient, args: &DeleteArgs) -> Result<()> {
    let target = &args.target;
    target.validate()?;
    validate_uuid(&args.expected_table_uuid, "--expected-table-uuid")?;
    let prefix = prefix(client, target).await?;
    let route = target.table_path(&prefix);
    let before = client.get_onelake_table_api(&route).await?;
    let uuid = native_table_uuid(target, &before)?;
    ensure_table_uuid(&uuid, &args.expected_table_uuid)?;
    let preview = json!({"scope":target.scope(),"expectedTableUuid":uuid,"destructive":true,"atomicIdentityGuard":false,
        "warnings":["Catalog DELETE removes the table's OneLake directory, data and metadata. Quiesce writers and table recreation before deletion.","The UUID check is a client snapshot, NOT atomic with DELETE. Do not retry after uncertainty; physical data removal is not verified by a catalog read."]});
    if output::dry_run_guard(cli, "lakehouse iceberg-delete-table", &preview) {
        return Ok(());
    }
    client
        .delete_onelake_table_api(&route)
        .await
        .map_err(write_error)?;
    let (absent, error) = match client.get_onelake_table_api(&route).await {
        Ok(_) => (Some(false), None),
        Err(error)
            if error
                .downcast_ref::<FabioError>()
                .is_some_and(|e| e.code == ErrorCode::NotFound) =>
        {
            (Some(true), None)
        }
        Err(error) => (
            None,
            Some(
                error
                    .downcast_ref::<FabioError>()
                    .map_or(ErrorCode::ApiError, |e| e.code)
                    .to_string(),
            ),
        ),
    };
    output::render_object(
        cli,
        &json!({"status":"acknowledged","scope":target.scope(),"tableUuid":uuid,"catalogAbsent":absent,
        "verificationError":error,"physicalDataRemovalVerified":false,"atomicIdentityGuard":false,"destructive":true,"retriable":false}),
        "status",
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    const WS: &str = "00000000-0000-0000-0000-000000000001";
    const ITEM: &str = "00000000-0000-0000-0000-000000000002";
    const TABLE: &str = "00000000-0000-0000-0000-000000000003";
    fn fixture_target() -> Target {
        Target {
            workspace: WS.to_string(),
            id: ITEM.to_string(),
            namespace: "dbo".to_string(),
            table: "native".to_string(),
        }
    }
    fn metadata() -> Value {
        json!({"metadata":{"table-uuid":TABLE,"location":format!("abfss://{WS}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/native")}})
    }

    #[test]
    fn exact_scope_rejects_roots_paths_wildcards_and_uri_escapes() {
        let mut target = fixture_target();
        for invalid in ["", "*", ".", "..", "a/b", "a\\b", "%2F", "\n"] {
            target.table = invalid.to_string();
            assert!(target.validate().is_err());
        }
        let target = fixture_target();
        assert!(target.validate().is_ok());
        assert!(location_matches(
            &target,
            &format!("https://onelake.dfs.fabric.microsoft.com/{WS}/{ITEM}/Tables/dbo/native")
        ));
    }

    #[test]
    fn native_guard_rejects_shortcuts_virtual_delta_and_external_locations() {
        let target = fixture_target();
        let mut value = metadata();
        assert_eq!(native_table_uuid(&target, &value).unwrap(), TABLE);
        value["metadata"]["location"] = json!(format!(
            "abfss://{ITEM}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/native"
        ));
        assert!(native_table_uuid(&target, &value).is_err());
        value = metadata();
        value["metadata"]["location"] = json!(format!(
            "abfss://{WS}@onelake.dfs.fabric.microsoft.com/{ITEM}/Tables/dbo/source"
        ));
        assert!(native_table_uuid(&target, &value).is_err());
        value = metadata();
        value["metadata"]["properties"] =
            json!({"XTABLE_METADATA":"{\"sourceTableFormat\":\"Delta\"}"});
        assert!(native_table_uuid(&target, &value).is_err());
    }

    #[test]
    fn create_payload_never_moves_scope_or_stages_external_metadata() {
        let target = fixture_target();
        let body = json!({"schema":{"type":"struct","fields":[{"id":1,"name":"id","type":"int","required":false}]}});
        assert_eq!(
            create_request(&target, body.clone()).unwrap()["name"],
            "native"
        );
        for (key, value) in [
            ("name", json!("other")),
            ("location", json!("https://example.com")),
            ("stage-create", json!(true)),
        ] {
            let mut invalid = body.clone();
            invalid[key] = value;
            assert!(create_request(&target, invalid).is_err());
        }
    }

    #[test]
    fn commit_assertions_are_preserved_and_uuid_or_location_rewrites_are_rejected() {
        let mut request = json!({"requirements":[{"type":"assert-table-uuid","uuid":TABLE},{"type":"assert-current-schema-id","current-schema-id":0}],"updates":[{"action":"set-properties","updates":{"test":"value"}}]});
        let target = fixture_target();
        let before = request.clone();
        assert_eq!(validate_commit(&target, &request).unwrap(), TABLE);
        assert_eq!(request, before);
        assert!(ensure_table_uuid(TABLE, WS).is_err());
        request["updates"][0]["action"] = json!("set-location");
        assert!(validate_commit(&target, &request).is_err());
        request = before;
        request["identifier"] = json!({"namespace":["dbo"],"name":"other"});
        assert!(validate_commit(&target, &request).is_err());
    }

    #[test]
    fn request_input_supports_bounded_files_and_rejects_non_objects_or_invalid_utf8() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("request.json");
        std::fs::write(&file, b"{\"requirements\":[]}").unwrap();
        let input = RequestInput {
            file: Some(file.to_string_lossy().into_owned()),
            content: None,
        };
        assert_eq!(input.read().unwrap(), json!({"requirements":[]}));
        std::fs::write(&file, [0xff]).unwrap();
        assert!(input.read().is_err());
        std::fs::write(&file, vec![b' '; MAX_REQUEST_BYTES + 1]).unwrap();
        assert!(input.read().is_err());
        assert!(
            RequestInput {
                file: None,
                content: Some("[]".to_string())
            }
            .read()
            .is_err()
        );
    }
}
