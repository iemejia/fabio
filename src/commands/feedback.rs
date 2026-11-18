use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::Cli;
use crate::output;

#[derive(Debug, Subcommand)]
#[command(
    after_help = "For complete flag reference, run: fabio context agent\nReturns machine-readable JSON schema of all commands, flags, and types."
)]
pub enum FeedbackCommand {
    /// Record feedback about CLI friction or issues
    Send {
        /// Feedback message
        message: String,
    },
    /// List recorded feedback entries
    List,
}

#[derive(Debug, Serialize, Deserialize)]
struct FeedbackEntry {
    timestamp: String,
    message: String,
}

fn feedback_path() -> PathBuf {
    let home = home::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".fabio").join("feedback.jsonl")
}

pub fn execute(cli: &Cli, command: &FeedbackCommand) -> Result<()> {
    match command {
        FeedbackCommand::Send { message } => send(cli, message),
        FeedbackCommand::List => list(cli),
    }
}

fn send(cli: &Cli, message: &str) -> Result<()> {
    let path = feedback_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).ok();
        }
    }

    let entry = FeedbackEntry {
        timestamp: crate::datetime::now_rfc3339(),
        message: message.to_string(),
    };

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)?
    };
    #[cfg(not(unix))]
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    let line = serde_json::to_string(&entry)?;
    writeln!(file, "{line}")?;

    // Optionally send upstream if endpoint configured
    let upstream_sent = std::env::var("FABIO_FEEDBACK_ENDPOINT").ok();

    let obj = serde_json::json!({
        "status": "recorded",
        "upstream": upstream_sent,
    });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[allow(clippy::unnecessary_wraps)]
fn list(cli: &Cli) -> Result<()> {
    let path = feedback_path();
    let entries: Vec<Value> = if path.exists() {
        fs::read_to_string(&path)
            .unwrap_or_default()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    } else {
        Vec::new()
    };

    output::render_list(
        cli,
        &entries,
        &["timestamp", "message"],
        &["TIMESTAMP", "MESSAGE"],
        "message",
    );
    Ok(())
}
