//! Agent introspection, offline docs, and workspace graph extraction.

mod agent;
mod best_practices;
mod disambiguations;
mod examples;
mod personas;
mod schemas;
#[cfg(test)]
mod skillgen;
pub mod tenant;
mod workflows;

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::output;

// ── CLI definition ──────────────────────────────────────────────────────────

/// Output format for context graph.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ContextFormat {
    /// Native format: nodes/edges/workspaces/summary arrays (for fabio `--merge`, `JMESPath`, agents)
    #[default]
    Graph,
    /// JSON-LD instance data: actual items as RDF resources (for triple stores, SPARQL endpoints)
    Jsonld,
    /// OWL schema as JSON-LD: type definitions importable by `fabio ontology import --file`
    Owl,
    /// OWL schema as RDF/XML: type definitions importable by `fabio ontology import --file` and Ontology Playground
    Rdf,
    /// Full RDF/XML: schema + instances in one file (Ontology Playground + triple stores + Fabric import)
    Full,
}

/// Output format for the agent schema.
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum AgentFormat {
    /// Native fabio format: rich metadata with `auth_scope`, async, returns, destructive fields
    #[default]
    Native,
    /// MCP (Model Context Protocol) tool definitions — standard `JSON Schema` `inputSchema` per tool
    Mcp,
    /// `OpenAI` function-calling format — standard `JSON Schema` parameters per function
    Openai,
}

#[derive(Debug, Subcommand)]
pub enum ContextCommand {
    /// Machine-readable CLI schema for agent introspection (flags, types, mutability, examples)
    #[command(display_order = 0)]
    Agent {
        /// Return schema for a single command group only (e.g. `lakehouse`, `workspace`, `deploy`)
        #[arg(long)]
        group: Option<String>,

        /// Emit the full 14K-line schema dump (all commands, all flags, all metadata).
        /// Without this flag, returns a compact index of groups + subcommand names.
        #[arg(long)]
        full: bool,

        /// Schema output format: native (default), mcp (Model Context Protocol), openai (function calling)
        #[arg(long, value_enum, default_value = "native")]
        format: AgentFormat,

        /// Approximate token budget — returns the most compact useful subset that fits
        /// within this many tokens (4 chars/token estimate). Overrides --full.
        #[arg(long)]
        budget: Option<usize>,
    },

    /// Deep-dive on a single command: flags, examples, output shape, notes — everything to invoke it
    #[command(display_order = 1)]
    Describe {
        /// Command group (e.g. `lakehouse`, `workspace`, `deploy`)
        #[arg(name = "GROUP")]
        group: String,

        /// Subcommand (e.g. `sync`, `list-tables`, `plan`)
        #[arg(name = "COMMAND")]
        command: String,
    },

    /// Show the definition schema/template for a Fabric item type
    #[command(display_order = 2)]
    Schema {
        /// Item type (e.g. `Notebook`, `DataPipeline`, `SemanticModel`)
        #[arg(name = "TYPE")]
        item_type: String,
    },

    /// Show a multi-step workflow recipe
    #[command(display_order = 3)]
    Workflow {
        /// Workflow name (use `fabio context list` to see available workflows)
        #[arg(name = "NAME")]
        name: String,
    },

    /// Show best-practices guidance for a topic
    #[command(display_order = 4)]
    BestPractices {
        /// Topic name (`throttling`, `lro`, `pagination`, `admin-apis`)
        #[arg(name = "TOPIC")]
        topic: String,
    },

    /// Show an orchestrator persona: which command groups, workflows, and best-practices to use for a role
    #[command(display_order = 8)]
    Persona {
        /// Persona name (`data-engineer`, `migration-engineer`, `fabric-admin`, `rti-engineer`, `bi-developer`)
        #[arg(name = "NAME")]
        name: String,
    },

    /// Resolve an overloaded Fabric term to the concrete artifact + command group that handles it
    #[command(display_order = 9)]
    Disambiguate {
        /// Overloaded term (e.g. `materialized-view`, `dataflow`, `semantic-model`, `sql-endpoint`)
        #[arg(name = "TERM")]
        term: String,
    },

    /// Show example output for a command (response shape + `JMESPath` tips)
    #[command(display_order = 5)]
    Examples {
        /// Command group (e.g. `lakehouse`, `warehouse`, `deploy`)
        #[arg(name = "GROUP")]
        group: String,

        /// Subcommand (e.g. `query`, `list-tables`, `plan`). Omit to list all examples for the group.
        #[arg(name = "COMMAND")]
        command: Option<String>,
    },

    /// List all available documentation topics (schemas, workflows, examples, best-practices)
    #[command(display_order = 6)]
    List,

    /// Search commands by keyword (matches descriptions, flag names, and notes)
    #[command(display_order = 7)]
    Find {
        /// Search query (e.g. "upload file", "sync lakehouse", "create table")
        #[arg(name = "QUERY")]
        query: String,
    },

    /// Scan your Fabric tenant — build a relationship graph from workspace(s)
    #[command(display_order = 10)]
    Tenant {
        /// Workspace ID(s) or name(s) to scan (repeatable)
        #[arg(short, long, env = "FABIO_WORKSPACE", num_args = 1..)]
        workspace: Vec<String>,

        /// Fetch item definitions to discover embedded references (slower)
        #[arg(long)]
        deep: bool,

        /// Also fetch item connections
        #[arg(long)]
        include_connections: bool,

        /// Filter to specific item types (comma-separated, case-insensitive)
        #[arg(long)]
        item_types: Option<String>,

        /// Skip type-specific detail fetching (fast inventory-only mode)
        #[arg(long)]
        no_properties: bool,

        /// Output format:
        ///   graph (default) — native arrays for fabio merge/query;
        ///   jsonld — instance data as RDF for triple stores;
        ///   owl — OWL schema as JSON-LD for `fabio ontology import`;
        ///   rdf — OWL schema as RDF/XML for `fabio ontology import` and Ontology Playground
        #[arg(long, value_enum, default_value = "graph")]
        format: ContextFormat,

        /// Merge results into an existing graph file (incremental extraction)
        #[arg(long)]
        merge: Option<PathBuf>,

        /// Write output to a file instead of stdout
        #[arg(long, visible_alias = "out")]
        output_file: Option<PathBuf>,

        /// Max concurrency for API calls (default: auto-scaled to CPU count)
        #[arg(long)]
        concurrency: Option<usize>,

        /// Return only a lightweight inventory summary (item counts by type, workspace info)
        /// without building the full graph. Useful for agents to probe before a full scan.
        #[arg(long)]
        summary_only: bool,

        /// Fast name-to-ID resolution. Comma-separated `Type:Name` pairs
        /// (e.g. `Notebook:my-nb,Lakehouse:bronze`). Returns matching items without full graph.
        #[arg(long)]
        resolve: Option<String>,

        /// Focus on a single item: return only the subgraph within `--depth` hops of this item ID
        #[arg(long)]
        focus: Option<String>,

        /// Maximum hop distance from the focused item (default: 1). Requires `--focus`.
        #[arg(long, default_value = "1")]
        depth: usize,
    },
}

// ── Dispatch ────────────────────────────────────────────────────────────────

pub async fn execute(cli: &Cli, client: &FabricClient, command: &ContextCommand) -> Result<()> {
    match command {
        ContextCommand::Agent {
            group,
            full,
            format,
            budget,
        } => {
            agent::execute(cli, group.as_deref(), *full, *format, *budget);
            Ok(())
        }
        ContextCommand::Describe { group, command } => {
            agent::execute_describe(cli, group, command);
            Ok(())
        }
        ContextCommand::Schema { item_type } => {
            schemas::execute(cli, item_type);
            Ok(())
        }
        ContextCommand::Workflow { name } => {
            workflows::execute(cli, name);
            Ok(())
        }
        ContextCommand::BestPractices { topic } => {
            best_practices::execute(cli, topic);
            Ok(())
        }
        ContextCommand::Persona { name } => {
            personas::execute(cli, name);
            Ok(())
        }
        ContextCommand::Disambiguate { term } => {
            disambiguations::execute(cli, term);
            Ok(())
        }
        ContextCommand::Examples { group, command } => {
            examples::execute(cli, group, command.as_deref());
            Ok(())
        }
        ContextCommand::List => {
            list_topics(cli);
            Ok(())
        }
        ContextCommand::Find { query } => {
            agent::execute_find(cli, query);
            Ok(())
        }
        ContextCommand::Tenant {
            workspace,
            deep,
            include_connections,
            item_types,
            no_properties,
            format,
            merge,
            output_file,
            concurrency,
            summary_only,
            resolve,
            focus,
            depth,
        } => {
            let params = tenant::ExtractParams {
                workspaces: workspace,
                deep: *deep,
                include_connections: *include_connections,
                item_types_filter: item_types.as_deref(),
                no_properties: *no_properties,
                format: *format,
                merge: merge.as_deref(),
                output_file: output_file.as_deref(),
                concurrency: concurrency.unwrap_or_else(crate::parallel::default_concurrency),
                summary_only: *summary_only,
                resolve: resolve.as_deref(),
                focus: focus.as_deref(),
                depth: *depth,
            };
            tenant::execute(cli, client, &params).await
        }
    }
}

// ─── Shared helpers ──────────────────────────────────────────────────────────

fn list_topics(cli: &Cli) {
    let topics = json!({
        "item_schemas": schemas::list_names(),
        "workflows": workflows::list_names(),
        "output_examples": examples::list_names(),
        "best_practices": best_practices::list_names(),
        "personas": personas::list_names(),
        "disambiguations": disambiguations::list_names(),
        "usage": {
            "schema": "fabio context schema <TYPE>",
            "workflow": "fabio context workflow <NAME>",
            "examples": "fabio context examples <GROUP> <COMMAND>",
            "best_practices": "fabio context best-practices <TOPIC>",
            "persona": "fabio context persona <NAME>",
            "disambiguate": "fabio context disambiguate <TERM>"
        }
    });
    output::render_object(cli, &topics, "item_schemas");
}

/// Find an entry in a static lookup table using normalized key matching.
fn find_entry<'a>(entries: &[(&str, &'a str)], normalized_key: &str) -> Option<&'a str> {
    entries
        .iter()
        .find(|(name, _)| name.to_lowercase().replace(['-', '_'], "") == *normalized_key)
        .map(|(_, content)| *content)
}

/// Expose the commands schema for reuse by the MCP server.
pub fn agent_commands_schema() -> serde_json::Value {
    serde_json::from_str(include_str!("data/agent/commands.json"))
        .expect("commands.json must contain valid JSON")
}
