//! Ontology CRUD: list, show, create, update, delete.

use anyhow::Result;
use serde_json::Value;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError};
use crate::output;

use super::definitions::{build_definition_from_dir, build_definition_from_rdf};
use super::read_file_or_stdin;

pub(super) async fn list(cli: &Cli, client: &FabricClient, workspace: &str) -> Result<()> {
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace}/ontologies"),
            "value",
            cli.all,
            cli.continuation_token.as_deref(),
        )
        .await?;

    let has_labels = resp
        .items
        .iter()
        .any(|item| item.get("sensitivityLabel").is_some_and(|v| !v.is_null()));
    let has_tags = output::has_tags(&resp.items);

    let display_items;
    let items_ref: &[Value] = if has_tags {
        display_items = output::enrich_with_tags_display(&resp.items);
        &display_items
    } else {
        &resp.items
    };

    match (has_labels, has_tags) {
        (true, true) => output::render_list_with_token(
            cli,
            items_ref,
            &[
                "displayName",
                "id",
                "description",
                "sensitivityLabel.id",
                "_tagsDisplay",
            ],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (true, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "sensitivityLabel.id"],
            &["NAME", "ID", "DESCRIPTION", "SENSITIVITY LABEL"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, true) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description", "_tagsDisplay"],
            &["NAME", "ID", "DESCRIPTION", "TAGS"],
            "id",
            resp.continuation_token.as_deref(),
        ),
        (false, false) => output::render_list_with_token(
            cli,
            items_ref,
            &["displayName", "id", "description"],
            &["NAME", "ID", "DESCRIPTION"],
            "id",
            resp.continuation_token.as_deref(),
        ),
    }
    Ok(())
}

pub(super) async fn show(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
) -> Result<()> {
    let data = client
        .get(&format!("/workspaces/{workspace}/ontologies/{id}"))
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn create(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    name: &str,
    description: Option<&str>,
    definition_path: Option<&str>,
    file_path: Option<&str>,
    dir_path: Option<&str>,
    sensitivity_label: Option<&str>,
) -> Result<()> {
    let mut body = serde_json::json!({
        "displayName": name,
    });

    if let Some(desc) = description {
        body["description"] = Value::from(desc);
    }

    if let Some(path) = definition_path {
        let content = read_file_or_stdin(path)?;
        let def: Value = serde_json::from_str(&content)
            .map_err(|e| FabioError::with_hint(ErrorCode::InvalidInput, format!("Invalid definition JSON: {e}"), "Provide valid JSON. Inspect format: fabio ontology get-definition --workspace <WS> --id <ID> --decode"))?;
        body["definition"] = def;
    } else if let Some(path) = file_path {
        body["definition"] = build_definition_from_rdf(path)?;
    } else if let Some(path) = dir_path {
        body["definition"] = build_definition_from_dir(path)?;
    }

    if let Some(label_id) = sensitivity_label {
        body["sensitivityLabelSettings"] = serde_json::json!({
            "sensitivityLabelId": label_id
        });
    }

    let data = client
        .post(&format!("/workspaces/{workspace}/ontologies"), &body, true)
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn update(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<()> {
    if name.is_none() && description.is_none() {
        return Err(FabioError::with_hint(
            ErrorCode::InvalidInput,
            "Specify at least one of --name or --description to update",
            "Example: fabio ontology update --workspace <WS> --id <ID> --name \"New Name\"",
        )
        .into());
    }

    let mut body = serde_json::json!({});
    if let Some(n) = name {
        body["displayName"] = Value::from(n);
    }
    if let Some(d) = description {
        body["description"] = Value::from(d);
    }

    let data = client
        .patch(&format!("/workspaces/{workspace}/ontologies/{id}"), &body)
        .await?;

    output::render_object(cli, &data, "id");
    Ok(())
}

pub(super) async fn delete(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    id: &str,
    hard: bool,
) -> Result<()> {
    let path = if hard {
        format!("/workspaces/{workspace}/ontologies/{id}?hardDelete=True")
    } else {
        format!("/workspaces/{workspace}/ontologies/{id}")
    };

    client.delete(&path).await?;

    output::render_object(
        cli,
        &serde_json::json!({"id": id, "status": "deleted"}),
        "status",
    );
    Ok(())
}
