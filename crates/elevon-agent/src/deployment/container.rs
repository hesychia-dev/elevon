use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use bollard::{
    auth::DockerCredentials,
    plugin::{
        ContainerCreateBody, ContainerInspectResponse, ContainerStateStatusEnum, HealthStatusEnum,
        HostConfig, PortBinding, PortMap, RestartPolicy,
    },
    query_parameters::{CreateContainerOptionsBuilder, CreateImageOptionsBuilder},
};
use elevon_contracts::deploy::{
    AppPayload, RegistryVars, StreamEvent, config::AppHealthCheckConfig,
};
use futures_util::TryStreamExt;
use tokio::time::Instant;

use crate::api::{
    db::models::{Deployment, DeploymentStatus, deployment::DeploymentHealthCheck},
    stream::{ActionLogger, emit},
};

use super::ActionState;

pub(super) async fn pull_image(
    action_state: &ActionState<'_>,
    image_reference: &str,
    app_vars: &mut HashMap<String, String>,
) -> Result<()> {
    if let Ok(_) = action_state.docker.inspect_image(image_reference).await {
        app_vars.remove(RegistryVars::KEY_SERVER);
        app_vars.remove(RegistryVars::KEY_USERNAME);
        app_vars.remove(RegistryVars::KEY_PASSWORD);

        emit(
            action_state.tx,
            StreamEvent::log(format!("[{}] Using cached image", action_state.app_name)),
        )
        .await;

        return Ok(());
    }

    let registry_vars = RegistryVars::get_from_vars(app_vars)?;

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
        && let (Some(internal_port), Some(exposed_port)) = (routing.port, new_port)
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

pub(super) async fn check_health(
    action_state: &ActionState<'_>,
    container_id: &str,
    healthcheck: &AppHealthCheckConfig,
) -> Result<DeploymentStatus> {
    emit(
        action_state.tx,
        StreamEvent::log(format!(
            "[{}] Checking container health. interval = {}s, stabilization = {}s",
            action_state.app_name, healthcheck.interval, healthcheck.stabilization
        )),
    )
    .await;

    check_container_health(
        &action_state.docker,
        container_id,
        Some(action_state.app_name),
        &DeploymentHealthCheck::from(healthcheck),
        action_state.tx,
    )
    .await?;

    Ok(DeploymentStatus::Active)
}

pub async fn check_container_state(
    docker: &bollard::Docker,
    container_id: &str,
    is_fresh_deployment: bool,
) -> DeploymentStatus {
    let Ok(container) = docker.inspect_container(container_id, None).await else {
        return DeploymentStatus::Failed;
    };

    let Some(state) = container.state else {
        return DeploymentStatus::Failed;
    };
    let Some(status) = state.status else {
        return DeploymentStatus::Failed;
    };

    if is_fresh_deployment {
        return match status {
            ContainerStateStatusEnum::RUNNING => DeploymentStatus::Active,
            _ => DeploymentStatus::Failed,
        };
    }

    match status {
        ContainerStateStatusEnum::RUNNING => DeploymentStatus::Active,
        ContainerStateStatusEnum::RESTARTING
        | ContainerStateStatusEnum::STOPPING
        | ContainerStateStatusEnum::EXITED => DeploymentStatus::Failed,
        _ => DeploymentStatus::Drained,
    }
}

async fn check_container_health(
    docker: &bollard::Docker,
    container_id: &str,
    app_name: Option<&str>,
    healthcheck: &DeploymentHealthCheck,
    logger: &impl ActionLogger,
) -> Result<()> {
    let log_id = app_name.unwrap_or(container_id);
    wait_for_health(log_id, healthcheck, logger, || async {
        let container = is_container_healthy(docker, container_id).await?;
        is_container_path_healthy(healthcheck, &container).await
    })
    .await
}

async fn wait_for_health<F, Fut>(
    log_id: &str,
    healthcheck: &DeploymentHealthCheck,
    logger: &impl ActionLogger,
    mut check: F,
) -> Result<()>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<()>>,
{
    if healthcheck.interval == 0
        || healthcheck.timeout == 0
        || healthcheck.retries == 0
        || healthcheck.required_successes == 0
    {
        bail!(
            "Health check interval, timeout, retries and required successes must be greater than zero"
        );
    }

    let start_time = Instant::now();
    let start_period = Duration::from_secs(healthcheck.start_period);
    let interval = Duration::from_secs(healthcheck.interval);
    let timeout = Duration::from_secs(healthcheck.timeout);
    let stabilization = Duration::from_secs(healthcheck.stabilization);

    let mut stable_since = None;
    let mut consecutive_successes = 0;
    let mut failures = 0;

    // TODO: First check for path fails, I think due to the container not being ready
    loop {
        let result = tokio::time::timeout(timeout, check())
            .await
            .context("Health check timed out")
            .and_then(|result| result);

        match result {
            Ok(()) => {
                consecutive_successes += 1;
                let since = stable_since.get_or_insert_with(Instant::now);

                if consecutive_successes >= healthcheck.required_successes
                    && since.elapsed() >= stabilization
                {
                    logger
                        .log(format!("[{log_id}] Container stabilized successfully").into())
                        .await?;
                    return Ok(());
                }
            }
            Err(err) => {
                stable_since = None;
                consecutive_successes = 0;

                if start_time.elapsed() >= start_period {
                    failures += 1;
                }

                logger
                    .log(
                        StreamEvent::warn(format!(
                            "[{log_id}] Health check failed ({failures}/{}): {err:#}",
                            healthcheck.retries
                        ))
                        .into(),
                    )
                    .await?;

                if failures >= healthcheck.retries {
                    return Err(err.context("Container failed to pass health check"));
                }
            }
        }

        tokio::time::sleep(interval).await;
    }
}

async fn is_container_healthy(
    docker: &bollard::Docker,
    container_id: &str,
) -> Result<ContainerInspectResponse> {
    let container = docker
        .inspect_container(container_id, None)
        .await
        .context("Failed to inspect container")?;
    let state = container
        .state
        .as_ref()
        .context("Container state is missing")?;

    // Check Docker's health status if a healthcheck is configured
    if let Some(health) = &state.health
        && let Some(status) = health.status
        && !matches!(status, HealthStatusEnum::HEALTHY)
    {
        bail!("Docker health check reports {status:?}");
    }

    let is_running_active = matches!(
        (
            state.running,
            state.restarting,
            state.dead,
            state.oom_killed
        ),
        (
            Some(true),
            Some(false) | None,
            Some(false) | None,
            Some(false) | None
        )
    );

    let is_running_status = matches!(state.status, Some(ContainerStateStatusEnum::RUNNING));

    if is_running_active && is_running_status {
        Ok(container)
    } else {
        bail!("Container is not running");
    }
}

async fn is_container_path_healthy(
    healthcheck: &DeploymentHealthCheck,
    container: &ContainerInspectResponse,
) -> Result<()> {
    let Some(path) = &healthcheck.path else {
        return Ok(());
    };

    let binding = container
        .network_settings
        .as_ref()
        .and_then(|network| network.ports.as_ref())
        .and_then(|ports| {
            ports
                .values()
                .filter_map(|bindings| bindings.as_ref())
                .flat_map(|bindings| bindings.iter())
                .find(|binding| binding.host_port.is_some())
        })
        .context("HTTP health check has no host port binding")?;

    let host: IpAddr = binding
        .host_ip
        .as_deref()
        .context("Host IP is missing")?
        .parse()
        .context("Invalid host IP")?;
    let port: u16 = binding
        .host_port
        .as_deref()
        .context("Host port is missing")?
        .parse()
        .context("Invalid host port")?;

    if port == 0 {
        bail!("Invalid host port 0");
    }

    let host = match host {
        IpAddr::V4(ip) if ip.is_unspecified() => IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(ip) if ip.is_unspecified() => IpAddr::V6(Ipv6Addr::LOCALHOST),
        ip => ip,
    };

    let url = format!(
        "http://{}/{}",
        SocketAddr::new(host, port),
        path.trim_start_matches('/')
    );
    let response = reqwest::get(url)
        .await
        .context("HTTP health check request failed")?;

    if response.status().is_client_error() || response.status().is_server_error() {
        bail!("HTTP health check returned {}", response.status());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::*;
    use crate::api::stream::LogInput;

    struct TestLogger;

    #[async_trait::async_trait]
    impl ActionLogger for TestLogger {
        async fn log(&self, _: LogInput) -> Result<()> {
            Ok(())
        }
    }

    fn healthcheck() -> DeploymentHealthCheck {
        DeploymentHealthCheck {
            start_period: 0,
            interval: 1,
            timeout: 1,
            retries: 1,
            stabilization: 0,
            required_successes: 1,
            path: None,
        }
    }

    #[tokio::test]
    async fn path_check_is_optional_but_requires_a_binding_when_configured() {
        let container = ContainerInspectResponse::default();
        let mut config = healthcheck();

        assert!(is_container_path_healthy(&config, &container).await.is_ok());

        config.path = Some("/health".into());
        assert!(
            is_container_path_healthy(&config, &container)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn retries_during_start_period_do_not_consume_the_failure_limit() {
        let mut config = healthcheck();
        config.start_period = 1;
        let checks = Arc::new(AtomicUsize::new(0));
        let counter = checks.clone();

        wait_for_health("test", &config, &TestLogger, move || {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    bail!("not ready");
                }

                Ok(())
            }
        })
        .await
        .unwrap();

        assert_eq!(checks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn stabilization_rechecks_health_and_fails_if_it_deteriorates() {
        let mut config = healthcheck();
        config.stabilization = 1;
        let checks = Arc::new(AtomicUsize::new(0));
        let counter = checks.clone();

        let result = wait_for_health("test", &config, &TestLogger, move || {
            let attempt = counter.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt > 0 {
                    bail!("unhealthy");
                }

                Ok(())
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(checks.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn health_checks_time_out() {
        let result = wait_for_health("test", &healthcheck(), &TestLogger, || {
            std::future::pending::<Result<()>>()
        })
        .await;

        assert!(format!("{:#}", result.unwrap_err()).contains("timed out"));
    }

    #[tokio::test]
    async fn rejects_invalid_health_check_settings() {
        let mut config = healthcheck();
        config.interval = 0;

        let result = wait_for_health("test", &config, &TestLogger, || async { Ok(()) }).await;
        assert!(result.is_err());
    }
}
