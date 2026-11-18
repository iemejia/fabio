use anyhow::Result;
use serde_json::{Value, json};

use crate::cli::Cli;
use crate::client::{FabricClient, validate_uuid};
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

fn instances_path(workspace: &str, environment: &str) -> String {
    format!("/workspaces/{workspace}/environments/{environment}/jobs/runCustomLivePool/instances")
}

fn schedules_path(workspace: &str, environment: &str) -> String {
    format!("/workspaces/{workspace}/environments/{environment}/jobs/runCustomLivePool/schedules")
}

fn schedule_path(workspace: &str, environment: &str, schedule: &str) -> String {
    format!("{}/{schedule}", schedules_path(workspace, environment))
}

fn read_schedule_body(file: Option<&str>, content: Option<&str>, command: &str) -> Result<Value> {
    let text = match (file, content) {
        (Some(path), None) => std::fs::read_to_string(path)
            .map_err(|error| anyhow::anyhow!("Failed to read file '{path}': {error}"))?,
        (None, Some(value)) => value.to_string(),
        _ => {
            return Err(FabioError::with_hint(
                ErrorCode::InvalidInput,
                "Exactly one of --file or --content must be provided",
                format!(
                    "Example: fabio environment {command} --workspace <WS> --id <ID> \
                     --content '{{\"enabled\":true,\"configuration\":{{...}}}}'"
                ),
            )
            .into());
        }
    };
    let body: Value = serde_json::from_str(&text)?;
    if !body.is_object() || !body["enabled"].is_boolean() || !body["configuration"].is_object() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Schedule JSON must contain boolean 'enabled' and object 'configuration' fields",
            "Provide the BaseScheduleRequest shape from the Fabric API specification.",
        )
        .into());
    }
    Ok(body)
}

pub(super) async fn run(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    wait: bool,
    timeout_secs: u64,
    cancel_on_timeout: bool,
) -> Result<()> {
    validate_run_scope(workspace, environment)?;
    let path = instances_path(workspace, environment);
    crate::commands::item_job::run_and_wait(
        cli,
        client,
        workspace,
        environment,
        wait,
        timeout_secs,
        cancel_on_timeout,
        &crate::commands::item_job::RunSpec {
            op: "environment run-custom-live-pool",
            label: "Custom live-pool hydration",
            job_type: "runCustomLivePool",
            trigger_path: Some(&path),
            role: "Contributor",
        },
    )
    .await
}

fn validate_run_scope(workspace: &str, environment: &str) -> Result<()> {
    validate_uuid(workspace, "--workspace")?;
    validate_uuid(environment, "--id")
}

pub(super) async fn list_schedules(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
) -> Result<()> {
    let response = client
        .get_list(
            &schedules_path(workspace, environment),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "environment list-custom-live-pool-schedules",
                "Viewer",
            )
        })?;
    output::render_list_with_token(
        cli,
        &response.items,
        &["id", "enabled", "createdDateTime", "configuration.type"],
        &["ID", "ENABLED", "CREATED", "TYPE"],
        "id",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) async fn show_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    schedule: &str,
) -> Result<()> {
    let data = client
        .get(&schedule_path(workspace, environment, schedule))
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "environment show-custom-live-pool-schedule",
                "Viewer",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn create_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_schedule_body(file, content, "create-custom-live-pool-schedule")?;
    let preview = json!({"workspaceId": workspace, "environmentId": environment, "request": body});
    if output::dry_run_guard(
        cli,
        "environment create-custom-live-pool-schedule",
        &preview,
    ) {
        return Ok(());
    }

    let data = client
        .post(&schedules_path(workspace, environment), &body, false)
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "environment create-custom-live-pool-schedule",
                "Contributor",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn update_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    schedule: &str,
    file: Option<&str>,
    content: Option<&str>,
) -> Result<()> {
    let body = read_schedule_body(file, content, "update-custom-live-pool-schedule")?;
    let preview = json!({
        "workspaceId": workspace,
        "environmentId": environment,
        "scheduleId": schedule,
        "request": body
    });
    if output::dry_run_guard(
        cli,
        "environment update-custom-live-pool-schedule",
        &preview,
    ) {
        return Ok(());
    }

    let data = client
        .patch(&schedule_path(workspace, environment, schedule), &body)
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "environment update-custom-live-pool-schedule",
                "Contributor",
            )
        })?;
    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete_schedule(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    schedule: &str,
) -> Result<()> {
    let preview = json!({
        "workspaceId": workspace,
        "environmentId": environment,
        "scheduleId": schedule
    });
    if output::dry_run_guard_maybe_destructive(
        cli,
        "environment delete-custom-live-pool-schedule",
        &preview,
        true,
    ) {
        return Ok(());
    }

    client
        .delete(&schedule_path(workspace, environment, schedule))
        .await
        .map_err(|error| {
            enrich_forbidden(
                error,
                "environment delete-custom-live-pool-schedule",
                "Contributor",
            )
        })?;
    output::render_object(cli, &json!({"status": "deleted", "id": schedule}), "id");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hydration_scope_is_validated_before_preview_and_submission() {
        let valid = "00000000-0000-0000-0000-000000000001";
        assert!(validate_run_scope(valid, valid).is_ok());
        for invalid in ["", "../items", "id?jobType=Other"] {
            assert!(validate_run_scope(invalid, valid).is_err());
            assert!(validate_run_scope(valid, invalid).is_err());
        }
    }

    #[test]
    fn schedule_path_uses_background_job_route() {
        assert_eq!(
            schedule_path("workspace", "environment", "schedule"),
            "/workspaces/workspace/environments/environment/jobs/runCustomLivePool/schedules/schedule"
        );
    }

    #[test]
    fn schedule_body_preserves_all_configuration_fields() {
        let body = read_schedule_body(
            None,
            Some(
                r#"{"enabled":true,"configuration":{"type":"Cron","startDateTime":"2026-10-01T00:00:00Z","endDateTime":"2027-10-01T00:00:00Z","localTimeZoneId":"Central Standard Time","interval":1440}}"#,
            ),
            "create-custom-live-pool-schedule",
        )
        .unwrap();
        assert_eq!(body["enabled"], true);
        assert_eq!(body["configuration"]["type"], "Cron");
        assert_eq!(body["configuration"]["interval"], 1440);
    }

    #[test]
    fn schedule_body_requires_exactly_one_input() {
        assert!(read_schedule_body(None, None, "test").is_err());
        assert!(read_schedule_body(Some("a"), Some("{}"), "test").is_err());
    }

    #[test]
    fn schedule_body_requires_base_schedule_fields() {
        assert!(read_schedule_body(None, Some(r#"{"enabled":true}"#), "test").is_err());
        assert!(
            read_schedule_body(
                None,
                Some(r#"{"enabled":"yes","configuration":{}}"#),
                "test"
            )
            .is_err()
        );
    }

    #[test]
    fn schedule_file_and_inline_content_have_the_same_shape() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("schedule.json");
        let content = "{\r\n\"enabled\":false,\r\n\"configuration\":{\"type\":\"Cron\",\"interval\":1440}}\r\n";
        std::fs::write(&path, content).unwrap();
        let inline = read_schedule_body(None, Some(content), "test").unwrap();
        let file = read_schedule_body(Some(path.to_str().unwrap()), None, "test").unwrap();
        assert_eq!(file, inline);
        assert_eq!(file["enabled"], false);
    }

    #[test]
    fn schedule_body_rejects_malformed_and_nonobject_inputs() {
        for content in ["{", "[]", "null", r#"{"enabled":true,"configuration":[]}"#] {
            assert!(read_schedule_body(None, Some(content), "test").is_err());
        }
    }
}
