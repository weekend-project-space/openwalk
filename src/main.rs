mod app;
mod browser;
mod cli;
mod output;
mod scheme_runtime;
mod tool_hub;
mod tool_metadata;
mod workspace;

use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> ExitCode {
    // Keep process concerns in `main`; command behavior and error construction live in `app`.
    let cli = Cli::parse();

    match app::run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}
