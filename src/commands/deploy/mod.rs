pub mod apply;
pub mod changeset;
pub mod config;
pub mod export;
pub mod folders;
pub mod git_diff;
pub mod init_params;
pub mod ordering;
pub mod params;
pub mod plan;
pub mod platform;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde_json::json;

use crate::cli::Cli;
use crate::client::FabricClient;
use crate::errors::{ErrorCode, FabioError, HintType};
use crate::output;

use self::changeset::ChangeAction;
use self::plan::resolve_workspace;
use self::platform::get_git_metadata;

/// Resolved config values from config file + CLI overrides.
struct ResolvedCliConfig {
    source: Option<PathBuf>,
    workspace: Option<String>,
    parameters: Option<PathBuf>,
}

/// Resolve configuration from an optional config file + CLI overrides.
/// CLI values always take precedence over config file values.
fn resolve_config_and_cli(
    config_path: Option<&Path>,
    env: Option<&str>,
    cli_source: Option<&Path>,
    cli_workspace: Option<&str>,
    cli_parameters: Option<&Path>,
) -> Result<ResolvedCliConfig> {
    if let Some(cfg_path) = config_path {
        let env_name =
            env.ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "--env is required when --config is specified", "Add --env <environment> to select which config environment. Example: fabio deploy plan --config config.yaml --env dev"))?;
        let cfg = config::parse_config(cfg_path)?;
        let resolved = config::resolve_config(&cfg, cfg_path, env_name)?;

        Ok(ResolvedCliConfig {
            source: cli_source.map(Path::to_path_buf).or(resolved.source),
            workspace: cli_workspace.map(str::to_owned).or(resolved.workspace),
            parameters: cli_parameters
                .map(Path::to_path_buf)
                .or(resolved.parameters),
        })
    } else {
        Ok(ResolvedCliConfig {
            source: cli_source.map(Path::to_path_buf),
            workspace: cli_workspace.map(str::to_owned),
            parameters: cli_parameters.map(Path::to_path_buf),
        })
    }
}

#[derive(Debug, Subcommand)]
#[command(
    after_help = "Before using this command, run: fabio context examples deploy\nAlso available: fabio context workflow cicd-deploy"
)]
pub enum DeployCommand {
    /// Preview what would be deployed (create/update/delete/skip)
    #[command(display_order = 1)]
    Plan {
        /// Source directory with .platform item definitions (local path or cloned git repo)
        #[arg(long)]
        source: Option<PathBuf>,

        /// Target workspace ID or name
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: Option<String>,

        /// Only deploy specific item types (comma-separated)
        #[arg(long, value_delimiter = ',')]
        item_types: Option<Vec<String>>,

        /// Include delete actions for items not in source
        #[arg(long)]
        delete_orphans: bool,

        /// Don't error on unresolved logical ID references
        #[arg(long)]
        allow_unresolved: bool,

        /// Skip content-hash comparison, show all items as needing update
        #[arg(long)]
        force_all: bool,

        /// Save plan to a file for later apply
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,

        /// Parameter file for environment-aware substitution (JSON)
        #[arg(long, value_name = "FILE")]
        parameters: Option<PathBuf>,

        /// Target environment name for parameter substitution (e.g., "prod", "staging")
        #[arg(long, value_name = "NAME")]
        env: Option<String>,

        /// Deploy config file (JSON or YAML) with per-environment settings
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// Only deploy items changed since this git ref (e.g., HEAD~1, main)
        #[arg(long, value_name = "REF")]
        git_diff: Option<String>,

        /// Exclude items whose display name matches this regex
        #[arg(long, value_name = "PATTERN")]
        exclude_regex: Option<String>,

        /// Only include specific items (format: "Name.Type", comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "ITEMS")]
        include_items: Option<Vec<String>>,

        /// Only include items in these folder paths (comma-separated, e.g., "/ETL,/Reports")
        #[arg(long, value_delimiter = ',', value_name = "PATHS")]
        include_folders: Option<Vec<String>>,

        /// Exclude items in these folder paths (comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "PATHS")]
        exclude_folders: Option<Vec<String>>,

        /// Item types permitted for deletion (comma-separated; protects Lakehouse, Warehouse, etc.)
        #[arg(long, value_delimiter = ',', value_name = "TYPES")]
        allow_delete_types: Option<Vec<String>>,

        /// Skip workspace folder management (create/move/delete folders)
        #[arg(long)]
        no_folders: bool,

        /// Skip automatic workspace ID replacement (00000000-... → target workspace)
        #[arg(long)]
        no_workspace_id_replace: bool,
    },

    /// Execute deployment (create/update/delete items)
    #[command(display_order = 2)]
    Apply {
        /// Source directory with .platform item definitions (local path or cloned git repo)
        #[arg(long, required_unless_present = "plan")]
        source: Option<PathBuf>,

        /// Target workspace ID or name
        #[arg(short, long, required_unless_present = "plan")]
        workspace: Option<String>,

        /// Apply a previously saved plan file
        #[arg(long, conflicts_with_all = ["source", "workspace"])]
        plan: Option<PathBuf>,

        /// Only deploy specific item types (comma-separated)
        #[arg(long, value_delimiter = ',')]
        item_types: Option<Vec<String>>,

        /// Delete items not in source
        #[arg(long)]
        delete_orphans: bool,

        /// Proceed despite unresolved references
        #[arg(long)]
        allow_unresolved: bool,

        /// Stop on first failure (default: continue remaining items)
        #[arg(long)]
        fail_fast: bool,

        /// Skip content-hash comparison, redeploy all items
        #[arg(long)]
        force_all: bool,

        /// Max parallel operations per type batch
        #[arg(long, default_value = "8")]
        concurrency: usize,

        /// Parameter file for environment-aware substitution (JSON)
        #[arg(long, value_name = "FILE")]
        parameters: Option<PathBuf>,

        /// Target environment name for parameter substitution (e.g., "prod", "staging")
        #[arg(long, value_name = "NAME")]
        env: Option<String>,

        /// Skip post-deploy hooks (semantic model refresh, environment publish)
        #[arg(long)]
        no_post_hooks: bool,

        /// Deploy config file (JSON or YAML) with per-environment settings
        #[arg(long, value_name = "FILE")]
        config: Option<PathBuf>,

        /// Only deploy items changed since this git ref (e.g., HEAD~1, main)
        #[arg(long, value_name = "REF")]
        git_diff: Option<String>,

        /// Exclude items whose display name matches this regex
        #[arg(long, value_name = "PATTERN")]
        exclude_regex: Option<String>,

        /// Only include specific items (format: "Name.Type", comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "ITEMS")]
        include_items: Option<Vec<String>>,

        /// Only include items in these folder paths (comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "PATHS")]
        include_folders: Option<Vec<String>>,

        /// Exclude items in these folder paths (comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "PATHS")]
        exclude_folders: Option<Vec<String>>,

        /// Item types permitted for deletion (comma-separated)
        #[arg(long, value_delimiter = ',', value_name = "TYPES")]
        allow_delete_types: Option<Vec<String>>,

        /// Skip workspace folder management
        #[arg(long)]
        no_folders: bool,

        /// Skip automatic workspace ID replacement
        #[arg(long)]
        no_workspace_id_replace: bool,

        /// Exclude shortcuts matching this regex during reconciliation
        #[arg(long, value_name = "PATTERN")]
        shortcut_exclude_regex: Option<String>,

        /// Run a pipeline or notebook after deployment completes (data orchestration)
        ///
        /// Triggers the named item's default job after all deploy operations succeed.
        /// Useful for populating lakehouses after deploying a solution with ETL items.
        #[arg(long, value_name = "NAME")]
        post_run_item: Option<String>,

        /// Deployment execution strategy
        ///
        /// - default: per-item create/update with bounded parallelism (content-hash skip)
        /// - bulk: batch creates/updates via Bulk Import Definitions API (faster for
        ///   large initial deploys; requires no Git integration on target workspace)
        /// - sequential: fully sequential execution (for debugging API issues)
        #[arg(long, value_parser = ["default", "bulk", "sequential"], default_value = "default")]
        strategy: String,

        /// After applying, re-fetch the applied items and confirm they converged to the
        /// source (content-hash for definition items, existence for platform-only items).
        /// Adds a `verification` block to the output. Report-only: never changes the exit code.
        #[arg(long)]
        verify: bool,
    },

    /// Export workspace item definitions to a local directory
    #[command(display_order = 3)]
    Export {
        /// Source workspace ID or name
        #[arg(short, long, env = "FABIO_WORKSPACE")]
        workspace: String,

        /// Output directory to write .platform items
        #[arg(long, value_name = "DIR")]
        dir: PathBuf,

        /// Only export specific item types (comma-separated)
        #[arg(long, value_delimiter = ',')]
        item_types: Option<Vec<String>>,

        /// Overwrite existing files in output directory
        #[arg(long)]
        overwrite: bool,

        /// Max parallel getDefinition requests
        #[arg(long, default_value = "8")]
        concurrency: usize,
    },

    /// Generate a parameters.json scaffold by scanning or diffing exported definitions
    #[command(display_order = 4, name = "init-params")]
    InitParams {
        /// Source directory containing exported .platform items (e.g., dev workspace)
        #[arg(long)]
        source: PathBuf,

        /// Comparison directory to diff against (e.g., prod workspace export)
        #[arg(long, value_name = "DIR")]
        compare: Option<PathBuf>,

        /// Environment name for the source directory (used in diff mode)
        #[arg(long, default_value = "dev")]
        source_env: String,

        /// Environment name for the comparison directory (used in diff mode)
        #[arg(long, default_value = "prod")]
        compare_env: String,

        /// Output file path for generated parameters.json
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,

        /// Auto-resolve connection GUIDs by looking up connections in the tenant
        #[arg(long)]
        resolve_connections: bool,
    },

    /// Validate source directory locally (no API calls). Checks .platform files,
    /// item types, duplicate names/logical IDs, cross-references, and parameters.
    #[command(display_order = 5)]
    Validate {
        /// Source directory containing Fabric item definitions with .platform files
        #[arg(long)]
        source: PathBuf,

        /// Parameter file to validate (JSON)
        #[arg(long, value_name = "FILE")]
        parameters: Option<PathBuf>,

        /// Environment name to validate parameter lookups against
        #[arg(long, value_name = "NAME")]
        env: Option<String>,
    },
}

#[allow(clippy::too_many_lines)]
pub async fn execute(cli: &Cli, client: &FabricClient, cmd: &DeployCommand) -> Result<()> {
    match cmd {
        DeployCommand::Plan {
            source,
            workspace,
            item_types,
            delete_orphans,
            allow_unresolved,
            force_all,
            out,
            parameters,
            env,
            config,
            git_diff,
            exclude_regex,
            include_items,
            include_folders,
            exclude_folders,
            allow_delete_types,
            no_folders,
            no_workspace_id_replace,
        } => {
            // Resolve config file if provided
            let resolved = resolve_config_and_cli(
                config.as_deref(),
                env.as_deref(),
                source.as_deref(),
                workspace.as_deref(),
                parameters.as_deref(),
            )?;

            let src = resolved
                .source
                .as_deref()
                .ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "--source is required (or set in config file)", "Provide --source <DIR> pointing to exported item definitions. Create one: fabio deploy export --workspace <WS> --dir ./export"))?;
            let ws = resolved.workspace.as_deref().ok_or_else(|| {
                FabioError::with_hint(
                    ErrorCode::InvalidInput,
                    "--workspace is required (or set in config file environments)",
                    "Provide --workspace <ID|NAME>. List workspaces: fabio workspace list",
                )
            })?;

            execute_plan(
                cli,
                client,
                src,
                ws,
                item_types.as_deref(),
                *delete_orphans,
                *allow_unresolved,
                *force_all,
                out.as_deref(),
                resolved.parameters.as_deref(),
                env.as_deref(),
                git_diff.as_deref(),
                exclude_regex.as_deref(),
                include_items.as_deref(),
                include_folders.as_deref(),
                exclude_folders.as_deref(),
                allow_delete_types.as_deref(),
                *no_folders,
                *no_workspace_id_replace,
            )
            .await
        }
        DeployCommand::Apply {
            source,
            workspace,
            plan,
            item_types,
            delete_orphans,
            allow_unresolved,
            fail_fast,
            force_all,
            concurrency,
            parameters,
            env,
            no_post_hooks,
            config,
            git_diff,
            exclude_regex,
            include_items,
            include_folders,
            exclude_folders,
            allow_delete_types,
            no_folders,
            no_workspace_id_replace,
            shortcut_exclude_regex,
            post_run_item,
            strategy,
            verify,
        } => {
            let resolved = resolve_config_and_cli(
                config.as_deref(),
                env.as_deref(),
                source.as_deref(),
                workspace.as_deref(),
                parameters.as_deref(),
            )?;

            execute_apply(
                cli,
                client,
                resolved.source.as_deref().or(source.as_deref()),
                resolved.workspace.as_deref().or(workspace.as_deref()),
                plan.as_deref(),
                item_types.as_deref(),
                *delete_orphans,
                *allow_unresolved,
                *fail_fast,
                cli.force,
                *force_all,
                *concurrency,
                resolved.parameters.as_deref(),
                env.as_deref(),
                *no_post_hooks,
                git_diff.as_deref(),
                exclude_regex.as_deref(),
                include_items.as_deref(),
                include_folders.as_deref(),
                exclude_folders.as_deref(),
                allow_delete_types.as_deref(),
                *no_folders,
                *no_workspace_id_replace,
                shortcut_exclude_regex.as_deref(),
                post_run_item.as_deref(),
                strategy,
                *verify,
            )
            .await
        }
        DeployCommand::Export {
            workspace,
            dir,
            item_types,
            overwrite,
            concurrency,
        } => {
            execute_export(
                cli,
                client,
                workspace,
                dir,
                item_types.as_deref(),
                *overwrite,
                *concurrency,
            )
            .await
        }
        DeployCommand::InitParams {
            source,
            compare,
            source_env,
            compare_env,
            out,
            resolve_connections,
        } => {
            if *resolve_connections {
                execute_init_params_with_connections(cli, client, source, out.as_deref()).await
            } else {
                execute_init_params(
                    cli,
                    source,
                    compare.as_deref(),
                    source_env,
                    compare_env,
                    out.as_deref(),
                )
            }
        }
        DeployCommand::Validate {
            source,
            parameters,
            env,
        } => execute_validate(cli, source, parameters.as_deref(), env.as_deref()),
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    clippy::too_many_lines
)]
async fn execute_plan(
    cli: &Cli,
    client: &FabricClient,
    source: &Path,
    workspace: &str,
    item_types: Option<&[String]>,
    delete_orphans: bool,
    allow_unresolved: bool,
    force_all: bool,
    out: Option<&std::path::Path>,
    parameters: Option<&std::path::Path>,
    env: Option<&str>,
    git_diff_ref: Option<&str>,
    exclude_regex: Option<&str>,
    include_items: Option<&[String]>,
    include_folders: Option<&[String]>,
    exclude_folders: Option<&[String]>,
    allow_delete_types: Option<&[String]>,
    _no_folders: bool,
    no_workspace_id_replace: bool,
) -> Result<()> {
    // Validate parameter flags
    if parameters.is_some() && env.is_none() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, "--env is required when --parameters is specified", "Both --parameters and --env must be provided together. Example: --parameters params.json --env dev").into());
    }

    // Resolve workspace
    let workspace_id = resolve_workspace(client, workspace).await?;

    // Parse source directory
    let mut source_workspace = platform::parse_source_directory(source)?;

    if source_workspace.items.is_empty() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, format!("No items found in source directory: {}", source.display()), "Ensure the directory contains item folders with .platform files. Export from a workspace: fabio deploy export --workspace <WS> --dir <DIR>").into());
    }

    // Apply workspace ID auto-replacement (any workspace GUID → target workspace)
    if !no_workspace_id_replace {
        params::replace_default_workspace_id(&mut source_workspace, &workspace_id);
        params::replace_all_workspace_ids(&mut source_workspace, &workspace_id);
    }

    // Apply git diff filter if specified
    if let Some(git_ref) = git_diff_ref {
        let diff_result = git_diff::get_changed_items(source, git_ref)?;
        source_workspace.items.retain(|item| {
            let key = (
                item.metadata.item_type.clone(),
                item.metadata.display_name.clone(),
            );
            diff_result.changed.contains(&key) || diff_result.deleted.contains(&key)
        });
    }

    // Apply selective filters
    apply_item_filters(
        &mut source_workspace,
        exclude_regex,
        include_items,
        include_folders,
        exclude_folders,
        source,
    )?;

    // Apply parameter substitution if configured
    let param_warnings = if let (Some(param_path), Some(env_name)) = (parameters, env) {
        let parsed_params = params::parse_parameters(param_path)?;

        // Build substitution context (no deployed items yet during plan)
        let deployed_items = std::collections::HashMap::new();
        let ctx = params::SubstitutionContext {
            workspace_id: &workspace_id,
            workspace_name: None,
            deployed_items: &deployed_items,
        };

        params::apply_parameters(&mut source_workspace, &parsed_params, env_name, &ctx)?
    } else {
        Vec::new()
    };

    // Build changeset
    let deployed_items = plan::fetch_deployed_items(client, &workspace_id, item_types).await?;
    let workspace_fingerprint = plan::compute_workspace_fingerprint(&deployed_items);

    let changeset = plan::build_changeset(
        cli,
        client,
        &workspace_id,
        &source_workspace,
        &deployed_items,
        item_types,
        delete_orphans,
        force_all,
        allow_delete_types,
    )
    .await?;

    // Check for errors
    if changeset.has_errors() && !allow_unresolved {
        let error_msg = changeset.errors.join("\n  ");
        bail!(
            "Plan has unresolved errors:\n  {error_msg}\nUse --allow-unresolved to proceed anyway."
        );
    }

    // Save plan to file if requested
    if let Some(out_path) = out {
        let plan_json = json!({
            "version": 1,
            "workspace_id": workspace_id,
            "workspace_fingerprint": workspace_fingerprint,
            "source_path": source.display().to_string(),
            "source_git": get_git_metadata(source),
            "changeset": changeset,
            "parameters": parameters.map(|p| p.display().to_string()),
            "env": env,
        });
        let content = serde_json::to_string_pretty(&plan_json)?;
        std::fs::write(out_path, content)?;
    }

    // Render output
    let summary = changeset.summary();
    let git_meta = get_git_metadata(source);

    let mut all_warnings = changeset.warnings.clone();
    all_warnings.extend(param_warnings);

    // Flag as destructive when plan contains deletes or force-all overwrites
    let destructive = summary.delete > 0 || force_all;
    if force_all {
        all_warnings.push(
            "--force-all is active: ALL matched items will be overwritten \
             regardless of content changes. This is irreversible."
                .to_owned(),
        );
    }

    let output_data = json!({
        "workspace_id": workspace_id,
        "source_git": git_meta,
        "changes": changeset.changes,
        "warnings": all_warnings,
        "errors": changeset.errors,
        "summary": summary,
        "destructive": destructive,
        "parameters_applied": parameters.is_some(),
        "env": env,
    });

    output::render_object(cli, &output_data, "summary");

    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    clippy::fn_params_excessive_bools,
    clippy::too_many_lines
)]
async fn execute_apply(
    cli: &Cli,
    client: &FabricClient,
    source: Option<&std::path::Path>,
    workspace: Option<&str>,
    plan_file: Option<&std::path::Path>,
    item_types: Option<&[String]>,
    delete_orphans: bool,
    allow_unresolved: bool,
    fail_fast: bool,
    force: bool,
    force_all: bool,
    concurrency: usize,
    parameters: Option<&std::path::Path>,
    env: Option<&str>,
    no_post_hooks: bool,
    git_diff_ref: Option<&str>,
    exclude_regex: Option<&str>,
    include_items: Option<&[String]>,
    include_folders: Option<&[String]>,
    exclude_folders: Option<&[String]>,
    allow_delete_types: Option<&[String]>,
    _no_folders: bool,
    no_workspace_id_replace: bool,
    _shortcut_exclude_regex: Option<&str>,
    post_run_item: Option<&str>,
    strategy: &str,
    verify: bool,
) -> Result<()> {
    // Validate parameter flags
    if parameters.is_some() && env.is_none() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, "--env is required when --parameters is specified", "Both --parameters and --env must be provided together. Example: --parameters params.json --env dev").into());
    }

    // Determine source and workspace from either direct args or plan file
    let (workspace_id, mut source_workspace, changeset) = if let Some(plan_path) = plan_file {
        // Load saved plan
        let content = std::fs::read_to_string(plan_path)?;
        let plan: serde_json::Value = serde_json::from_str(&content)?;

        let ws_id = plan
            .get("workspace_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "Invalid plan file: missing workspace_id", "The plan file is malformed. Regenerate: fabio deploy plan --source <DIR> --workspace <WS> --out plan.json"))?
            .to_owned();

        let cs: changeset::Changeset = serde_json::from_value(
            plan.get("changeset")
                .ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "Invalid plan file: missing changeset", "The plan file is malformed. Regenerate: fabio deploy plan --source <DIR> --workspace <WS> --out plan.json"))?
                .clone(),
        )?;

        let source_path = plan
            .get("source_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "Invalid plan file: missing source_path", "The plan file is malformed. Regenerate: fabio deploy plan --source <DIR> --workspace <WS> --out plan.json"))?;

        let src = platform::parse_source_directory(std::path::Path::new(source_path))?;

        // Staleness check: compare workspace fingerprint from plan time vs now
        if let Some(saved_fingerprint) = plan.get("workspace_fingerprint").and_then(|v| v.as_str())
        {
            let current_items = plan::fetch_deployed_items(client, &ws_id, None).await?;
            let current_fingerprint = plan::compute_workspace_fingerprint(&current_items);

            if current_fingerprint != saved_fingerprint && !force {
                return Err(FabioError::with_typed_hint(
                    ErrorCode::InvalidInput,
                    format!(
                        "Workspace state has changed since plan was created.\n\
                         Plan fingerprint: {saved_fingerprint}\n\
                         Current fingerprint: {current_fingerprint}"
                    ),
                    "Use --force to apply anyway, or re-run `fabio deploy plan` to get a fresh plan.",
                    HintType::SafetyBypass,
                )
                .set_verify_after("fabio deploy plan --source <DIR> --workspace <WS> --dry-run")
                .into());
            }
        }

        (ws_id, src, cs)
    } else {
        // Build changeset from source + workspace
        let src_path =
            source.ok_or_else(|| FabioError::with_hint(ErrorCode::InvalidInput, "--source is required when not using --plan", "Provide --source <DIR> or use --plan <FILE> from a previous 'fabio deploy plan --out' run."))?;
        let ws = workspace.ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::InvalidInput,
                "--workspace is required when not using --plan",
                "Provide --workspace <ID|NAME>. List workspaces: fabio workspace list",
            )
        })?;

        let workspace_id = resolve_workspace(client, ws).await?;
        let mut source_ws = platform::parse_source_directory(src_path)?;

        if source_ws.items.is_empty() {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput, format!("No items found in source directory: {}", src_path.display()), "Ensure the directory contains item folders with .platform files. Export from a workspace: fabio deploy export --workspace <WS> --dir <DIR>").into());
        }

        // Apply workspace ID auto-replacement
        if !no_workspace_id_replace {
            params::replace_default_workspace_id(&mut source_ws, &workspace_id);
            params::replace_all_workspace_ids(&mut source_ws, &workspace_id);
        }

        // Apply git diff filter
        if let Some(git_ref) = git_diff_ref {
            let diff_result = git_diff::get_changed_items(src_path, git_ref)?;
            source_ws.items.retain(|item| {
                let key = (
                    item.metadata.item_type.clone(),
                    item.metadata.display_name.clone(),
                );
                diff_result.changed.contains(&key) || diff_result.deleted.contains(&key)
            });
        }

        // Apply selective filters
        apply_item_filters(
            &mut source_ws,
            exclude_regex,
            include_items,
            include_folders,
            exclude_folders,
            src_path,
        )?;

        // Apply parameter substitution before building changeset
        if let (Some(param_path), Some(env_name)) = (parameters, env) {
            let parsed_params = params::parse_parameters(param_path)?;
            let deployed_items = std::collections::HashMap::new();
            let ctx = params::SubstitutionContext {
                workspace_id: &workspace_id,
                workspace_name: None,
                deployed_items: &deployed_items,
            };
            let _warnings =
                params::apply_parameters(&mut source_ws, &parsed_params, env_name, &ctx)?;
        }

        let deployed = plan::fetch_deployed_items(client, &workspace_id, item_types).await?;

        let cs = plan::build_changeset(
            cli,
            client,
            &workspace_id,
            &source_ws,
            &deployed,
            item_types,
            delete_orphans,
            force_all,
            allow_delete_types,
        )
        .await?;

        (workspace_id, source_ws, cs)
    };

    // Apply parameter substitution for plan-file path (re-apply to parsed source)
    if plan_file.is_some()
        && let (Some(param_path), Some(env_name)) = (parameters, env)
    {
        let parsed_params = params::parse_parameters(param_path)?;
        let deployed_items = std::collections::HashMap::new();
        let ctx = params::SubstitutionContext {
            workspace_id: &workspace_id,
            workspace_name: None,
            deployed_items: &deployed_items,
        };
        let _warnings =
            params::apply_parameters(&mut source_workspace, &parsed_params, env_name, &ctx)?;
    }

    // Check for errors
    if changeset.has_errors() && !allow_unresolved {
        let error_msg = changeset.errors.join("\n  ");
        bail!(
            "Deployment blocked by unresolved errors:\n  {error_msg}\nUse --allow-unresolved to proceed."
        );
    }

    // Check if there's anything to do
    if !changeset.has_changes() {
        let mut output_data = json!({
            "status": "no_changes",
            "message": "Workspace is already in sync with source.",
            "summary": changeset.summary(),
        });
        // --verify: no changes means the workspace already matches the source
        // (the plan showed all Skip), so convergence is trivially satisfied.
        if verify {
            output_data.as_object_mut().unwrap().insert(
                "verification".to_owned(),
                json!({
                    "converged": true,
                    "checked": 0,
                    "discrepancies": [],
                    "existence_only": [],
                    "note": "No changes to apply — the workspace already matches the source (all items Skip)."
                }),
            );
        }
        output::render_object(cli, &output_data, "status");
        return Ok(());
    }

    // Dry-run guard
    if cli.dry_run {
        let summary = changeset.summary();
        let destructive = summary.delete > 0 || force_all;
        let mut warnings: Vec<String> = changeset.warnings.clone();
        if force_all {
            warnings.push(
                "--force-all is active: ALL matched items will be overwritten regardless of \
                 content changes. This is irreversible — previous definitions cannot be recovered."
                    .to_owned(),
            );
        }
        let output_data = json!({
            "status": "dry_run",
            "message": format!(
                "Would create {}, update {}, delete {}, skip {}",
                summary.create, summary.update, summary.delete, summary.skip
            ),
            "changes": changeset.changes.iter()
                .filter(|c| c.action != ChangeAction::Skip)
                .collect::<Vec<_>>(),
            "summary": summary,
            "destructive": destructive,
            "warnings": warnings,
        });
        output::render_object(cli, &output_data, "status");
        return Ok(());
    }

    // Execute the changeset (strategy dispatch)
    let effective_concurrency = if strategy == "sequential" {
        1
    } else {
        concurrency
    };

    let result = if strategy == "bulk" {
        apply::execute_changeset_bulk(cli, client, &workspace_id, &changeset, &source_workspace)
            .await?
    } else {
        apply::execute_changeset(
            cli,
            client,
            &workspace_id,
            &changeset,
            &source_workspace,
            effective_concurrency,
            fail_fast,
        )
        .await?
    };

    // Execute post-deploy hooks (unless --no-post-hooks)
    let mut hook_results: Vec<serde_json::Value> = if !no_post_hooks && !cli.dry_run {
        apply::execute_post_hooks(cli, client, &workspace_id, &result.succeeded).await
    } else {
        Vec::new()
    };

    // Execute shortcut reconciliation for Lakehouse items (unless --no-post-hooks)
    if !no_post_hooks && !cli.dry_run {
        let shortcut_results = apply::execute_shortcut_hooks(
            cli,
            client,
            &workspace_id,
            &result.succeeded,
            &source_workspace,
        )
        .await;
        hook_results.extend(shortcut_results);
    }

    // Apply governance tags to newly created items (unless --no-post-hooks)
    if !no_post_hooks && !cli.dry_run {
        let governance_results = apply::apply_governance_tags(
            cli,
            client,
            &workspace_id,
            &result.succeeded,
            &source_workspace,
        )
        .await;
        hook_results.extend(governance_results);
    }

    // Activate variable library value sets matching --env name (unless --no-post-hooks)
    if !no_post_hooks
        && !cli.dry_run
        && let Some(env_name) = env
    {
        let vl_results = apply::activate_variable_library_value_sets(
            cli,
            client,
            &workspace_id,
            &result.succeeded,
            env_name,
        )
        .await;
        hook_results.extend(vl_results);
    }

    // Apply job schedules for deployed items (unless --no-post-hooks)
    if !no_post_hooks && !cli.dry_run {
        let schedule_results = apply::apply_item_schedules(
            cli,
            client,
            &workspace_id,
            &result.succeeded,
            &source_workspace,
        )
        .await;
        hook_results.extend(schedule_results);
    }

    // Run a post-deploy item (pipeline/notebook) if specified (unless --no-post-hooks)
    if !no_post_hooks
        && !cli.dry_run
        && let Some(item_name) = post_run_item
    {
        apply::emit_progress(
            cli.quiet,
            &format!("post-hook: triggering run for \"{item_name}\""),
        );
        // Find the item ID from succeeded deploys or list workspace items
        let run_result = run_post_deploy_item(client, &workspace_id, item_name).await;
        match run_result {
            Ok(status) => {
                hook_results.push(json!({
                    "hook": "post_run_item",
                    "item_name": item_name,
                    "status": status
                }));
            }
            Err(e) => {
                apply::emit_progress(
                    cli.quiet,
                    &format!(
                        "  post-hook WARNING: run \"{item_name}\": {}",
                        e.root_cause()
                    ),
                );
                hook_results.push(json!({
                    "hook": "post_run_item",
                    "item_name": item_name,
                    "status": "failed",
                    "error": e.to_string()
                }));
            }
        }
    }

    // Render result
    let mut output_data = json!({
        "status": if result.failed.is_empty() { "succeeded" } else { "partial_failure" },
        "succeeded": result.succeeded.len(),
        "failed": result.failed.len(),
        "skipped": result.skipped.len(),
        "duration_ms": result.duration_ms,
        "failures": result.failed,
    });

    // Add contextual hints for common failure patterns
    let has_pipeline_reference_error = result.failed.iter().any(|f| {
        f.change.item_type.eq_ignore_ascii_case("DataPipeline")
            && (f.error.contains("invalid reference") || f.error.contains("UnknownError"))
    });
    if has_pipeline_reference_error {
        output_data.as_object_mut().unwrap().insert(
            "hint".to_owned(),
            json!("Pipeline failures may be caused by unresolved connection or item GUIDs. Run: fabio deploy init-params --source <DIR> --resolve-connections --out params.json"),
        );
    }

    if !hook_results.is_empty() {
        output_data
            .as_object_mut()
            .unwrap()
            .insert("post_hooks".to_owned(), json!(hook_results));
    }

    // Post-apply convergence audit (opt-in --verify). Runs AFTER post-hooks so
    // LRO creates have settled. Report-only: adds a `verification` block, never
    // changes the exit code.
    if verify && !cli.dry_run {
        apply::emit_progress(cli.quiet, "verifying convergence of applied items...");
        let v = Box::pin(apply::verify_convergence(
            client,
            &workspace_id,
            &result.succeeded,
        ))
        .await;
        output_data.as_object_mut().unwrap().insert(
            "verification".to_owned(),
            json!({
                "converged": v.converged,
                "checked": v.checked,
                "discrepancies": v.discrepancies,
                "existence_only": v.existence_only,
                "note": "Re-fetched created/updated items after post-hooks. Content items hash-compared to source (.platform excluded from the hash); platform-only items existence-checked. Deletes are confirmed by apply itself. Report-only — exit code unchanged."
            }),
        );
    }

    output::render_object(cli, &output_data, "status");

    if !result.failed.is_empty() {
        bail!("Deployment completed with {} failures", result.failed.len());
    }

    Ok(())
}

async fn execute_export(
    cli: &Cli,
    client: &FabricClient,
    workspace: &str,
    output: &std::path::Path,
    item_types: Option<&[String]>,
    overwrite: bool,
    concurrency: usize,
) -> Result<()> {
    let workspace_id = resolve_workspace(client, workspace).await?;

    let result = export::export_workspace(
        cli,
        client,
        &workspace_id,
        output,
        item_types,
        overwrite,
        concurrency,
    )
    .await?;

    let output_data = json!({
        "status": if cli.dry_run { "dry_run" } else { "exported" },
        "workspace_id": workspace_id,
        "output_dir": output.display().to_string(),
        "total_items": result.total_items,
        "exported": result.exported,
        "skipped": result.skipped,
    });

    output::render_object(cli, &output_data, "status");

    Ok(())
}

fn execute_init_params(
    cli: &Cli,
    source: &Path,
    compare: Option<&Path>,
    source_env: &str,
    compare_env: &str,
    out: Option<&Path>,
) -> Result<()> {
    let result = if let Some(compare_dir) = compare {
        init_params::diff_for_parameters(source, compare_dir, source_env, compare_env)?
    } else {
        init_params::scan_for_candidates(source)?
    };

    // Write to file if --out specified
    if let Some(out_path) = out {
        let content = serde_json::to_string_pretty(&result.parameters_json)?;
        std::fs::write(out_path, &content)?;
    }

    // Render output
    let output_data = json!({
        "status": "generated",
        "mode": result.summary.mode,
        "source_items": result.summary.source_items,
        "compare_items": result.summary.compare_items,
        "rules_generated": result.summary.rules_generated,
        "guids_found": result.summary.guids_found,
        "parameters": result.parameters_json,
        "output_file": out.map(|p| p.display().to_string()),
    });

    output::render_object(cli, &output_data, "status");

    Ok(())
}

/// Execute `deploy init-params --resolve-connections`.
///
/// Scans pipeline definitions for connection GUIDs, looks up connections in the
/// tenant by name/ID, and generates a `parameters.json` with pre-resolved mappings.
async fn execute_init_params_with_connections(
    cli: &Cli,
    client: &FabricClient,
    source: &Path,
    out: Option<&Path>,
) -> Result<()> {
    let workspace = platform::parse_source_directory(source)?;

    // Step 1: Extract all connection GUIDs from pipeline definitions
    let mut connection_guids: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let guid_re = regex::Regex::new(
        r#""connection"\s*:\s*"([0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12})""#,
    ).expect("valid regex");

    for item in &workspace.items {
        if !item.metadata.item_type.eq_ignore_ascii_case("DataPipeline") {
            continue;
        }
        for part in &item.parts {
            let Ok(decoded) =
                base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &part.payload)
            else {
                continue;
            };
            let Ok(content) = String::from_utf8(decoded) else {
                continue;
            };
            for cap in guid_re.captures_iter(&content) {
                connection_guids.insert(cap[1].to_lowercase());
            }
        }
    }

    if connection_guids.is_empty() {
        let output_data = json!({
            "status": "no_connections",
            "message": "No connection GUIDs found in pipeline definitions"
        });
        output::render_object(cli, &output_data, "status");
        return Ok(());
    }

    // Step 2: Fetch available connections from the tenant
    let connections_resp = client.get_list("/connections", "value", true, None).await?;

    let available_connections: Vec<(String, String)> = connections_resp
        .items
        .iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_str()?.to_owned();
            let name = c
                .get("displayName")
                .and_then(|v| v.as_str())
                .unwrap_or("(unnamed)")
                .to_owned();
            Some((id, name))
        })
        .collect();

    // Step 3: Build find_replace rules
    let mut rules: Vec<serde_json::Value> = Vec::new();
    let mut resolved = 0_usize;
    let mut unresolved = Vec::new();

    for guid in &connection_guids {
        // Check if this GUID matches an existing connection directly
        let matched = available_connections
            .iter()
            .find(|(id, _)| id.eq_ignore_ascii_case(guid));

        if let Some((id, name)) = matched {
            // Already matches a connection in the tenant — no replacement needed
            rules.push(json!({
                "find_value": guid,
                "replace_value": { "_ALL_": id },
                "item_type": "DataPipeline",
                "_comment": format!("Connection already exists: \"{name}\"")
            }));
            resolved += 1;
        } else {
            // GUID doesn't match any existing connection — ask user to fill in
            rules.push(json!({
                "find_value": guid,
                "replace_value": { "_ALL_": format!("TODO: replace with connection ID (original not found in tenant)") },
                "item_type": "DataPipeline",
                "_available_connections": available_connections.iter().map(|(id, name)| format!("{name}: {id}")).collect::<Vec<_>>()
            }));
            unresolved.push(guid.clone());
        }
    }

    let parameters_json = json!({ "find_replace": rules });

    // Write to file if --out specified
    if let Some(out_path) = out {
        let content = serde_json::to_string_pretty(&parameters_json)?;
        std::fs::write(out_path, &content)?;
    }

    let output_data = json!({
        "status": if unresolved.is_empty() { "fully_resolved" } else { "partially_resolved" },
        "connection_guids_found": connection_guids.len(),
        "resolved": resolved,
        "unresolved": unresolved.len(),
        "unresolved_guids": unresolved,
        "available_connections": available_connections.iter().map(|(id, name)| json!({"id": id, "name": name})).collect::<Vec<_>>(),
        "parameters": parameters_json,
        "output_file": out.map(|p| p.display().to_string()),
    });

    output::render_object(cli, &output_data, "status");

    Ok(())
}

/// Execute `deploy validate` — local-only pre-flight checks on source directory.
#[allow(clippy::too_many_lines)]
fn execute_validate(
    cli: &Cli,
    source: &Path,
    parameters: Option<&Path>,
    env: Option<&str>,
) -> Result<()> {
    use std::collections::{HashMap, HashSet};

    use base64::Engine;
    use base64::engine::general_purpose::STANDARD as BASE64;

    use self::ordering::DEPLOY_ORDER;
    use self::params::parse_parameters;
    use self::platform::parse_source_directory;

    let mut errors: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // --- 1. Parse source directory ---
    if !source.exists() {
        return Err(FabioError::with_hint(ErrorCode::InvalidInput, format!("Source directory does not exist: {}", source.display()), "Check the path. Create one with: fabio deploy export --workspace <WS> --dir <DIR>, or clone a git repo: git clone <URL> && fabio deploy apply --source <cloned-dir>").into());
    }

    let source_ws = match parse_source_directory(source) {
        Ok(ws) => ws,
        Err(e) => {
            return Err(FabioError::with_hint(ErrorCode::InvalidInput, format!("Failed to parse source directory: {e}"), "Ensure item directories have valid .platform files. Validate: fabio deploy validate --source <DIR>").into());
        }
    };

    if source_ws.items.is_empty() {
        errors.push("No items found in source directory".to_string());
    }

    // --- 2. Check for unknown item types ---
    let known_types: HashSet<&str> = DEPLOY_ORDER.iter().copied().collect();
    for item in &source_ws.items {
        if !known_types
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&item.metadata.item_type))
        {
            warnings.push(format!(
                "\"{}\" has unknown item type \"{}\"; it will be deployed last",
                item.metadata.display_name, item.metadata.item_type
            ));
        }
    }

    // --- 3. Check for duplicate (type, name) pairs ---
    let mut type_name_seen: HashMap<(String, String), usize> = HashMap::new();
    for item in &source_ws.items {
        let key = (
            item.metadata.item_type.to_lowercase(),
            item.metadata.display_name.to_lowercase(),
        );
        *type_name_seen.entry(key).or_insert(0) += 1;
    }
    for ((item_type, name), count) in &type_name_seen {
        if *count > 1 {
            errors.push(format!(
                "Duplicate item: type=\"{item_type}\" name=\"{name}\" appears {count} times"
            ));
        }
    }

    // --- 4. Check for duplicate logical IDs ---
    let mut logical_id_seen: HashMap<&str, Vec<&str>> = HashMap::new();
    for item in &source_ws.items {
        if let Some(ref lid) = item.metadata.logical_id {
            logical_id_seen
                .entry(lid.as_str())
                .or_default()
                .push(&item.metadata.display_name);
        }
    }
    for (lid, names) in &logical_id_seen {
        if names.len() > 1 {
            errors.push(format!(
                "Duplicate logical ID \"{lid}\" used by: {}",
                names.join(", ")
            ));
        }
    }

    // --- 5. Validate cross-references (logical IDs in payloads) ---
    let all_logical_ids: HashSet<&str> = source_ws
        .items
        .iter()
        .filter_map(|item| item.metadata.logical_id.as_deref())
        .collect();

    for item in &source_ws.items {
        for part in &item.parts {
            let Ok(bytes) = BASE64.decode(&part.payload) else {
                warnings.push(format!(
                    "\"{}\" has invalid base64 in part \"{}\"",
                    item.metadata.display_name, part.path
                ));
                continue;
            };
            let Ok(content) = std::str::from_utf8(&bytes) else {
                continue; // binary content, skip reference checks
            };

            // Check for references to logical IDs that don't exist in source
            for other_item in &source_ws.items {
                let Some(ref other_lid) = other_item.metadata.logical_id else {
                    continue;
                };
                if std::ptr::eq(item, other_item) {
                    continue;
                }
                if content.contains(other_lid.as_str())
                    && !all_logical_ids.contains(other_lid.as_str())
                {
                    // This branch is unreachable since all_logical_ids contains all items' logical IDs,
                    // but the check matters if we filter by item_types later.
                    warnings.push(format!(
                        "\"{}\" references logical ID \"{}\" (\"{}\") which is not in source",
                        item.metadata.display_name, other_lid, other_item.metadata.display_name,
                    ));
                }
            }
        }
    }

    // --- 6. Validate parameters file ---
    if let Some(params_path) = parameters {
        match parse_parameters(params_path) {
            Ok(params) => {
                // Check that the environment key exists in rules
                if let Some(env_name) = env {
                    for (i, rule) in params.find_replace.iter().enumerate() {
                        let has_env = rule.replace_value.keys().any(|k| {
                            k.eq_ignore_ascii_case(env_name) || k.eq_ignore_ascii_case("_ALL_")
                        });
                        if !has_env {
                            warnings.push(format!(
                                "find_replace rule #{}: no value for env \"{env_name}\" (and no _ALL_ fallback)",
                                i + 1
                            ));
                        }
                    }
                    for (i, rule) in params.key_value_replace.iter().enumerate() {
                        let has_env = rule.replace_value.keys().any(|k| {
                            k.eq_ignore_ascii_case(env_name) || k.eq_ignore_ascii_case("_ALL_")
                        });
                        if !has_env {
                            warnings.push(format!(
                                "key_value_replace rule #{}: no value for env \"{env_name}\" (and no _ALL_ fallback)",
                                i + 1
                            ));
                        }
                    }
                    for (i, rule) in params.spark_pool.iter().enumerate() {
                        let has_env = rule.replace_value.keys().any(|k| {
                            k.eq_ignore_ascii_case(env_name) || k.eq_ignore_ascii_case("_ALL_")
                        });
                        if !has_env {
                            warnings.push(format!(
                                "spark_pool rule #{}: no value for env \"{env_name}\" (and no _ALL_ fallback)",
                                i + 1
                            ));
                        }
                    }
                }
            }
            Err(e) => {
                errors.push(format!("Parameters file error: {e}"));
            }
        }
    } else if env.is_some() {
        warnings.push("--env specified but no --parameters file provided".to_string());
    }

    // --- 7. Check for items with empty display name ---
    for item in &source_ws.items {
        if item.metadata.display_name.trim().is_empty() {
            errors.push(format!(
                "Item of type \"{}\" has empty display name",
                item.metadata.item_type
            ));
        }
    }

    // --- 8. Check notebooks for missing notebook-settings.json (auto-binding) ---
    for item in &source_ws.items {
        if item.metadata.item_type.eq_ignore_ascii_case("Notebook") {
            let has_settings = item
                .parts
                .iter()
                .any(|p| p.path.eq_ignore_ascii_case("notebook-settings.json"));
            if !has_settings {
                warnings.push(format!(
                    "Notebook \"{}\" is missing notebook-settings.json — auto-binding of lakehouse dependencies will not work after deployment. Add this file to enable automatic rebinding.",
                    item.metadata.display_name
                ));
            }
        }
    }

    // --- Produce output ---
    let is_valid = errors.is_empty();
    let output_data = json!({
        "status": if is_valid { "valid" } else { "invalid" },
        "items": source_ws.items.len(),
        "errors": errors,
        "warnings": warnings,
        "summary": {
            "errors": errors.len(),
            "warnings": warnings.len(),
        }
    });

    output::render_object(cli, &output_data, "status");

    if !is_valid {
        bail!("Validation failed with {} error(s)", errors.len());
    }

    Ok(())
}

/// Apply selective item filters to a source workspace.
///
/// Filters are applied in this order:
/// 1. `include_items` (if set, only matching items are kept)
/// 2. `exclude_regex` (remove items whose name matches)
/// 3. `include_folders` / `exclude_folders` (filter by folder path)
fn apply_item_filters(
    source: &mut platform::SourceWorkspace,
    exclude_regex: Option<&str>,
    include_items: Option<&[String]>,
    include_folders: Option<&[String]>,
    exclude_folders: Option<&[String]>,
    _source_dir: &Path,
) -> Result<()> {
    // 1. include_items: only keep items matching "Name.Type" format
    if let Some(items) = include_items {
        let allowed: std::collections::HashSet<String> =
            items.iter().map(|s| s.to_lowercase()).collect();
        source.items.retain(|item| {
            let key = format!("{}.{}", item.metadata.display_name, item.metadata.item_type)
                .to_lowercase();
            allowed.contains(&key)
        });
    }

    // 2. exclude_regex: remove items whose display name matches
    if let Some(pattern) = exclude_regex {
        let re = regex::Regex::new(pattern)
            .with_context(|| format!("Invalid --exclude-regex pattern: {pattern}"))?;
        source
            .items
            .retain(|item| !re.is_match(&item.metadata.display_name));
    }

    // 3. include_folders / exclude_folders (mutually exclusive)
    if let Some(include_paths) = include_folders {
        source.items.retain(|item| {
            let folder = &item.folder_path;
            if folder.is_empty() {
                // Root items only kept if "/" is in include list
                include_paths.iter().any(|p| p == "/")
            } else {
                include_paths
                    .iter()
                    .any(|p| folder == p || folder.starts_with(&format!("{p}/")))
            }
        });
    } else if let Some(exclude_paths) = exclude_folders {
        source.items.retain(|item| {
            let folder = &item.folder_path;
            if folder.is_empty() {
                true // Root items not affected by folder exclusion
            } else {
                !exclude_paths
                    .iter()
                    .any(|p| folder == p || folder.starts_with(&format!("{p}/")))
            }
        });
    }

    Ok(())
}

/// Protected item types that require explicit `--allow-delete-types` to be deleted.
const PROTECTED_DELETE_TYPES: &[&str] = &[
    "Lakehouse",
    "Warehouse",
    "SQLDatabase",
    "Eventhouse",
    "KQLDatabase",
];

/// Check if a delete action is allowed for the given item type.
/// Returns true if the type is allowed to be deleted.
pub(super) fn is_delete_allowed(item_type: &str, allow_delete_types: Option<&[String]>) -> bool {
    let is_protected = PROTECTED_DELETE_TYPES
        .iter()
        .any(|t| t.eq_ignore_ascii_case(item_type));

    if !is_protected {
        return true; // Non-protected types can always be deleted
    }

    // Protected types require explicit opt-in
    allow_delete_types
        .is_some_and(|allowed| allowed.iter().any(|t| t.eq_ignore_ascii_case(item_type)))
}

/// Run a named item (pipeline/notebook) as a post-deploy data orchestration step.
///
/// Finds the item by display name in the workspace, determines its job type,
/// and triggers a run-on-demand via the Job Scheduler API.
async fn run_post_deploy_item(
    client: &FabricClient,
    workspace_id: &str,
    item_name: &str,
) -> Result<String> {
    // List workspace items to find the named item
    let resp = client
        .get_list(
            &format!("/workspaces/{workspace_id}/items"),
            "value",
            true,
            None,
        )
        .await?;

    let item = resp
        .items
        .iter()
        .find(|i| {
            i.get("displayName")
                .and_then(|v| v.as_str())
                .is_some_and(|n| n.eq_ignore_ascii_case(item_name))
        })
        .ok_or_else(|| {
            FabioError::with_hint(
                ErrorCode::NotFound,
                format!("Item \"{item_name}\" not found in workspace"),
                "Check the item name. List items: fabio item list --workspace <WS>",
            )
        })?;

    let item_id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
    let item_type = item
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    // Determine the job type based on item type
    let job_type = match item_type {
        "DataPipeline" => "Pipeline",
        "Notebook" => "RunNotebook",
        "SparkJobDefinition" => "SparkJob",
        _ => "DefaultJob",
    };

    // Trigger run-on-demand
    let path =
        format!("/workspaces/{workspace_id}/items/{item_id}/jobs/instances?jobType={job_type}");
    client.post(&path, &json!({}), false).await?;

    Ok("triggered".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── is_delete_allowed tests ─────────────────────────────────────────────

    #[test]
    fn non_protected_type_always_allowed() {
        assert!(is_delete_allowed("Notebook", None));
        assert!(is_delete_allowed("DataPipeline", None));
        assert!(is_delete_allowed("Report", None));
        assert!(is_delete_allowed("SemanticModel", None));
        assert!(is_delete_allowed("SparkJobDefinition", None));
    }

    #[test]
    fn protected_type_blocked_without_allow_flag() {
        assert!(!is_delete_allowed("Lakehouse", None));
        assert!(!is_delete_allowed("Warehouse", None));
        assert!(!is_delete_allowed("SQLDatabase", None));
        assert!(!is_delete_allowed("Eventhouse", None));
        assert!(!is_delete_allowed("KQLDatabase", None));
    }

    #[test]
    fn protected_type_blocked_with_empty_allow_list() {
        let empty: Vec<String> = vec![];
        assert!(!is_delete_allowed("Lakehouse", Some(&empty)));
        assert!(!is_delete_allowed("Warehouse", Some(&empty)));
    }

    #[test]
    fn protected_type_allowed_when_explicitly_listed() {
        let allowed = vec!["Lakehouse".to_string()];
        assert!(is_delete_allowed("Lakehouse", Some(&allowed)));
    }

    #[test]
    fn protected_type_blocked_when_different_type_listed() {
        let allowed = vec!["Warehouse".to_string()];
        assert!(!is_delete_allowed("Lakehouse", Some(&allowed)));
    }

    #[test]
    fn multiple_protected_types_in_allow_list() {
        let allowed = vec![
            "Lakehouse".to_string(),
            "Warehouse".to_string(),
            "KQLDatabase".to_string(),
        ];
        assert!(is_delete_allowed("Lakehouse", Some(&allowed)));
        assert!(is_delete_allowed("Warehouse", Some(&allowed)));
        assert!(is_delete_allowed("KQLDatabase", Some(&allowed)));
        // Not in list
        assert!(!is_delete_allowed("SQLDatabase", Some(&allowed)));
        assert!(!is_delete_allowed("Eventhouse", Some(&allowed)));
    }

    #[test]
    fn case_insensitive_type_matching() {
        assert!(!is_delete_allowed("lakehouse", None));
        assert!(!is_delete_allowed("LAKEHOUSE", None));
        assert!(!is_delete_allowed("LakeHouse", None));
        assert!(!is_delete_allowed("sqldatabase", None));
        assert!(!is_delete_allowed("kqldatabase", None));
    }

    #[test]
    fn case_insensitive_allow_list_matching() {
        let allowed = vec!["lakehouse".to_string()];
        assert!(is_delete_allowed("Lakehouse", Some(&allowed)));
        assert!(is_delete_allowed("LAKEHOUSE", Some(&allowed)));
        assert!(is_delete_allowed("lakehouse", Some(&allowed)));
    }

    #[test]
    fn non_protected_type_allowed_regardless_of_allow_list() {
        // Non-protected types are always allowed, even with an empty allow list
        let empty: Vec<String> = vec![];
        assert!(is_delete_allowed("Notebook", Some(&empty)));
        assert!(is_delete_allowed("Report", Some(&empty)));
        assert!(is_delete_allowed("DataPipeline", None));
    }

    #[test]
    fn unknown_item_type_is_not_protected() {
        assert!(is_delete_allowed("FutureNewItemType", None));
        assert!(is_delete_allowed("", None));
    }

    // ─── PROTECTED_DELETE_TYPES constant tests ───────────────────────────────

    #[test]
    fn protected_types_list_is_non_empty() {
        assert!(!PROTECTED_DELETE_TYPES.is_empty());
    }

    #[test]
    fn protected_types_are_data_bearing() {
        // All protected types are items that contain user data (tables, files, databases)
        // This is a documentation test — if we add new protected types, they should
        // be data-bearing items where deletion means data loss.
        let expected = [
            "Lakehouse",
            "Warehouse",
            "SQLDatabase",
            "Eventhouse",
            "KQLDatabase",
        ];
        for t in &expected {
            assert!(
                PROTECTED_DELETE_TYPES.contains(t),
                "Expected {t} to be protected"
            );
        }
        assert_eq!(
            PROTECTED_DELETE_TYPES.len(),
            expected.len(),
            "Protected types count mismatch — update this test when adding new types"
        );
    }
}
