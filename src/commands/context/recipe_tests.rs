//! Check authored snippets against clap, not a second hand-written flag list.
//! Shell-style quoting is parsed as data on every platform: nothing is expanded,
//! executed, authenticated, or read from a model/report file.

use clap::CommandFactory as _;
use serde_json::Value;

use crate::cli::Cli;

fn validate_command(cli: &clap::Command, text: &str) -> Result<usize, String> {
    let tokens = shlex::split(text).ok_or_else(|| "Unclosed shell quotation".to_string())?;
    if tokens.is_empty() {
        return Err("Empty command example".to_string());
    }
    let mut count = 0;
    for args in tokens.split(|token| token == "&&") {
        if args.first().is_none_or(|token| token != "fabio") {
            return Err("Every invocation must begin with fabio".to_string());
        }
        if args
            .iter()
            .any(|token| matches!(token.as_str(), "|" | ";" | "||" | ">" | ">>"))
        {
            return Err("Only plain invocations and && chains are checked".to_string());
        }
        let matches = cli
            .clone()
            .try_get_matches_from(args)
            .map_err(|error| error.to_string())?;
        validate_safety(&matches)?;
        count += 1;
    }
    Ok(count)
}

fn validate_safety(matches: &clap::ArgMatches) -> Result<(), String> {
    if let Some(("dataflow", group)) = matches.subcommand()
        && let Some(("upgrade-gen1", command)) = group.subcommand()
    {
        if matches.get_flag("dry_run") {
            if !matches.get_flag("readonly") {
                return Err(
                    "Gen1 preview examples must retain both --readonly and --dry-run".to_string(),
                );
            }
        } else if !command.get_flag("allow_in_place_upgrade") || matches.get_flag("readonly") {
            return Err("Gen1 execution examples require --allow-in-place-upgrade and cannot execute under --readonly".to_string());
        }
    }
    Ok(())
}

fn validate_document(cli: &clap::Command, value: &Value, location: &str) -> usize {
    match value {
        Value::Object(object) => object
            .iter()
            .map(|(key, value)| {
                let location = format!("{location}.{key}");
                if matches!(
                    key.as_str(),
                    "command" | "commandTemplate" | "verifyAfter" | "example"
                ) && let Some(text) = value.as_str()
                {
                    validate_command(cli, text)
                        .unwrap_or_else(|error| panic!("{location}: {error}"))
                } else {
                    validate_document(cli, value, &location)
                }
            })
            .sum(),
        Value::Array(values) => values
            .iter()
            .enumerate()
            .map(|(index, value)| validate_document(cli, value, &format!("{location}[{index}]")))
            .sum(),
        _ => 0,
    }
}

fn with_cli(check: impl FnOnce(clap::Command) + Send + 'static) {
    // The full clap tree exceeds the default test-thread stack on Windows.
    std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(move || check(Cli::command()))
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn published_bi_recipes_parse_with_the_current_cli() {
    with_cli(|cli| {
        let mut documents = Vec::new();
        for (name, content) in super::workflows::entries() {
            if matches!(
                *name,
                "direct-lake-report"
                    | "report-authoring"
                    | "bi-readiness"
                    | "source-checked-creation"
            ) {
                documents.push((*name, *content));
            }
        }
        for (name, content) in super::schemas::entries() {
            if matches!(*name, "SemanticModel" | "Report") {
                documents.push((*name, *content));
            }
        }
        for (name, content) in super::examples::example_entries() {
            if matches!(
                *name,
                "semantic-model/create"
                    | "semantic-model/readiness"
                    | "semantic-model/refresh"
                    | "semantic-model/check-source"
            ) {
                documents.push((*name, *content));
            }
        }
        assert_eq!(
            documents.len(),
            10,
            "Keep the selected knowledge surfaces covered"
        );
        for (name, content) in documents {
            let document: Value = serde_json::from_str(content).unwrap();
            assert_ne!(
                validate_document(&cli, &document, name),
                0,
                "{name} contains no checked examples"
            );
        }
    });
}

#[test]
fn recipe_contract_rejects_the_original_repeated_file_bug_and_invalid_modes() {
    with_cli(|cli| {
        for invalid in [
            "fabio semantic-model create --workspace $WS --name Model --file model.tmdl --file table.tmdl",
            "fabio semantic-model create --workspace $WS --name Model --file model.tmdl --one-lake-table $URL",
            "fabio report create --workspace $WS --name Report --semantic-model $SM --dataset $SM",
            "fabio report create --workspace $WS --name Report",
            "fabio semantic-model create --workspace $WS --name 'Unclosed",
            "fabio workspace list &&",
            "fabio workspace list | other-program",
            "",
        ] {
            assert!(validate_command(&cli, invalid).is_err(), "{invalid}");
        }
        let chain = r#"fabio --wrap-untrusted semantic-model create --workspace $WS --name "Quoted model" && fabio --wrap-untrusted semantic-model query --workspace $WS --id $SM --dax 'EVALUATE ROW("rowCount", 2)'"#;
        assert_eq!(validate_command(&cli, chain).unwrap(), 2);
    });
}

#[test]
fn published_admin_connection_review_recipes_parse_with_the_current_cli() {
    with_cli(|cli| {
        let workflow: Value =
            serde_json::from_str(include_str!("data/workflows/admin_connection_review.json"))
                .unwrap();
        let example: Value =
            serde_json::from_str(include_str!("data/examples/admin_connection_hygiene.json"))
                .unwrap();
        assert_eq!(
            validate_document(&cli, &workflow, "admin-connection-review"),
            5
        );
        assert_eq!(
            validate_document(&cli, &example, "admin/connection-hygiene"),
            5
        );
    });
}

#[test]
fn published_gen1_upgrade_review_recipes_are_guarded_and_parse_with_the_cli() {
    with_cli(|cli| {
        let workflow: Value =
            serde_json::from_str(include_str!("data/workflows/gen1_upgrade_review.json")).unwrap();
        let example: Value =
            serde_json::from_str(include_str!("data/examples/dataflow_plan_upgrade.json")).unwrap();
        assert_eq!(validate_document(&cli, &workflow, "gen1-upgrade-review"), 4);
        assert_eq!(
            validate_document(&cli, &example, "dataflow/plan-upgrade"),
            3
        );
    });
}

#[test]
fn published_gen1_execution_and_observation_use_supported_scoped_syntax() {
    with_cli(|cli| {
        for (name, content, count) in [
            (
                "gen1-upgrade-execution",
                include_str!("data/workflows/gen1_upgrade_execution.json"),
                4,
            ),
            (
                "dataflow/upgrade-gen1",
                include_str!("data/examples/dataflow_upgrade_gen1.json"),
                1,
            ),
            (
                "dataflow/follow-upgrade",
                include_str!("data/examples/dataflow_follow_upgrade.json"),
                1,
            ),
        ] {
            assert_eq!(
                validate_document(&cli, &serde_json::from_str(content).unwrap(), name),
                count
            );
        }
    });
}

#[test]
fn gen1_example_safety_checks_reject_parseable_but_nonexecutable_invocations() {
    with_cli(|cli| {
        for invalid in [
            "fabio dataflow upgrade-gen1 --workspace $WS --id $DF1",
            "fabio --readonly dataflow upgrade-gen1 --workspace $WS --id $DF1",
            "fabio dataflow upgrade-gen1 --workspace $WS --id $DF1 --dry-run",
            "fabio --readonly dataflow upgrade-gen1 --workspace $WS --id $DF1 --allow-in-place-upgrade",
            "fabio dataflow upgrade-gen1 --workspace $WS --id=--allow-in-place-upgrade",
            "fabio --readonly --dry-run dataflow upgrade-gen1 --workspace $WS --id $DF1 && fabio dataflow upgrade-gen1 --workspace $WS --id $DF1",
        ] {
            // These are syntax-valid: the regression is in runtime guard semantics.
            for args in shlex::split(invalid).unwrap().split(|arg| arg == "&&") {
                cli.clone().try_get_matches_from(args).unwrap();
            }
            assert!(validate_command(&cli, invalid).is_err(), "{invalid}");
        }
        for valid in [
            "fabio --readonly --dry-run dataflow upgrade-gen1 --workspace $WS --id $DF1",
            "fabio dataflow upgrade-gen1 --workspace $WS --id $DF1 --readonly --dry-run",
            "fabio dataflow upgrade-gen1 --workspace $WS --id $DF1 --allow-in-place-upgrade",
            "fabio --readonly dataflow plan-upgrade --workspace $WS --id $DF1",
            "fabio --readonly context describe dataflow upgrade-gen1",
        ] {
            assert_eq!(validate_command(&cli, valid).unwrap(), 1);
        }
    });
}

#[test]
fn published_gen1_discovery_examples_parse_and_satisfy_runtime_guards() {
    with_cli(|cli| {
        let schema: Value = serde_json::from_str(include_str!("data/agent/commands.json")).unwrap();
        let examples = schema["dataflow"]["subcommands"]["upgrade-gen1"]["examples"]
            .as_array()
            .unwrap();
        assert_eq!(
            examples.len(),
            2,
            "Publish a guarded preview and explicitly approved execution example"
        );
        for (index, example) in examples.iter().enumerate() {
            validate_command(&cli, example.as_str().unwrap()).unwrap_or_else(|error| {
                panic!(
                    "dataflow upgrade-gen1 examples[{index}]: {error}. \
                     Regenerate commands.json from the authored workflow."
                );
            });
        }
    });
}
