mod apply;
mod model;
mod state;

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, enrich_admin};
use crate::output;

use model::{Desired, Setting, VERSION, bytes_digest, invalid, parse};
use state::{Checkpoint, Locks, Phase};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Operation {
    Export,
    Plan,
    Apply,
}

impl Operation {
    const fn command(self) -> &'static str {
        match self {
            Self::Export => "admin export-tenant-settings",
            Self::Plan => "admin plan-tenant-settings",
            Self::Apply => "admin apply-tenant-settings",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn execute(
    cli: &Cli,
    client: &FabricClient,
    operation: Operation,
    file: Option<&str>,
    out: Option<&str>,
    resume: Option<&str>,
    continuation_file: Option<&str>,
) -> Result<()> {
    if cli.continuation_token.is_some() {
        return Err(invalid(
            "--continuation-token cannot be used for complete tenant snapshots; use --continue with a workflow checkpoint",
        ));
    }
    let mut cp = match resume {
        Some(path) => load_checkpoint(Path::new(path), operation)?,
        None => state::fresh(operation, file, out, continuation_file)?,
    };
    state::validate_paths(&cp, resume.is_none())?;
    if let Some(path) = &cp.next_page {
        page_path(&cp.endpoint, path)?;
    }
    if cli.dry_run {
        let mut preview = cp.summary();
        preview["settings"] = serde_json::json!(
            cp.plan
                .as_ref()
                .map(|p| p
                    .updates
                    .iter()
                    .map(|u| &u.setting_name)
                    .collect::<Vec<_>>())
                .unwrap_or_default()
        );
        output::dry_run_guard(cli, operation.command(), &preview);
        return Ok(());
    }
    if operation == Operation::Apply && client.is_readonly() {
        return Err(FabioError::with_hint(
            ErrorCode::ReadonlyMode,
            "Applying tenant settings is blocked in readonly mode",
            "Use 'admin plan-tenant-settings' or '--dry-run' to review without changing settings.",
        )
        .into());
    }
    let _locks = Locks::acquire(&cp)?;
    if resume.is_some() {
        let latest = load_checkpoint(&cp.checkpoint, operation)?;
        if latest.input_file != cp.input_file || latest.out != cp.out {
            return Err(invalid("Checkpoint paths changed while acquiring the lock"));
        }
        cp = latest;
    } else {
        state::validate_paths(&cp, true)?;
    }
    let tenant = client.tenant_identity().await?;
    let endpoint = client.fabric_endpoint();
    model::binding(VERSION, &tenant, &endpoint)?;
    if operation == Operation::Export && resume.is_none() {
        cp.tenant_id = tenant;
        cp.endpoint = endpoint;
    } else if !cp.tenant_id.eq_ignore_ascii_case(&tenant) || cp.endpoint != endpoint {
        return Err(invalid(
            "Authenticated tenant or API endpoint differs from the plan/checkpoint; select the original tenant and endpoint",
        ));
    }
    if cp.phase == Phase::Completed {
        verify_artifact(&cp)?;
        output::render_object(cli, &cp.summary(), "status");
        return Ok(());
    }
    if let Some(path) = &cp.next_page {
        page_path(&cp.endpoint, path)?;
    }
    if resume.is_some()
        && operation == Operation::Apply
        && cp
            .outcomes
            .iter()
            .any(|s| matches!(s, state::Outcome::Pending | state::Outcome::Uncertain))
        && (cp.phase == Phase::Apply
            || (cp.phase == Phase::Snapshot && cp.pages > 0 && cp.next_page.is_none()))
    {
        // Refresh pending/uncertain entries after an interruption, never the
        // acknowledged prefix. A partially read snapshot retains its cursor.
        cp.phase = Phase::Snapshot;
        cp.snapshot.clear();
        cp.next_page = None;
        cp.visited_pages.clear();
        cp.pages = 0;
    }
    cp.save()?;
    match run(client, &mut cp).await {
        Ok(()) => {
            output::render_object(cli, &cp.summary(), "status");
            Ok(())
        }
        Err(error) => Err(failure(&mut cp, error)),
    }
}

fn failure(cp: &mut Checkpoint, error: anyhow::Error) -> anyhow::Error {
    let error = enrich_admin(error, cp.operation.command());
    let mut error = match error.downcast::<FabioError>() {
        Ok(error) => error,
        Err(error) => FabioError::api_error(error.to_string()),
    };
    cp.retry_after.clone_from(&error.retry_after);
    cp.retry_at.clone_from(&error.retry_at);
    if let Err(save_error) = cp.save() {
        error.message = format!(
            "{}; {save_error}. The last durable checkpoint may require reconciliation.",
            error.message
        );
    }
    let resume_hint = format!(
        "Resume: fabio {} --continue \"{}\"",
        cp.operation.command(),
        cp.checkpoint.display()
    );
    if error.code == ErrorCode::RateLimited {
        error.hint = Some(error.retry_at.as_ref().map_or_else(
            || "No automatic retry was attempted. Wait for the API quota to recover before continuing.".to_owned(),
            |at| format!("No automatic retry was attempted. Retry-After indicates {at}; wait before continuing."),
        ));
    }
    error.hint = Some(match error.hint {
        Some(hint) => format!("{hint}\n{resume_hint}"),
        None => resume_hint,
    });
    error.set_progress(cp.summary()).into()
}

fn load_checkpoint(path: &Path, operation: Operation) -> Result<Checkpoint> {
    let path = state::resolved(path)?;
    let cp: Checkpoint = parse(&state::read(&path)?)?;
    cp.validate(operation, &path)?;
    Ok(cp)
}

async fn run(client: &FabricClient, cp: &mut Checkpoint) -> Result<()> {
    let empty_apply = cp.operation == Operation::Apply
        && !cp
            .outcomes
            .iter()
            .any(|s| matches!(s, state::Outcome::Pending | state::Outcome::Uncertain));
    if cp.phase == Phase::Snapshot && !empty_apply {
        snapshot(client, cp).await?;
    }
    match cp.operation {
        Operation::Export => {
            if cp.phase == Phase::Snapshot {
                cp.snapshot
                    .sort_by(|a, b| a.setting_name.cmp(&b.setting_name));
                cp.phase = Phase::Finalize;
                cp.save()?;
            }
            let desired = Desired {
                schema_version: VERSION,
                kind: "tenant-settings".into(),
                tenant_id: cp.tenant_id.clone(),
                endpoint: cp.endpoint.clone(),
                settings: cp.snapshot.clone(),
            };
            publish(cp, serde_yaml::to_string(&desired)?.as_bytes())?;
        }
        Operation::Plan => {
            if cp.phase == Phase::Snapshot {
                cp.plan = Some(model::build_plan(
                    cp.desired
                        .as_ref()
                        .ok_or_else(|| invalid("Missing desired state"))?,
                    &cp.snapshot,
                    cp.input_digest
                        .clone()
                        .ok_or_else(|| invalid("Missing source digest"))?,
                )?);
                cp.phase = Phase::Finalize;
                cp.save()?;
            }
            let plan = cp
                .plan
                .as_ref()
                .ok_or_else(|| invalid("Missing generated plan"))?;
            publish(cp, serde_yaml::to_string(plan)?.as_bytes())?;
        }
        Operation::Apply => {
            if !empty_apply {
                apply::run(client, cp).await?;
            }
        }
    }
    cp.phase = Phase::Completed;
    cp.retry_after = None;
    cp.retry_at = None;
    cp.save()
}

async fn snapshot(client: &FabricClient, cp: &mut Checkpoint) -> Result<()> {
    while cp.pages == 0 || cp.next_page.is_some() {
        let path = page_path(
            &cp.endpoint,
            cp.next_page.as_deref().unwrap_or("/admin/tenantsettings"),
        )?;
        if cp.visited_pages.contains(&path) {
            return Err(FabioError::api_error(
                "Tenant settings pagination would replay a completed page",
            )
            .into());
        }
        let response = client.get_once(&path).await?;
        let (settings, next) = decode_page(&cp.endpoint, &response)?;
        if next.as_deref() == Some(path.as_str()) {
            return Err(
                FabioError::api_error("Tenant settings pagination cursor did not advance").into(),
            );
        }
        let mut names: BTreeSet<_> = cp
            .snapshot
            .iter()
            .map(|s| s.setting_name.as_str())
            .collect();
        for setting in &settings {
            if !names.insert(&setting.setting_name) {
                return Err(FabioError::api_error(
                    "Duplicate tenant setting in paginated response",
                )
                .into());
            }
        }
        cp.snapshot.extend(settings);
        cp.next_page = next;
        cp.visited_pages.insert(path);
        cp.pages += 1;
        cp.retry_after = None;
        cp.retry_at = None;
        cp.save()?;
    }
    Ok(())
}

fn decode_page(endpoint: &str, response: &Value) -> Result<(Vec<Setting>, Option<String>)> {
    let settings = response
        .get("tenantSettings")
        .and_then(Value::as_array)
        .ok_or_else(|| FabioError::api_error("Expected tenantSettings array in list response"))?
        .iter()
        .cloned()
        .map(Setting::from_api)
        .collect::<Result<Vec<_>>>()?;
    let cursor = |key| -> Result<Option<&str>> {
        match response.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) if s.is_empty() => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            _ => Err(
                FabioError::api_error(format!("Invalid {key} in tenant settings response")).into(),
            ),
        }
    };
    let uri = cursor("continuationUri")?;
    let token = cursor("continuationToken")?;
    let next = match (uri, token) {
        (Some(uri), _) => Some(page_path(endpoint, uri)?),
        (None, Some(token)) => Some(format!(
            "/admin/tenantsettings?continuationToken={}",
            urlencoding::encode(token)
        )),
        (None, None) => None,
    };
    Ok((settings, next))
}

fn page_path(endpoint: &str, path: &str) -> Result<String> {
    let target = format!("{endpoint}/admin/tenantsettings");
    let expected = reqwest::Url::parse(&target)?;
    let url = if path.starts_with("/admin/tenantsettings") {
        reqwest::Url::parse(&format!("{endpoint}{path}"))
    } else {
        reqwest::Url::parse(path)
    }
    .map_err(|_| invalid("Invalid tenant settings continuation URI"))?;
    if url.origin() != expected.origin()
        || url.path() != expected.path()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid(
            "Continuation URI must target the same endpoint and tenant-settings list path",
        ));
    }
    Ok(url.query().map_or_else(
        || "/admin/tenantsettings".into(),
        |q| format!("/admin/tenantsettings?{q}"),
    ))
}

fn verify_artifact(cp: &Checkpoint) -> Result<()> {
    if let Some(out) = &cp.out
        && cp.artifact_digest.as_deref() != Some(bytes_digest(&state::read(out)?).as_str())
    {
        return Err(invalid("Completed output artifact is missing or changed"));
    }
    Ok(())
}

fn publish(cp: &mut Checkpoint, bytes: &[u8]) -> Result<()> {
    let out = cp
        .out
        .clone()
        .ok_or_else(|| invalid("Missing output path"))?;
    let expected = bytes_digest(bytes);
    if out.exists() {
        if cp.artifact_digest.as_deref() != Some(expected.as_str())
            || bytes_digest(&state::read(&out)?) != expected
        {
            return Err(invalid(
                "Refusing to overwrite an unrelated existing output artifact",
            ));
        }
        return Ok(());
    }
    cp.artifact_digest = Some(expected);
    cp.save()?;
    state::atomic_write(&out, bytes, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_continuation_scope_and_encoding() {
        let endpoint = "https://api.fabric.microsoft.com/v1";
        for uri in [
            "https://evil.example/v1/admin/tenantsettings",
            "https://api.fabric.microsoft.com/v1/admin/workspaces",
            "https://api.fabric.microsoft.com:444/v1/admin/tenantsettings",
            "//evil.example/admin/tenantsettings",
            "/admin/tenantsettings/../workspaces",
        ] {
            assert!(page_path(endpoint, uri).is_err(), "{uri}");
        }
        let (_, next) = decode_page(
            endpoint,
            &serde_json::json!({"tenantSettings":[],"continuationToken":"a&b +/"}),
        )
        .unwrap();
        assert_eq!(
            next.as_deref(),
            Some("/admin/tenantsettings?continuationToken=a%26b%20%2B%2F")
        );
    }

    #[test]
    fn test_malformed_pages_fail_closed() {
        for value in [
            serde_json::json!({}),
            serde_json::json!({"tenantSettings":{}}),
            serde_json::json!({"tenantSettings":[],"continuationToken":3}),
            serde_json::json!({"tenantSettings":[{"settingName":"A"}]}),
        ] {
            assert!(decode_page("https://api.fabric.microsoft.com/v1", &value).is_err());
        }
    }
}
