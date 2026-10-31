use anyhow::{Context, Result};
use clap::Parser;
use elevon_agent::{
    cli::{Cli, CliCommand, Commands, key::KeyCommands},
    env::ElevonEnv,
};
use elevon_contracts::cli::app::AppCommands;
use elevon_http::runtime::run_async;

fn main() {
    if let Err(err) = run() {
        tracing::error!("{err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let with_level = matches!(cli.command, Commands::Api(_) | Commands::Proxy);
    elevon_http::init_cli_logging(with_level);

    if matches!(cli.command, Commands::Init) {
        return elevon_agent::cli::init::run_init().context("init command failed");
    }

    if let Commands::Install(args) = cli.command {
        return run_async(elevon_agent::cli::install::run_install(cli.args, args))
            .context("install command failed");
    }

    // This still needs the current user to be in `elevon-agent` group to run without sudo.
    if let Commands::Key { subcommand } = cli.command {
        return run_async(KeyCommands::run(subcommand)).context("key command failed");
    }

    if let Commands::App { subcommand } = cli.command {
        return run_async(AppCommands::run(subcommand)).context("app command failed");
    }

    if let Commands::Upgrade(args) = cli.command {
        return run_async(elevon_agent::cli::install::upgrade(args))
            .context("upgrade command failed");
    }

    let env = ElevonEnv::load().context("failed to load agent environment")?;

    match cli.command {
        Commands::Init => unreachable!(),
        Commands::Install(_) => unreachable!(),
        Commands::Upgrade(_) => unreachable!(),
        Commands::Key { subcommand: _ } => unreachable!(),
        Commands::App { subcommand: _ } => unreachable!(),

        Commands::Uninstall(args) => {
            elevon_agent::cli::install::run_uninstall(args).context("uninstall command failed")?;
        }

        Commands::Api(args) => {
            run_async(elevon_agent::api::run_api_server(args, &env))
                .context("api command failed")?;
        }

        Commands::Proxy => {
            elevon_agent::proxy::run_proxy(&env, false).context("proxy command failed")?;
        }
    }

    Ok(())
}
