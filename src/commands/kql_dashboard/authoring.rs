//! Conservative offline scaffolding from the public KQL dashboard definition contract.
use crate::{cli::Cli, errors::FabioError, output};
use anyhow::Result;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde_json::{Value, json};

fn blank_definition(title: &str) -> Result<Value> {
    if title.trim().is_empty() || title.chars().count() > 256 || title.chars().any(char::is_control)
    {
        return Err(FabioError::invalid_input(
            "Dashboard title must contain 1-256 characters without control characters",
        )
        .into());
    }
    let dashboard = json!({
        "schema_version": "52", "title": title, "autoRefresh": {"enabled": false},
        "baseQueries": [], "tiles": [], "dataSources": [], "queries": [],
        "pages": [{"name": "Overview", "id": uuid::Uuid::new_v4().to_string()}],
        "parameters": [{"kind": "duration", "id": uuid::Uuid::new_v4().to_string(),
            "displayName": "Time range", "description": "", "beginVariableName": "_startTime", "endVariableName": "_endTime",
            "defaultValue": {"kind": "dynamic", "count": 1, "unit": "hours"}, "showOnPages": {"kind": "all"}}]
    });
    Ok(
        json!({"definition": {"parts": [{"path": "RealTimeDashboard.json", "payloadType": "InlineBase64",
        "payload": BASE64.encode(serde_json::to_vec(&dashboard)?)}]},
        "scaffoldOnly": true, "renderingVerified": false,
        "warnings": ["Blank schema-52 starter, not a fully authored or rendered dashboard. No queries run, data sources connected or Fabric resources created.",
            "Author Multi Stat/KPI tiles in the target UI or from a verified version-matched export; do not guess visualOptions or downgrade an existing dashboard. See context best-practices dashboard-authoring."]}),
    )
}

pub(super) fn scaffold(cli: &Cli, title: &str) -> Result<()> {
    output::render_object(cli, &blank_definition(title)?, "definition");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_blank_contract_has_local_ids_no_queries_and_canonical_part() {
        let output = blank_definition("Synthetic dashboard").unwrap();
        assert_eq!(output["scaffoldOnly"], true);
        assert_eq!(output["renderingVerified"], false);
        let part = &output["definition"]["parts"][0];
        assert_eq!(part["path"], "RealTimeDashboard.json");
        assert_eq!(part["payloadType"], "InlineBase64");
        let dashboard: Value =
            serde_json::from_slice(&BASE64.decode(part["payload"].as_str().unwrap()).unwrap())
                .unwrap();
        assert_eq!(dashboard["schema_version"], "52");
        assert_eq!(dashboard["title"], "Synthetic dashboard");
        for field in ["baseQueries", "tiles", "queries", "dataSources"] {
            assert_eq!(dashboard[field], json!([]));
        }
        assert!(uuid::Uuid::parse_str(dashboard["pages"][0]["id"].as_str().unwrap()).is_ok());
        assert!(uuid::Uuid::parse_str(dashboard["parameters"][0]["id"].as_str().unwrap()).is_ok());
        assert_eq!(dashboard["autoRefresh"]["enabled"], false);
    }

    #[test]
    fn title_validation_and_definition_transport_preserve_user_text_without_execution() {
        for title in ["", "  ", "x\ny", &"x".repeat(257)] {
            assert!(blank_definition(title).is_err());
        }
        let output = blank_definition("Quotes: \" and unicode é").unwrap();
        let body = crate::definition_spec::build_update_definition_body(
            &output.to_string(),
            "RealTimeDashboard.json",
        );
        assert_eq!(body["definition"], output["definition"]);
    }
}
