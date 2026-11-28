use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use elevon_config::ElevonConfig;
use elevon_fs::agent::{install_api_unit, install_proxy_unit};

use crate::{
    api::db::{AgentDb, models::AuthKey},
    cli::{CliArgs, InstallArgs, UninstallArgs, UpgradeArgs},
    config::Config,
    env::ElevonEnv,
};

const TEMP_FILE_CONFIG: &str = "/etc/tmpfiles.d/elevon-agent.conf";

fn check_before_installation() -> Result<()> {
    let docker_version_output = Command::new("docker")
        .arg("--version")
        .output()
        .context("failed to execute `docker --version`")?;

    if !docker_version_output.status.success() {
        bail!(
            "`docker --version` failed with {}",
            docker_version_output.status
        );
    }

    Ok(())
}

fn create_elevon_agent_user() -> Result<()> {
    let user_exists = Command::new("getent")
        .args(["passwd", "elevon-agent"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to execute `getent passwd elevon-agent`")?
        .success();

    if user_exists {
        return Ok(());
    }

    let status = Command::new("useradd")
        .args([
            "--system",
            "--home-dir",
            "/var/lib/elevon-agent",
            "--no-create-home",
            "--shell",
            "/usr/sbin/nologin",
            "elevon-agent",
        ])
        .status()
        .context("failed to execute `useradd elevon-agent`")?;

    if !status.success() {
        bail!("`useradd elevon-agent` failed with {status}");
    }

    Ok(())
}

fn ensure_agent_state_ownership() -> Result<()> {
    let status = Command::new("chown")
        .args(["-R", "elevon-agent:elevon-agent", "/var/lib/elevon-agent"])
        .status()
        .context("failed to assign ownership of /var/lib/elevon-agent")?;

    if !status.success() {
        bail!("failed to assign ownership of /var/lib/elevon-agent: {status}");
    }

    Ok(())
}

fn run_tmpfiles() -> Result<()> {
    std::fs::write(
        TEMP_FILE_CONFIG,
        "d /run/elevon-agent 0750 elevon-agent elevon-agent -\n",
    )?;

    let status = Command::new("systemd-tmpfiles")
        .args(["--create", TEMP_FILE_CONFIG])
        .status()
        .context("failed to create agent runtime directory")?;

    if !status.success() {
        bail!("systemd-tmpfiles failed with {status}");
    }

    Ok(())
}

fn run_systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl")
        .args(args)
        .status()
        .with_context(|| format!("failed to run systemctl {}", args.join(" ")))?;

    if !status.success() {
        bail!("systemctl {} failed with {status}", args.join(" "));
    }
    Ok(())
}

// create elevon-agent user
// initialize config/database/files
// chown /var/lib/elevon-agent to elevon-agent
// write units
// daemon-reload
// start services
pub async fn run_install(cli_args: CliArgs, args: InstallArgs) -> Result<()> {
    if !args.no_systemd {
        check_before_installation()?;
        create_elevon_agent_user()?;
    }

    let config = Config::from_file(cli_args.config).context("failed to load config")?;
    config.setup().context("failed to setup configuration")?;

    ElevonEnv::setup(config).context("failed to init agent env")?;

    let mut db = AgentDb::new(None)
        .await
        .context("failed to initialize the agent database")?
        .get();

    let auth_keys = AuthKey::all().exec(&mut db).await?;

    if !auth_keys.is_empty() {
        tracing::info!("Auth key already exist, run 'key list' to view the keys");
    } else {
        let auth_key = AuthKey::create_key(&mut db, "Default").await?;
        tracing::info!("Save your API Key: {}", auth_key);
    }

    if !args.no_systemd {
        ensure_agent_state_ownership()?;

        let agent = std::env::current_exe().context("failed to resolve current executable")?;

        tracing::info!("Creating systemd units in /etc/systemd/system/ ...");

        install_api_unit(&agent).context("failed to write elevon-agent-api.service")?;
        install_proxy_unit(&agent).context("failed to write elevon-agent-proxy.service")?;

        tracing::info!("Systemd units written");

        run_tmpfiles()?;

        run_systemctl(&["daemon-reload"])?;

        if args.enable {
            run_systemctl(&[
                "enable",
                "--now",
                "elevon-agent-api.service",
                "elevon-agent-proxy.service",
            ])?;
            tracing::info!("Services enabled and started");
        } else {
            tracing::info!(
                "Run `systemctl enable --now elevon-agent-api elevon-agent-proxy` when ready"
            );
        }
    }

    tracing::info!("Elevon Agent was installed and started successfully!");

    Ok(())
}

pub fn run_uninstall(args: UninstallArgs) -> Result<()> {
    // TODO: Stop any running containers

    tracing::info!("This will uninstall Elevon.");

    if !args.keep_systemd {
        tracing::info!("- systemd units");
    }
    if !args.keep_database {
        tracing::info!("- database");
    }
    if !args.keep_env_files {
        tracing::info!("- environment files");
    }

    elevon_contracts::handle_cli_yes(
        args.yes,
        "This will remove Elevon files and services. Continue? [y/N]",
    )?;

    elevon_fs::agent::remove_files(
        args.keep_database,
        args.keep_env_files,
        args.keep_systemd,
        TEMP_FILE_CONFIG,
    )
}

pub async fn upgrade(args: UpgradeArgs) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let version = args.version.unwrap_or("latest".to_string());

    tracing::info!("Current Elevon Agent version: {}", current_version);
    tracing::info!("Looking for {} version", version);

    let client = elevon_fs::upgrade::ReleaseClient::new("elevon-agent".to_string(), version)?;
    let destination = std::env::current_exe().context("failed to resolve current executable")?;

    let installed = client.install_binary(current_version, &destination).await?;

    if installed {
        tracing::info!(path = %destination.display(), "Agent binary updated");
    } else if args.reload_services {
        run_systemctl(&[
            "restart",
            "elevon-agent-api.service",
            "elevon-agent-proxy.service",
        ])?;
        tracing::info!("agent services restarted");
    }

    Ok(())
}
