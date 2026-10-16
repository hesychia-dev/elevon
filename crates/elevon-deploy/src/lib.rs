use anyhow::Context;
use elevon_config::{ElevonConfig, ResolveEnvCredentials};

use crate::{
    agent::AgentClient,
    cli::{Commands, DeployArgs},
    config::Config,
    util::resolve_config_path,
};

pub mod agent;
pub mod cli;
pub mod config;
pub mod image;
pub mod util;

pub async fn run_deploy_cli(
    arg: DeployArgs,
    command: Commands,
    cli_version: Option<&str>,
) -> anyhow::Result<()> {
    let with_level = matches!(command, Commands::Deploy(_) | Commands::Rollback(_));
    elevon_http::init_cli_logging(with_level);

    if matches!(command, Commands::Init) {
        return cli::init::run_init();
    }

    if let Commands::Upgrade(args) = command {
        return cli::init::run_upgrade(cli_version, args.version)
            .await
            .context("failed to upgrade cli");
    }

    let config_path = resolve_config_path(&arg.config, &arg.project);

    let config = Config::from_file(&config_path)?;
    let agent_credentials = config
        .elevon
        .agent
        .resolved_credentials()
        .context("failed to resolve agent credentials")?;

    let agent_client = AgentClient::new(&agent_credentials.url, &agent_credentials.key)
        .context("failed to create agent client")?;

    let registry_credentials = config
        .registry
        .resolved_credentials()
        .context("failed to resolve registry credentials")?;

    match command {
        Commands::Init => unreachable!(),
        Commands::Check => {
            tracing::info!("Successfully passed config file check {}", &config_path);
        }
        Commands::Upgrade(_) => unreachable!(),
        Commands::Build(args) => {
            config
                .run_build(&registry_credentials, &config_path, args.push)
                .await?;
        }
        Commands::Push => {
            config.run_push(&config_path).await?;
        }
        Commands::Deploy(args) => {
            config
                .run_deploy(agent_client, &args.apps, registry_credentials, &config_path)
                .await?;
        }
        Commands::Rollback(args) => {
            config.run_rollback(agent_client, args).await?;
        }
        Commands::Agent { command } => {
            command.handle(agent_client, config).await?;
        }
    }

    Ok(())
}
