use std::collections::HashMap;

use anyhow::Result;
use bollard::{
    auth::DockerCredentials,
    plugin::{ContainerCreateBody, HostConfig, PortBinding, PortMap, RestartPolicy},
    query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder},
};
use elevon_contracts::deploy::{AppPayload, RegistryVars, StreamEvent};
use futures_util::TryStreamExt;

use crate::api::{db::models::Deployment, stream::emit};

use super::ActionState;

pub(super) async fn pull_image(
    action_state: &ActionState<'_>,
    image_reference: &str,
    app_vars: &mut HashMap<String, String>,
) -> Result<()> {
    let registry_vars = RegistryVars::get_from_vars(app_vars)?;

    if let Ok(_) = action_state.docker.inspect_image(image_reference).await {
        emit(
            action_state.tx,
            StreamEvent::log(format!("[{}] Using cached image", action_state.app_name)),
        )
        .await;

        return Ok(());
    }

    emit(
        action_state.tx,
        StreamEvent::log(format!(
            "[{}] Pulling the image from registry",
            action_state.app_name
        )),
    )
    .await;

    let image_options = CreateImageOptionsBuilder::new()
        .from_image(image_reference)
        .build();

    let credentials = Some(DockerCredentials {
        username: Some(registry_vars.username),
        password: Some(registry_vars.password),
        serveraddress: Some(registry_vars.server),
        ..Default::default()
    });

    let mut pull_stream = action_state
        .docker
        .create_image(Some(image_options), None, credentials);

    while let Some(event) = pull_stream.try_next().await? {
        tracing::debug!(?event, "docker pull event");
    }

    emit(
        action_state.tx,
        StreamEvent::log(format!("[{}] Image was pulled", action_state.app_name)),
    )
    .await;

    Ok(())
}

pub(super) async fn create_container(
    action_state: &ActionState<'_>,
    deployment: &Deployment,
    app_config: &AppPayload,
    app_vars: &HashMap<String, String>,
    new_port: Option<u16>,
) -> Result<String> {
    if let Some(port) = new_port {
        emit(
            action_state.tx,
            StreamEvent::log(format!(
                "[{}] Starting running a container on port {}",
                app_config.name, port
            )),
        )
        .await;
    }

    let container_name = format!("{}-{}", app_config.name, deployment.id);
    let options = CreateContainerOptionsBuilder::new()
        .name(&container_name)
        .build();

    let app_env = app_vars
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();

    let mut port_bindings = PortMap::new();

    if let Some(routing) = &app_config.routing
        && let (Some(internal_port), Some(exposed_port)) = (routing.resolved_port, new_port)
    {
        port_bindings.insert(
            format!("{}/tcp", internal_port),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(exposed_port.to_string()),
            }]),
        );
    }

    let host_config = Some(HostConfig {
        restart_policy: Some(RestartPolicy {
            name: app_config.runtime_options.restart,
            ..Default::default()
        }),
        nano_cpus: app_config.runtime_options.cpu_limit,
        memory: app_config.runtime_options.memory_limit,
        network_mode: app_config.runtime_options.network.clone(),
        port_bindings: Some(port_bindings),
        ..Default::default()
    });

    let config = ContainerCreateBody {
        image: Some(app_config.image_ref.clone()),
        cmd: app_config.runtime_options.cmd.clone(),
        env: Some(app_env),
        host_config,
        ..Default::default()
    };

    let container = action_state
        .docker
        .create_container(Some(options), config)
        .await?;

    Ok(container.id)
}
