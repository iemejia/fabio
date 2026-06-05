#![recursion_limit = "512"]

mod cli;
mod client;
mod commands;
mod errors;
mod output;
mod parallel;
mod token_cache;

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

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    let result: Result<()> = runtime.block_on(Box::pin(commands::execute(cli)));

    match result {
        Ok(()) => Ok(()),
        Err(e) => {
            if let Some(fabio_err) = e.downcast_ref::<errors::FabioError>() {
                output::render_error(fabio_err);
                return Err(1);
            }
            // Unexpected error - still render as structured JSON
            let fabio_err = errors::FabioError::new(errors::ErrorCode::Unknown, e.to_string());
            output::render_error(&fabio_err);
            Err(1)
        }
    }
}
