#![recursion_limit = "512"]

mod agent;
mod cli;
mod client;
mod commands;
mod definition_spec;
mod errors;
mod llm;
mod mcp_client;
mod metrics;
mod output;
mod parallel;
mod token_cache;
mod verbose;

use std::io::Write;

use anyhow::Result;
use clap::Parser;
use cli::Cli;

fn main() {
    // On Windows the default stack is 1 MB vs 8 MB on Linux/macOS. The async
    // command dispatch generates large future state machines that exceed 1 MB
    // when there are many command variants. Run all logic on a thread with an
    // explicit 8 MB stack to match the Linux/macOS behaviour.
    let result = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(run)
        .expect("failed to spawn main thread")
        .join()
        .expect("main thread panicked");

    if let Err(exit_code) = result {
        std::process::exit(exit_code);
    }
}

fn run() -> std::result::Result<(), i32> {
    // Inject profile defaults as env vars BEFORE clap parses, so that
    // clap `env = "FABIO_..."` attributes pick them up as fallback values.
    // Precedence: explicit CLI flag > env var (external) > profile > clap default.
    inject_profile_env_vars();

    // Use try_parse so we can handle missing subcommand gracefully:
    // print help to stdout (composable) instead of stderr (clap default).
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) => {
            match e.kind() {
                clap::error::ErrorKind::DisplayHelp
                | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
                | clap::error::ErrorKind::DisplayVersion => {
                    // Write help/version to stdout so piping works:
                    // `fabio | grep`, `fabio | less`
                    write!(std::io::stdout(), "{e}").ok();
                    return Ok(());
                }
                _ => {
                    e.print().ok();
                    return Err(2);
                }
            }
        }
    };

    // Enforce HTTPS on any endpoint/scope override env vars before any network
    // use — fabio only communicates with endpoints over TLS.
    if let Err(e) = client::validate_endpoint_env_overrides() {
        if let Some(fabio_err) = e.downcast_ref::<errors::FabioError>() {
            output::render_error(fabio_err);
            return Err(fabio_err.code.exit_code());
        }
        let fabio_err = errors::FabioError::new(errors::ErrorCode::InvalidInput, e.to_string());
        output::render_error(&fabio_err);
        return Err(fabio_err.code.exit_code());
    }

    // Fail fast on a malformed or jq-shaped `--query` BEFORE any network call, so
    // an agent gets a teaching error instead of a silent `{"data":null}` success.
    if let Some(ref q) = cli.query
        && let Some(fabio_err) = output::validate_query(q)
    {
        output::render_error(&fabio_err);
        return Err(fabio_err.code.exit_code());
    }

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let quiet = cli.quiet;
    let result: Result<()> = runtime.block_on(Box::pin(commands::execute(cli)));

    // Emit timing summary to stderr (diagnostics channel, not stdout data)
    if !quiet {
        metrics::emit_timing_summary();
    }

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some(fabio_err) = e.downcast_ref::<errors::FabioError>() {
                output::render_error(fabio_err);
                return Err(fabio_err.code.exit_code());
            }
            // Unexpected error - still render as structured JSON
            let fabio_err = errors::FabioError::new(errors::ErrorCode::Unknown, e.to_string());
            output::render_error(&fabio_err);
            Err(fabio_err.code.exit_code())
        }
    }
}

/// Load the active profile and inject its values as environment variables,
/// only if those env vars are not already set externally. This allows
/// clap `env = "FABIO_..."` attributes to pick them up as fallback values.
///
/// Precedence: explicit CLI flag > external env var > profile value > clap default.
///
/// # Safety
/// `set_var` is unsafe in edition 2024 because it is not thread-safe. This
/// function is called at program startup before any threads exist (before
/// tokio runtime and before `std::thread::spawn` in `main()`), so no data
/// race can occur.
#[allow(unsafe_code)]
fn inject_profile_env_vars() {
    use commands::profile::ProfileStore;

    let store = ProfileStore::load();

    // Pre-scan argv for --profile <name> since clap hasn't parsed yet.
    // This allows `--profile prod` to correctly inject that profile's defaults
    // (not just the active profile).
    let profile_from_argv = scan_profile_from_args();

    let profile_name = profile_from_argv
        .or_else(|| std::env::var("FABIO_PROFILE").ok())
        .or_else(|| store.active.clone());

    let Some(name) = profile_name else {
        return;
    };
    let Some(profile) = store.profiles.get(&name) else {
        return;
    };

    // SAFETY: single-threaded at this point — called before tokio runtime is built.
    if let Some(ref ws) = profile.workspace
        && std::env::var("FABIO_WORKSPACE").is_err()
    {
        unsafe { std::env::set_var("FABIO_WORKSPACE", ws) };
    }
    if let Some(ref cap) = profile.capacity
        && std::env::var("FABIO_CAPACITY").is_err()
    {
        unsafe { std::env::set_var("FABIO_CAPACITY", cap) };
    }
    if let Some(ref fmt) = profile.output
        && std::env::var("FABIO_OUTPUT").is_err()
    {
        unsafe { std::env::set_var("FABIO_OUTPUT", fmt) };
    }
}

/// Pre-scan argv for `--profile <name>` or `--profile=<name>` before clap parsing.
fn scan_profile_from_args() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let mut i = 1; // skip argv[0] (binary name)
    while i < args.len() {
        if args[i] == "--profile" {
            return args.get(i + 1).cloned();
        }
        if let Some(val) = args[i].strip_prefix("--profile=") {
            return Some(val.to_string());
        }
        i += 1;
    }
    None
}
