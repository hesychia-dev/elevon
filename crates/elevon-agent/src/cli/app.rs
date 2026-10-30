use anyhow::Result;
use elevon_contracts::{
    cli::app::{AppCommands, DeploymentCommands},
    cli_verify_action_with_items,
    deploy::{AppStopPayload, log_stream_events},
};

use crate::{
    api::{
        db::models::{App, Deployment},
        state::SharedApiState,
        stream::ActionLogger,
    },
    cli::CliCommand,
    image::drain_app,
    proxy::types::DrainAppData,
    socket::UnixClient,
};

impl CliCommand for AppCommands {
    async fn run(command: Self) -> Result<()> {
        let unix_client = UnixClient::new()?;
        const BASE_PATH: &str = "/cli/apps/local";

        match command {
            AppCommands::List(params) => {
                let path = format!("{BASE_PATH}/list");
                let stream = unix_client
                    .client
                    .get(unix_client.resolve_url(&path))
                    .query(&params)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(stream).await?;
            }

            AppCommands::Deployment { subcommand } => {
                DeploymentCommands::run(subcommand).await?;
            }

            AppCommands::Stop(args) => {
                cli_verify_action_with_items(args.force, &args.apps, "This stops running app(s)")?;

                let payload = AppStopPayload {
                    project: None,
                    apps: args.apps,
                    force: args.force,
                };

                let path = format!("{BASE_PATH}/stop");
                let stream = unix_client
                    .client
                    .post(unix_client.resolve_url(&path))
                    .json(&payload)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(stream).await?;
            }
        }

        Ok(())
    }
}

impl CliCommand for DeploymentCommands {
    async fn run(command: Self) -> Result<()> {
        let unix_client = UnixClient::new()?;
        const BASE_PATH: &str = "/cli/deployments/local";

        match command {
            DeploymentCommands::List(params) => {
                let path = format!("{BASE_PATH}/list");
                let stream = unix_client
                    .client
                    .get(unix_client.resolve_url(&path))
                    .query(&params)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(stream).await?;
            }
        }

        Ok(())
    }
}

/// Marks running apps to be drained
pub async fn handle_app_stop(
    payload: AppStopPayload,
    state: SharedApiState,
    logger: &impl ActionLogger,
) -> Result<()> {
    let mut db = state.db.get();

    for app in payload.apps {
        let app = App::get_by_name_or_id(&mut db, &app, payload.project.clone()).await?;

        let mut active_deployment = Deployment::get_latest_deployment(&mut db, &app.id).await?;
        let Some(active_deployment) = active_deployment.as_mut() else {
            logger
                .log(format!("App '{}' is not running, skipping", app.name).into())
                .await?;
            continue;
        };

        if !payload.force {
            logger
                .log(format!(
                    "App '{}' is running and cant be stopped without being forced, use --force to force  it",
                    app.name
                ).into())
                .await?;
            continue;
        }

        let app_data = DrainAppData {
            deployment_id: active_deployment.id.to_string(),
            container_id: active_deployment.container_id.clone(),
            domain: active_deployment.domain.clone(),
            port: active_deployment.web.as_ref().map(|w| w.port),
        };

        let is_web_app = active_deployment.web.is_some();

        drain_app(&state, &mut db, active_deployment, app_data).await?;

        let log_msg = match is_web_app {
            true => format!(
                "App '{}' was marked to be drained and will be stopped soon",
                app.name
            ),
            false => format!("App '{}' was stopped", app.name),
        };

        logger.log(log_msg.into()).await?;
    }

    Ok(())
}
