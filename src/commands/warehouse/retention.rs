use anyhow::Result;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::{execute_insights_query, execute_insights_statement};
/// Validate the warehouse data-retention window (Fabric allows 1–120 days).
fn validate_retention_days(days: u32) -> Result<()> {
    if !(1..=120).contains(&days) {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            format!("Retention must be between 1 and 120 days (got {days})"),
            "Example: fabio warehouse set-retention --id <WAREHOUSE> --days 30",
        )
        .into());
    }
    Ok(())
}

/// Build the `ALTER DATABASE` T-SQL that sets the time-travel retention period.
/// Pure function for testing.
fn build_set_retention_sql(days: u32) -> String {
    format!("ALTER DATABASE CURRENT SET TIME_TRAVEL_RETENTION_PERIOD = {days} DAYS;")
}

/// Report the configured data-retention (time-travel) period, in days.
pub(super) async fn get_retention(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let sql = "SELECT name, time_travel_retention_period_days \
               FROM sys.databases WHERE name = DB_NAME()";
    execute_insights_query(cli, client, workspace, id, sql).await
}

/// Configure the data-retention (time-travel) period, in days (1–120). This
/// controls the window for time travel, table clones, restore points, and
/// snapshots. DECREASING it is irreversible (older history is garbage-collected).
pub(super) async fn set_retention(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    days: u32,
) -> Result<()> {
    validate_retention_days(days)?;

    if output::dry_run_guard(
        cli,
        "warehouse set-retention",
        &serde_json::json!({ "id": id, "retentionDays": days }),
    ) {
        return Ok(());
    }

    let sql = build_set_retention_sql(days);
    execute_insights_statement(client, workspace, id, &sql).await?;

    let obj = serde_json::json!({ "id": id, "retentionDays": days, "status": "updated" });
    output::render_object(cli, &obj, "status");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_retention_sql_uses_alter_database() {
        assert_eq!(
            build_set_retention_sql(15),
            "ALTER DATABASE CURRENT SET TIME_TRAVEL_RETENTION_PERIOD = 15 DAYS;"
        );
    }

    #[test]
    fn retention_days_valid_range() {
        assert!(validate_retention_days(1).is_ok());
        assert!(validate_retention_days(30).is_ok());
        assert!(validate_retention_days(120).is_ok());
    }

    #[test]
    fn retention_days_rejects_out_of_range() {
        assert!(validate_retention_days(0).is_err());
        assert!(validate_retention_days(121).is_err());
    }
}
