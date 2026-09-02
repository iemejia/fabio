use anyhow::Result;
use clap::Subcommand;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum LroCommand {
    /// Get the state of a long-running operation (add --follow to watch to completion)
    #[command(display_order = 1)]
    GetState {
        /// Operation ID
        #[arg(long)]
        operation_id: String,

        /// Continuously poll the operation, streaming NDJSON until it reaches a
        /// terminal state (Succeeded/Failed/Canceled/Undefined) or `--max-duration`
        /// / Ctrl-C. Watches an async operation to completion; always terminates.
        #[arg(long)]
        follow: bool,

        /// Seconds between polls in `--follow` mode (default 5)
        #[arg(long)]
        interval: Option<u64>,

        /// Total seconds to follow before stopping — the agent-safety bound (default 60)
        #[arg(long)]
        max_duration: Option<u64>,
    },
    /// Get the result of a completed long-running operation
    #[command(display_order = 2)]
    GetResult {
        /// Operation ID
        #[arg(long)]
        operation_id: String,
    },
}

pub async fn execute(cli: &Cli, client: &FabricClient, command: &LroCommand) -> Result<()> {
    match command {
        LroCommand::GetState {
            operation_id,
            follow,
            interval,
            max_duration,
        } => {
            Box::pin(get_state(
                cli,
                client,
                operation_id,
                *follow,
                *interval,
                *max_duration,
            ))
            .await
        }
        LroCommand::GetResult { operation_id } => get_result(cli, client, operation_id).await,
    }
}

async fn get_state(
    cli: &Cli,
    client: &FabricClient,
    operation_id: &str,
    follow: bool,
    interval: Option<u64>,
    max_duration: Option<u64>,
) -> Result<()> {
    let path = format!("/operations/{operation_id}");

    if !follow {
        if interval.is_some() || max_duration.is_some() {
            return Err(crate::errors::FabioError::with_hint(
                crate::errors::ErrorCode::InvalidInput,
                "--interval and --max-duration require --follow".to_string(),
                "Add --follow to watch the operation until it completes.".to_string(),
            )
            .into());
        }
        let data = client.get(&path).await?;
        output::render_object(cli, &data, "status");
        return Ok(());
    }

    let follow_opts = crate::commands::follow::FollowOptions {
        interval,
        max_duration,
        dedup_column: None,
    };
    crate::commands::follow::follow_stream(
        cli,
        &follow_opts,
        async || {
            let data = client.get(&path).await?;
            Ok((vec![data], vec!["status".to_string()]))
        },
        |rows| {
            rows.first()
                .and_then(|r| r.get("status"))
                .and_then(serde_json::Value::as_str)
                .is_some_and(is_terminal_operation_status)
        },
    )
    .await
}

/// A Fabric long-running operation status is terminal when it is Succeeded,
/// Failed, Canceled, or Undefined (case-insensitive on the first letter).
fn is_terminal_operation_status(status: &str) -> bool {
    matches!(
        status,
        "Succeeded" | "succeeded" | "Failed" | "failed" | "Canceled" | "Cancelled" | "Undefined"
    )
}

async fn get_result(cli: &Cli, client: &FabricClient, operation_id: &str) -> Result<()> {
    let data = client
        .get(&format!("/operations/{operation_id}/result"))
        .await?;
    output::render_object(cli, &data, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_terminal_operation_status;

    #[test]
    fn terminal_operation_status_detection() {
        for s in [
            "Succeeded",
            "succeeded",
            "Failed",
            "failed",
            "Canceled",
            "Cancelled",
            "Undefined",
        ] {
            assert!(is_terminal_operation_status(s), "{s} should be terminal");
        }
        for s in ["Running", "NotStarted", "", "InProgress"] {
            assert!(
                !is_terminal_operation_status(s),
                "{s} should NOT be terminal"
            );
        }
    }
}
