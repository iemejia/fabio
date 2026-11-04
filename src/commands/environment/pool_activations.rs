use anyhow::Result;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::cli::{Cli, OutputFormat};
use crate::client::FabricClient;
use crate::commands::diagnostics::{error_evidence, id, uuid_target};
use crate::commands::follow::{FollowOptions, follow_stream};
use crate::commands::spark_monitoring::JobState;
use crate::errors::{ErrorCode, FabioError, enrich_forbidden};
use crate::output;

#[allow(clippy::too_many_arguments)]
pub(super) async fn list(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    page_size: Option<u32>,
    filter: Option<&str>,
    state: Option<JobState>,
) -> Result<()> {
    if state.is_some() && filter.is_some() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "--state conflicts with --filter.",
            "Choose a typed --state or one raw filter expression.",
        )
        .into());
    }
    let typed = state.map(|state| format!("state eq '{}'", state.as_str()));
    let filter = typed.as_deref().or(filter);
    let mut url = format!("/workspaces/{workspace}/environments/{environment}/poolActivations");
    let mut params = Vec::new();
    if let Some(size) = page_size {
        params.push(format!("pageSize={size}"));
    }
    if let Some(filter) = filter {
        params.push(format!("filter={}", urlencoding::encode(filter)));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    let response = client
        .get_list(&url, "value", cli.all, cli.continuation_token.as_deref())
        .await
        .map_err(|error| enrich_forbidden(error, "environment list-pool-activations", "Viewer"))?;
    output::render_list_with_token(
        cli,
        &response.items,
        &[
            "name",
            "state",
            "jobType",
            "startDateTime",
            "endDateTime",
            "jobInstanceId",
        ],
        &["NAME", "STATE", "JOB TYPE", "STARTED", "ENDED", "JOB ID"],
        "jobInstanceId",
        response.continuation_token.as_deref(),
    );
    Ok(())
}

pub(super) fn observation(body: &Value, activation: Uuid) -> Value {
    let matched = id(&body["jobInstanceId"]) == Some(activation);
    let state = matched.then(|| JobState::parse(&body["state"])).flatten();
    json!({"jobInstanceId":activation.to_string(),"scopeMatched":matched,"state":state,
        "runtimeVersion":if matched {crate::commands::spark_monitoring::version(&body["runtimeVersion"])} else {None},
        "resourceStatusSummary":if matched {resource_counts(&body["resourceStatusSummary"])} else {Value::Null}})
}

pub(super) fn resource_counts(body: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in [
        "starting",
        "ready",
        "acquired",
        "stopping",
        "deactivated",
        "expired",
        "cancelled",
        "failed",
    ] {
        result.insert(
            key.to_string(),
            body[key]
                .as_u64()
                .filter(|value| i32::try_from(*value).is_ok())
                .map_or(Value::Null, Value::from),
        );
    }
    Value::Object(result)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    environment: &str,
    activation: &str,
    follow: bool,
    interval: Option<u64>,
    max_duration: Option<u64>,
) -> Result<()> {
    let workspace = uuid_target(workspace, "--workspace")?;
    let environment = uuid_target(environment, "--id")?;
    let activation = uuid_target(activation, "--activation-id")?;
    let options = FollowOptions {
        interval,
        max_duration,
        dedup_column: None,
    };
    options.validate(follow)?;
    if follow && (cli.query.is_some() || !matches!(cli.effective_output(), OutputFormat::Json)) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Activation follow emits bounded NDJSON snapshots.",
            "Omit --query and use --output json with --follow.",
        )
        .into());
    }
    if output::dry_run_guard(
        cli,
        "environment show-pool-activation",
        &json!({"workspaceId":workspace.to_string(),"environmentId":environment.to_string(),"activationId":activation.to_string(),"follow":follow,"maxDurationSeconds":max_duration.unwrap_or(60)}),
    ) {
        return Ok(());
    }
    let path =
        format!("/workspaces/{workspace}/environments/{environment}/poolActivations/{activation}");
    if !follow {
        let body = client.get(&path).await.map_err(|error| {
            enrich_forbidden(error, "environment show-pool-activation", "Viewer")
        })?;
        output::render_object(cli, &body, "jobInstanceId");
        return Ok(());
    }
    follow_stream(cli,&options,async || {
        let row=match client.get(&path).await {
            Ok(body)=>observation(&body,activation),
            Err(error)=>json!({"jobInstanceId":activation.to_string(),"state":null,"scopeMatched":false,"error":error_evidence(&error)})
        };
        Ok((vec![row],vec!["state".to_string(),"resourceStatusSummary".to_string()]))
    },|rows|rows.first().is_some_and(|row|row["scopeMatched"]==true && JobState::parse(&row["state"]).is_some_and(JobState::terminal))).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_omits_names_configuration_and_unknown_state_values() {
        let activation = "cccccccc-1111-2222-3333-444444444444"
            .parse::<Uuid>()
            .unwrap();
        let body = json!({"jobInstanceId":activation.to_string(),"state":"FutureState","name":"private","sparkConfiguration":{"secret":"private"},"resourceStatusSummary":{"ready":2,"failed":-1}});
        let row = observation(&body, activation);
        assert_eq!(row["state"], Value::Null);
        assert_eq!(row["resourceStatusSummary"]["ready"], 2);
        assert_eq!(row["resourceStatusSummary"]["failed"], Value::Null);
        assert!(!row.to_string().contains("private"));
    }
}
