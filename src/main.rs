use anyhow::Result;
use clap::Parser;

use tracebox::cli::{Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            parent,
            tool_kind,
            command,
        } => {
            let exit_code =
                tracebox::commands::run::execute(cli.trace_root, parent, tool_kind, command)?;

            // Tracebox should behave like a transparent command wrapper:
            // if the wrapped command fails, the Tracebox process should fail
            // with the same exit code. The trace is still written.
            std::process::exit(exit_code);
        }

        Commands::Inspect {
            trace_id,
            stdout,
            stderr,
            tail,
            json,
        } => {
            tracebox::commands::inspect::execute(
                cli.trace_root,
                trace_id,
                stdout,
                stderr,
                tail,
                json,
            )?;
        }

        Commands::Verify { trace_id, json } => {
            let exit_code = tracebox::commands::verify::execute(cli.trace_root, trace_id, json)?;
            std::process::exit(exit_code);
        }

        Commands::Validate { trace_id, json } => {
            let exit_code = tracebox::commands::validate::execute(cli.trace_root, trace_id, json)?;
            std::process::exit(exit_code);
        }

        Commands::List { json } => {
            tracebox::commands::list::execute(cli.trace_root, json)?;
        }

        Commands::Report { trace_id, output } => {
            tracebox::commands::report::execute(cli.trace_root, trace_id, output)?;
        }

        Commands::Diff { left, right, json } => {
            tracebox::commands::diff::execute(cli.trace_root, left, right, json)?;
        }
    }

    Ok(())
}
