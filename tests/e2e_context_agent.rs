use assert_cmd::Command;

fn fabio() -> Command {
    Command::cargo_bin("fabio").unwrap()
}

#[test]
fn agent_context_returns_schema() {
    let assert = fabio()
        .args(["context", "agent", "--full"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    // Should have commands object
    assert!(json["data"]["commands"].is_object());
    let commands = json["data"]["commands"].as_object().unwrap();
    assert!(!commands.is_empty(), "context agent should list commands");
}

#[test]
fn agent_context_includes_workspace_command() {
    let assert = fabio()
        .args(["context", "agent", "--full"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let commands = json["data"]["commands"].as_object().unwrap();
    assert!(
        commands.contains_key("workspace"),
        "context agent should include 'workspace' command"
    );
}

#[test]
fn agent_context_output_table_format() {
    let assert = fabio()
        .args(["--output", "table", "context", "agent"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Table format should contain column headers or structured text
    assert!(!stdout.is_empty());
}

#[test]
fn agent_context_includes_app_backend_command_with_delete_flag() {
    let assert = fabio()
        .args(["context", "agent", "--full"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let commands = json["data"]["commands"].as_object().unwrap();

    let app_backend = commands
        .get("app-backend")
        .expect("context agent should include 'app-backend' command");
    let hard_delete_type = &app_backend["subcommands"]["delete"]["flags"]["--hard-delete"]["type"];
    assert_eq!(hard_delete_type, "bool");
}

// ── Phase 1: Hierarchical schema access ──────────────────────────────────────

#[test]
fn agent_context_default_returns_compact_index() {
    let assert = fabio().args(["context", "agent"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let commands = json["data"]["commands"].as_object().unwrap();
    // Should have all 74 groups
    assert!(commands.len() >= 70, "default should list all groups");
    // Each group should have description + subcommands array
    let lakehouse = &commands["lakehouse"];
    assert!(lakehouse["description"].is_string());
    assert!(lakehouse["subcommands"].is_array());
    let subs = lakehouse["subcommands"].as_array().unwrap();
    assert!(subs.len() > 30, "lakehouse should have 30+ subcommands");
    // Should NOT have full flag details (compact = names only)
    assert!(lakehouse.get("flags").is_none());
}

#[test]
fn agent_context_default_is_small() {
    let assert = fabio().args(["context", "agent"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // Default compact output should be much smaller than full dump
    assert!(
        stdout.len() < 26_000,
        "default output should be under 26KB, got {} bytes",
        stdout.len()
    );
}

#[test]
fn agent_context_group_returns_single_group() {
    let assert = fabio()
        .args(["context", "agent", "--group", "lakehouse"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["data"]["group"], "lakehouse");
    assert!(json["data"]["group_details"]["subcommands"].is_object());
    // Should include global_flags for context
    assert!(json["data"]["global_flags"].is_array());
    // Should include error_codes for context
    assert!(json["data"]["error_codes"].is_array());
}

#[test]
fn agent_context_group_case_insensitive() {
    let assert = fabio()
        .args(["context", "agent", "--group", "KQL-Database"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(json["data"]["group"], "kql-database");
}

#[test]
fn agent_context_group_invalid_shows_available() {
    let assert = fabio()
        .args(["context", "agent", "--group", "nonexistent"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json["data"]["error"]
            .as_str()
            .unwrap()
            .contains("nonexistent")
    );
    assert!(json["data"]["available_groups"].is_array());
    assert!(json["data"]["hint"].is_string());
}

#[test]
fn agent_context_group_much_smaller_than_full() {
    let full = fabio()
        .args(["context", "agent", "--full"])
        .assert()
        .success();
    let full_size = full.get_output().stdout.len();

    let group = fabio()
        .args(["context", "agent", "--group", "lakehouse"])
        .assert()
        .success();
    let group_size = group.get_output().stdout.len();

    // Group output should be at most 20% of full output
    assert!(
        group_size < full_size / 5,
        "group ({group_size}) should be <20% of full ({full_size})"
    );
}

// ── Describe subcommand ──────────────────────────────────────────────────────

#[test]
fn describe_returns_command_metadata() {
    let assert = fabio()
        .args(["context", "describe", "lakehouse", "sync"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["data"]["command"], "fabio lakehouse sync");
    assert!(json["data"]["description"].is_string());
    assert!(json["data"]["flags"].is_object());
    assert_eq!(json["data"]["mutates"], true);
    assert_eq!(json["data"]["returns"], "object");
}

#[test]
fn describe_includes_output_example_when_available() {
    let assert = fabio()
        .args(["context", "describe", "lakehouse", "sync"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // lakehouse/sync has an output example
    assert!(
        json["data"]["output_example"].is_object(),
        "describe should cross-reference the matching output example"
    );
    assert!(json["data"]["output_example"]["response"].is_object());
}

#[test]
fn describe_includes_auth_scope_from_group() {
    let assert = fabio()
        .args(["context", "describe", "lakehouse", "list"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    // auth_scope should be inherited from the group level
    assert!(json["data"]["auth_scope"].is_string());
}

#[test]
fn describe_invalid_group_shows_available() {
    let assert = fabio()
        .args(["context", "describe", "bogus", "list"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(json["data"]["error"].as_str().unwrap().contains("bogus"));
    assert!(json["data"]["available_groups"].is_array());
}

#[test]
fn describe_invalid_subcommand_shows_available() {
    let assert = fabio()
        .args(["context", "describe", "lakehouse", "nonexistent"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json["data"]["error"]
            .as_str()
            .unwrap()
            .contains("nonexistent")
    );
    assert!(json["data"]["available_subcommands"].is_array());
}

// ── Phase 2: Standard format emission ────────────────────────────────────────

#[test]
fn agent_format_mcp_emits_tools_array() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "mcp",
            "--group",
            "lakehouse",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let tools = json["data"]["tools"].as_array().unwrap();
    assert!(!tools.is_empty());

    // Each tool should have MCP-standard fields.
    let tool = &tools[0];
    assert!(tool["name"].is_string());
    assert!(tool["description"].is_string());
    assert!(tool["inputSchema"].is_object());
    assert_eq!(tool["inputSchema"]["type"], "object");
    assert!(tool["inputSchema"]["properties"].is_object());
    assert!(tool["annotations"].is_object());
}

#[test]
fn agent_format_mcp_tool_names_use_underscores() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "mcp",
            "--group",
            "kql-database",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let tools = json["data"]["tools"].as_array().unwrap();
    for tool in tools {
        let name = tool["name"].as_str().unwrap();
        assert!(
            !name.contains('-'),
            "MCP tool name should not contain hyphens: {name}"
        );
        assert!(
            name.starts_with("fabio_kql_database_"),
            "tool name should start with fabio_kql_database_: {name}"
        );
    }
}

#[test]
fn agent_format_mcp_marks_mutations() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "mcp",
            "--group",
            "lakehouse",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let tools = json["data"]["tools"].as_array().unwrap();
    // Find a read-only tool (list).
    let list_tool = tools
        .iter()
        .find(|t| t["name"] == "fabio_lakehouse_list")
        .expect("should have list tool");
    assert_eq!(list_tool["annotations"]["readOnlyHint"], true);

    // Find a mutating tool (create).
    let create_tool = tools
        .iter()
        .find(|t| t["name"] == "fabio_lakehouse_create")
        .expect("should have create tool");
    assert_eq!(create_tool["annotations"]["readOnlyHint"], false);
}

#[test]
fn agent_format_mcp_includes_required_params() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "mcp",
            "--group",
            "lakehouse",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let tools = json["data"]["tools"].as_array().unwrap();
    let sync_tool = tools
        .iter()
        .find(|t| t["name"] == "fabio_lakehouse_sync")
        .expect("should have sync tool");
    let required = sync_tool["inputSchema"]["required"].as_array().unwrap();
    assert!(
        !required.is_empty(),
        "sync tool should have required params"
    );
}

#[test]
fn agent_format_openai_emits_functions_array() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "openai",
            "--group",
            "workspace",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let functions = json["data"]["functions"].as_array().unwrap();
    assert!(!functions.is_empty());

    // Each function should have OpenAI-standard structure.
    let func = &functions[0];
    assert_eq!(func["type"], "function");
    assert!(func["function"]["name"].is_string());
    assert!(func["function"]["description"].is_string());
    assert!(func["function"]["parameters"].is_object());
    assert_eq!(
        func["function"]["parameters"]["additionalProperties"],
        false
    );
}

#[test]
fn agent_format_openai_includes_mutation_hint_in_description() {
    let assert = fabio()
        .args([
            "context",
            "agent",
            "--format",
            "openai",
            "--group",
            "workspace",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let functions = json["data"]["functions"].as_array().unwrap();
    // Find a read-only function.
    let list_fn = functions
        .iter()
        .find(|f| f["function"]["name"] == "fabio_workspace_list")
        .expect("should have list function");
    assert!(
        list_fn["function"]["description"]
            .as_str()
            .unwrap()
            .contains("[read-only")
    );

    // Find a mutating function.
    let create_fn = functions
        .iter()
        .find(|f| f["function"]["name"] == "fabio_workspace_create")
        .expect("should have create function");
    assert!(
        create_fn["function"]["description"]
            .as_str()
            .unwrap()
            .contains("[mutates")
    );
}

#[test]
fn agent_format_mcp_full_emits_all_tools() {
    let assert = fabio()
        .args(["context", "agent", "--format", "mcp"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let tools = json["data"]["tools"].as_array().unwrap();
    // Should have 800+ tools (all 807 subcommands)
    assert!(
        tools.len() >= 800,
        "full MCP output should have 800+ tools, got {}",
        tools.len()
    );
}

// ── Phase 4: context find ────────────────────────────────────────────────────

#[test]
fn find_returns_relevant_results() {
    let assert = fabio()
        .args(["context", "find", "upload file lakehouse"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "find should return results");
    // Top result should be lakehouse upload
    assert_eq!(results[0]["command"], "fabio lakehouse upload");
}

#[test]
fn find_returns_empty_for_nonsense() {
    let assert = fabio()
        .args(["context", "find", "xyzzy frobulate quantum"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(
        results.is_empty(),
        "nonsense query should return no results"
    );
    assert!(json["data"]["hint"].is_string());
}

#[test]
fn find_results_have_required_fields() {
    let assert = fabio()
        .args(["context", "find", "workspace create"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(!results.is_empty());
    let first = &results[0];
    assert!(first["command"].is_string());
    assert!(first["score"].is_number());
    assert!(first["description"].is_string());
}

// ── Phase 4b: context find — knowledge base search ───────────────────────────

#[test]
fn find_surfaces_best_practice_for_deploy_parameters() {
    let assert = fabio()
        .args(["context", "find", "environment variables deploy parameters"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "should find deploy-parameters topic");

    // Best-practice should be the top result
    let top = &results[0];
    assert_eq!(
        top["command"].as_str().unwrap(),
        "fabio context best-practices deploy-parameters",
        "deploy-parameters should be top result"
    );
    assert_eq!(top["type"].as_str().unwrap(), "best-practice");
    assert!(
        top["description"]
            .as_str()
            .unwrap()
            .contains("$ENV:VAR_NAME"),
        "description should mention $ENV:VAR_NAME"
    );
}

#[test]
fn find_surfaces_workflow_for_rti_pipeline() {
    let assert = fabio()
        .args([
            "context",
            "find",
            "real-time intelligence pipeline eventhouse",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "should find rti-pipeline workflow");

    // The RTI pipeline workflow should appear in results
    let has_rti = results.iter().any(|r| {
        r["command"]
            .as_str()
            .is_some_and(|c| c.contains("rti-pipeline"))
    });
    assert!(has_rti, "rti-pipeline workflow should be in results");

    let rti = results
        .iter()
        .find(|r| {
            r["command"]
                .as_str()
                .is_some_and(|c| c.contains("rti-pipeline"))
        })
        .unwrap();
    assert_eq!(rti["type"].as_str().unwrap(), "workflow");
}

#[test]
fn find_surfaces_throttling_best_practice() {
    let assert = fabio()
        .args(["context", "find", "throttling rate limit retry"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(!results.is_empty(), "should find throttling topic");

    let has_throttling = results.iter().any(|r| {
        r["command"]
            .as_str()
            .is_some_and(|c| c.contains("throttling"))
    });
    assert!(
        has_throttling,
        "throttling best-practice should be in results: {results:?}"
    );
}

#[test]
fn find_surfaces_cicd_deploy_workflow() {
    let assert = fabio()
        .args(["context", "find", "CI CD deployment convergence"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    let has_cicd = results.iter().any(|r| {
        r["command"]
            .as_str()
            .is_some_and(|c| c.contains("cicd-deploy"))
    });
    assert!(
        has_cicd,
        "cicd-deploy workflow should appear for CI/CD query: {results:?}"
    );
}

#[test]
fn find_knowledge_results_have_type_field() {
    let assert = fabio()
        .args(["context", "find", "shortcuts ADLS connection"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();

    // Filter to knowledge results (non-command)
    let knowledge_results: Vec<_> = results.iter().filter(|r| r.get("type").is_some()).collect();
    assert!(
        !knowledge_results.is_empty(),
        "should have knowledge-type results for shortcuts query"
    );

    for result in &knowledge_results {
        let result_type = result["type"].as_str().unwrap();
        assert!(
            matches!(
                result_type,
                "best-practice"
                    | "workflow"
                    | "persona"
                    | "disambiguation"
                    | "item-schema"
                    | "output-example"
            ),
            "unexpected knowledge type: {result_type}"
        );
        // Knowledge results should have command pointing to context subcommand
        let cmd = result["command"].as_str().unwrap();
        assert!(
            cmd.starts_with("fabio context "),
            "knowledge result command should start with 'fabio context': {cmd}"
        );
    }
}

#[test]
fn find_command_results_lack_type_field() {
    // Command results should NOT have a 'type' field (distinguishes from knowledge)
    let assert = fabio()
        .args(["context", "find", "workspace create"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    let command_results: Vec<_> = results
        .iter()
        .filter(|r| {
            // Command results are real CLI invocations, not `fabio context <helper>`
            // knowledge pointers (best-practices, workflows, schemas, examples, ...).
            r["command"]
                .as_str()
                .is_some_and(|c| !c.starts_with("fabio context "))
        })
        .collect();

    assert!(!command_results.is_empty());
    for result in &command_results {
        assert!(
            result.get("type").is_none() || result["type"].is_null(),
            "command results should not have 'type' field: {result}"
        );
        // Command results have 'mutates' field instead
        assert!(
            result.get("mutates").is_some(),
            "command results should have 'mutates' field: {result}"
        );
    }
}

#[test]
fn find_mixed_results_commands_and_knowledge() {
    // A query that matches both commands and knowledge
    let assert = fabio()
        .args(["context", "find", "deploy plan apply"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    assert!(results.len() >= 2, "should have multiple results");

    // Should have at least one command result
    let has_command = results
        .iter()
        .any(|r| r.get("mutates").is_some() && r.get("type").is_none());
    assert!(has_command, "should include command results");

    // Should have at least one knowledge result (cicd-deploy or deploy-parameters)
    let has_knowledge = results.iter().any(|r| r.get("type").is_some());
    assert!(
        has_knowledge,
        "should include knowledge results for deploy query"
    );
}

#[test]
fn find_content_search_matches_inside_json() {
    // "$ENV:VAR_NAME" is only in the deploy-parameters content, not in its name
    let assert = fabio()
        .args(["context", "find", "$ENV:VAR_NAME secrets"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    let results = json["data"]["results"].as_array().unwrap();
    let has_deploy_params = results.iter().any(|r| {
        r["command"]
            .as_str()
            .is_some_and(|c| c.contains("deploy-parameters"))
    });
    assert!(
        has_deploy_params,
        "content search should find deploy-parameters via $ENV: {results:?}"
    );
}

// ── Phase 5: context persona (orchestrator layer) ────────────────────────────

#[test]
fn persona_returns_delegation_table() {
    let assert = fabio()
        .args(["context", "persona", "data-engineer"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["data"]["name"].as_str().unwrap(), "data-engineer");
    assert!(
        json["data"]["delegates_to"].is_array(),
        "persona should expose a delegates_to routing table"
    );
    assert!(
        !json["data"]["delegates_to"].as_array().unwrap().is_empty(),
        "delegation table should not be empty"
    );
}

#[test]
fn persona_migration_engineer_references_migration_workflows() {
    let assert = fabio()
        .args(["context", "persona", "migration-engineer"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("synapse-migration") && stdout.contains("databricks-migration"),
        "migration-engineer persona should route to migration workflows"
    );
}

#[test]
fn persona_unknown_returns_available_list() {
    let assert = fabio()
        .args(["context", "persona", "nonexistent-role"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json["data"]["available_personas"].is_array(),
        "unknown persona should list available personas"
    );
}

#[test]
fn find_surfaces_persona() {
    let assert = fabio()
        .args(["context", "find", "migration engineer synapse databricks"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let results = json["data"]["results"].as_array().unwrap();
    let has_persona = results
        .iter()
        .any(|r| r["type"].as_str() == Some("persona"));
    assert!(has_persona, "find should surface personas: {results:?}");
}

// ── Phase 5: context disambiguate (terminology routing) ──────────────────────

#[test]
fn disambiguate_materialized_view_lists_three_meanings() {
    let assert = fabio()
        .args(["context", "disambiguate", "materialized view"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();

    assert_eq!(json["data"]["term"].as_str().unwrap(), "materialized-view");
    let meanings = json["data"]["meanings"].as_array().unwrap();
    assert!(
        meanings.len() >= 3,
        "materialized view should have at least 3 workload meanings"
    );
    // Each meaning must name the command group that handles it.
    for m in meanings {
        assert!(
            m["command_group"].is_string(),
            "each meaning must map to a command_group"
        );
    }
}

#[test]
fn disambiguate_normalizes_spaces_and_hyphens() {
    // "sql endpoint" (space) and "sql-endpoint" (hyphen) resolve to the same table.
    let by_space = fabio()
        .args(["context", "disambiguate", "sql endpoint"])
        .assert()
        .success();
    let by_hyphen = fabio()
        .args(["context", "disambiguate", "sql-endpoint"])
        .assert()
        .success();
    let s1 = String::from_utf8_lossy(&by_space.get_output().stdout);
    let s2 = String::from_utf8_lossy(&by_hyphen.get_output().stdout);
    let j1: serde_json::Value = serde_json::from_str(&s1).unwrap();
    let j2: serde_json::Value = serde_json::from_str(&s2).unwrap();
    assert_eq!(j1["data"]["term"], j2["data"]["term"]);
}

#[test]
fn disambiguate_unknown_returns_available_terms() {
    let assert = fabio()
        .args(["context", "disambiguate", "totally-unknown-term"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json["data"]["available_terms"].is_array(),
        "unknown term should list available disambiguation terms"
    );
}

// ── context list includes the new topic types ────────────────────────────────

#[test]
fn list_includes_personas_and_disambiguations() {
    let assert = fabio().args(["context", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert!(
        json["data"]["personas"].is_array()
            && !json["data"]["personas"].as_array().unwrap().is_empty(),
        "context list should include personas"
    );
    assert!(
        json["data"]["disambiguations"].is_array()
            && !json["data"]["disambiguations"]
                .as_array()
                .unwrap()
                .is_empty(),
        "context list should include disambiguations"
    );
}

#[test]
fn app_developer_persona_routes_app_groups() {
    let assert = fabio()
        .args(["context", "persona", "app-developer"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("user-data-function")
            && stdout.contains("graphql-api")
            && stdout.contains("data-agent"),
        "app-developer persona should route app/API/AI-app command groups"
    );
}

#[test]
fn context_list_has_full_persona_and_skill_coverage() {
    let assert = fabio().args(["context", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let personas = json["data"]["personas"].as_array().unwrap();
    // Six personas covering the major Fabric roles (incl. app-developer).
    assert!(
        personas.len() >= 6,
        "expected at least 6 personas, got {}",
        personas.len()
    );
    for expected in [
        "data-engineer",
        "app-developer",
        "bi-developer",
        "rti-engineer",
        "migration-engineer",
        "fabric-admin",
    ] {
        assert!(
            personas.iter().any(|p| p.as_str() == Some(expected)),
            "missing persona: {expected}"
        );
    }
}

#[test]
fn data_scientist_persona_routes_ml_groups() {
    let assert = fabio()
        .args(["context", "persona", "data-scientist"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(
        stdout.contains("ml-model") && stdout.contains("ml-experiment"),
        "data-scientist persona should route ML command groups"
    );
}

#[test]
fn disambiguate_mirroring_distinguishes_replication_from_shortcut() {
    let assert = fabio()
        .args(["context", "disambiguate", "mirroring"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let groups: Vec<&str> = json["data"]["meanings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["command_group"].as_str().unwrap())
        .collect();
    assert!(
        groups.contains(&"mirrored-database") && groups.contains(&"lakehouse"),
        "mirroring disambiguation should distinguish replication (mirrored-database) from shortcut (lakehouse)"
    );
}

#[test]
fn disambiguate_model_distinguishes_ml_from_semantic() {
    let assert = fabio()
        .args(["context", "disambiguate", "model"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let groups: Vec<&str> = json["data"]["meanings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["command_group"].as_str().unwrap())
        .collect();
    assert!(
        groups.contains(&"ml-model") && groups.contains(&"semantic-model"),
        "model disambiguation should distinguish ml-model from semantic-model"
    );
}

#[test]
fn geospatial_family_covers_map_group() {
    let assert = fabio().args(["context", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    // 13 intent-scoped sub-skill families now exist (full workload coverage).
    // The geospatial family is generated on disk; confirm its skill file is discoverable.
    assert!(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(".agents/skills/fabio-geospatial/SKILL.md")
            .exists(),
        "fabio-geospatial sub-skill should be generated"
    );
    let _ = stdout;
}

#[test]
fn context_blueprint_returns_item_set_and_decisions() {
    let assert = fabio()
        .args(["context", "blueprint", "medallion"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let data = &json["data"];
    assert_eq!(data["name"], "medallion");
    assert!(
        data["item_set"].as_array().is_some_and(|a| !a.is_empty()),
        "blueprint must enumerate an item_set"
    );
    assert!(
        data["decisions"].as_array().is_some_and(|a| !a.is_empty()),
        "blueprint must surface key decisions"
    );
    // Every item in the set names a real fabio command group and a deployment phase.
    for item in data["item_set"].as_array().unwrap() {
        assert!(
            item["command_group"].is_string(),
            "item needs command_group"
        );
        assert!(item["phase"].is_string(), "item needs deployment phase");
    }
}

#[test]
fn context_blueprint_invalid_lists_available() {
    let assert = fabio()
        .args(["context", "blueprint", "does-not-exist"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let available = json["data"]["available_blueprints"].as_array().unwrap();
    let names: Vec<&str> = available.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"medallion") && names.contains(&"event-analytics"),
        "invalid blueprint should enumerate available blueprints"
    );
}

#[test]
fn context_list_includes_blueprints() {
    let assert = fabio().args(["context", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let blueprints = json["data"]["blueprints"].as_array().unwrap();
    let names: Vec<&str> = blueprints.iter().filter_map(|v| v.as_str()).collect();
    assert!(
        names.contains(&"app-backend") && names.contains(&"lambda"),
        "context list should expose the blueprints layer"
    );
}

#[test]
fn solution_architect_persona_routes_problem_to_blueprint() {
    let assert = fabio()
        .args(["context", "persona", "data-solution-architect"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let routes = json["data"]["delegates_to"].as_array().unwrap();
    let blueprints: Vec<&str> = routes
        .iter()
        .filter_map(|r| r["blueprint"].as_str())
        .collect();
    assert!(
        blueprints.contains(&"medallion")
            && blueprints.contains(&"event-analytics")
            && blueprints.contains(&"app-backend"),
        "architect persona must route problems to the blueprints"
    );
}

#[test]
fn context_blueprint_specialized_ml_has_experiment_and_model_items() {
    let assert = fabio()
        .args(["context", "blueprint", "basic-machine-learning-models"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let types: Vec<&str> = json["data"]["item_set"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|i| i["type"].as_str())
        .collect();
    assert!(
        types.contains(&"MLExperiment") && types.contains(&"MLModel"),
        "ML blueprint must include MLExperiment + MLModel; got {types:?}"
    );
}

#[test]
fn context_blueprints_cover_all_twelve_shapes() {
    let assert = fabio().args(["context", "list"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let names: Vec<&str> = json["data"]["blueprints"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    for expected in [
        "medallion",
        "lambda",
        "event-analytics",
        "event-medallion",
        "basic-data-analytics",
        "data-analytics-sql-endpoint",
        "basic-machine-learning-models",
        "sensitive-data-insights",
        "conversational-analytics",
        "app-backend",
        "translytical",
        "semantic-governance",
    ] {
        assert!(
            names.contains(&expected),
            "blueprint '{expected}' should be registered; got {names:?}"
        );
    }
}

#[test]
fn item_capabilities_full_matrix_has_rows_and_legend() {
    let assert = fabio()
        .args(["context", "item-capabilities"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let data = &json["data"];
    let rows = data["item_capabilities"].as_array().unwrap();
    assert!(rows.len() >= 30, "expected the full item-type universe");
    assert!(data["legend"].is_object(), "matrix must carry a legend");
    for r in rows {
        assert!(r["type"].is_string());
        assert!(r["creatable"].is_boolean());
        assert!(r["supports_definition"].is_boolean());
        assert!(matches!(
            r["deploy_strategy"].as_str(),
            Some("content" | "platform_only" | "unsupported")
        ));
        assert!(r["deployable_from_definition"].is_boolean());
    }
}

#[test]
fn item_capabilities_single_type_two_axes() {
    let nb = fabio()
        .args(["context", "item-capabilities", "Notebook"])
        .assert()
        .success();
    let nbj: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&nb.get_output().stdout)).unwrap();
    assert_eq!(nbj["data"]["deploy_strategy"], "content");
    assert_eq!(nbj["data"]["deployable_from_definition"], true);

    // Lakehouse: supports the definition API but deploys as a shell (platform_only).
    let lh = fabio()
        .args(["context", "item-capabilities", "lakehouse"])
        .assert()
        .success();
    let lhj: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&lh.get_output().stdout)).unwrap();
    assert_eq!(lhj["data"]["deploy_strategy"], "platform_only");
    assert_eq!(lhj["data"]["supports_definition"], true);
    assert_eq!(lhj["data"]["deployable_from_definition"], false);

    // Warehouse: no definition API at all.
    let wh = fabio()
        .args(["context", "item-capabilities", "Warehouse"])
        .assert()
        .success();
    let whj: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&wh.get_output().stdout)).unwrap();
    assert_eq!(whj["data"]["supports_definition"], false);
}

#[test]
fn item_capabilities_unknown_type_lists_available() {
    let assert = fabio()
        .args(["context", "item-capabilities", "NotAType"])
        .assert()
        .success();
    let json: serde_json::Value =
        serde_json::from_str(&String::from_utf8_lossy(&assert.get_output().stdout)).unwrap();
    let available = json["data"]["available_types"].as_array().unwrap();
    let names: Vec<&str> = available.iter().filter_map(|v| v.as_str()).collect();
    assert!(names.contains(&"Notebook") && names.contains(&"Lakehouse"));
}

#[test]
fn find_routes_streaming_problem_to_event_blueprint() {
    let assert = fabio()
        .args(["context", "find", "streaming telemetry real-time events"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    let results = json["data"]["results"].as_array().unwrap();
    let commands: Vec<&str> = results
        .iter()
        .filter_map(|r| r["command"].as_str())
        .collect();
    assert!(
        commands
            .iter()
            .any(|c| c.contains("blueprint event-analytics")),
        "find should surface the event-analytics blueprint for a streaming problem; got {commands:?}"
    );
}
