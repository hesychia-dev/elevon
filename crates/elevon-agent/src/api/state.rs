use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use bollard::plugin::{ContainerInspectResponse, ContainerStateStatusEnum, HealthStatusEnum};
use elevon_contracts::deploy::{StaticApp, StreamEvent, WebApp};
use tokio::sync::RwLock;

use crate::api::db::AgentDb;
use crate::api::db::models::deployment::DeploymentHealthCheck;
use crate::api::db::models::{Deployment, DeploymentStatus};
use crate::api::stream::{ActionLogger, OptionLoggerExt};
use crate::env::ElevonEnv;
use crate::proxy::types::DeployAppData;
use crate::socket::{Socket, SocketType};

pub type SharedApiState = Arc<ApiState>;

pub struct ApiState {
    pub db: AgentDb,
    pub docker: bollard::Docker,
    pub env: RwLock<ElevonEnv>,
    pub proxy_socket: Socket,
}

impl ApiState {
    pub async fn new(env: &ElevonEnv) -> Result<Self> {
        let db = AgentDb::new(env.turso_remote_url.as_deref()).await?;
        let docker = bollard::Docker::connect_with_defaults()?;
        let env = RwLock::new(env.clone());

        let proxy_socket = Socket::new(SocketType::Proxy, true)?;

        let state = Self {
            db,
            docker,
            env,
            proxy_socket,
        };

        state.check_running_containers().await?;

        Ok(state)
    }

    async fn check_running_containers(&self) -> Result<()> {
        // This is going to update the values anyway so we can just use it
        self.get_running_route_containers().await?;
        Ok(())
    }

    // TODO: Fix it so it also returns non web app containers and puts them into `runtime`
    pub async fn get_running_route_containers(&self) -> Result<Vec<DeployAppData>> {
        let mut db = self.db.get();

        let deployments = Deployment::filter(
            Deployment::fields()
                .status()
                .eq(DeploymentStatus::Active)
                .or(Deployment::fields().status().eq(DeploymentStatus::Pending)),
        )
        .include(Deployment::fields().app())
        .exec(&mut db)
        .await?;

        let mut routes: Vec<DeployAppData> = Vec::new();

        for mut deployment in deployments {
            let app = deployment.app.get();

            let Some(container_id) = deployment.container_id.clone() else {
                continue;
            };

            if deployment.web.is_none() && deployment.static_web.is_none() {
                continue;
            }

            let (web_deployment, static_web_deployment) =
                (deployment.web.clone(), deployment.static_web.clone());

            let app_name = app.name.clone();
            let project_name = app.project.clone();

            let deployment_status = self.check_container_state(&container_id).await;
            if deployment_status != DeploymentStatus::Active {
                deployment
                    .update()
                    .status(deployment_status)
                    .exec(&mut db)
                    .await?;

                // TODO: Maybe add a debug log or something

                continue;
            }

            let route_config = DeployAppData {
                id: deployment.id.to_string(),
                project: project_name,
                name: app_name,
                status: deployment_status,
                container_id: Some(container_id),
                web_app: web_deployment.map(|w| WebApp::from(&w)),
                static_app: static_web_deployment.map(|s| StaticApp::from(&s)),
            };

            routes.push(route_config);
        }

        Ok(routes)
    }

    pub async fn check_container_state(&self, container_id: &str) -> DeploymentStatus {
        let Ok(container) = self.docker.inspect_container(container_id, None).await else {
            return DeploymentStatus::Failed;
        };

        let Some(state) = container.state else {
            return DeploymentStatus::Failed;
        };
        let Some(status) = state.status else {
            return DeploymentStatus::Failed;
        };

        match status {
            ContainerStateStatusEnum::RUNNING => DeploymentStatus::Active,
            ContainerStateStatusEnum::RESTARTING
            | ContainerStateStatusEnum::STOPPING
            | ContainerStateStatusEnum::EXITED => DeploymentStatus::Failed,
            _ => DeploymentStatus::Drained,
        }
    }

    async fn is_container_healthy(&self, container_id: &str) -> Option<ContainerInspectResponse> {
        let Ok(container) = self.docker.inspect_container(container_id, None).await else {
            return None;
        };

        let Some(state) = &container.state else {
            return None;
        };

        if let Some(health) = &state.health
            && let Some(status) = health.status
        {
            if matches!(
                status,
                HealthStatusEnum::HEALTHY | HealthStatusEnum::STARTING
            ) {
                return Some(container);
            } else {
                return None;
            }
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
            Some(container)
        } else {
            None
        }
    }

    pub async fn check_container_health(
        &self,
        container_id: &str,
        app_name: Option<&str>,
        healthcheck: &DeploymentHealthCheck,
        logger: Option<&(dyn ActionLogger + Send + Sync)>,
    ) -> Result<()> {
        let log_id = app_name.unwrap_or(container_id);

        let start_period = Duration::from_secs(healthcheck.start_period);
        let timeout_duration = Duration::from_secs(healthcheck.timeout);
        let interval_duration = Duration::from_secs(healthcheck.interval);
        let stabilization_duration = Duration::from_secs(healthcheck.stabilization);

        let start_time = Instant::now();
        let mut consecutive_successes = 0;
        let mut attempt = 0;

        while attempt < healthcheck.retries {
            attempt += 1;

            let check_result =
                tokio::time::timeout(timeout_duration, self.is_container_healthy(container_id))
                    .await;

            let container = match check_result {
                Ok(Some(c)) => c,
                _ => {
                    consecutive_successes = 0;
                    logger.log_event(StreamEvent::warn(
                        format!(
                        "[{log_id}] Health check failed or timed out; resetting stability counter (attempt {attempt}/{})",
                        healthcheck.retries
                    )).into()).await?;
                    continue;
                }
            };

            consecutive_successes += 1;

            // Check the HTTP path health
            let path_healthy = is_container_path_healthy(log_id, healthcheck, logger, &container)
                .await
                .unwrap_or_default();

            if !path_healthy {
                consecutive_successes = 0;

                if start_time.elapsed() < start_period {
                    attempt -= 1;
                    tokio::time::sleep(interval_duration).await;
                    continue;
                }

                logger.log_event(
                            format!("[{log_id}] Container path health check failed, retrying (retries {}/{})...",
                                attempt,
                                healthcheck.retries
                        ).into()
                    ).await?;

                // Intervals are between or after faifed checks so...
                if attempt < healthcheck.retries {
                    tokio::time::sleep(interval_duration).await;
                }

                continue;
            }

            logger
                .log_event(
                    format!(
                        "[{log_id}] Healthcheck passed. Waiting for the container to stabilize"
                    )
                    .into(),
                )
                .await?;

            // Sleeping for the <stabilization_duration> just to wait it out.
            // TODO: Also we could check the health once again after this.
            if attempt < healthcheck.retries {
                tokio::time::sleep(stabilization_duration).await;
            }

            if start_time.elapsed() >= stabilization_duration
                && consecutive_successes >= healthcheck.required_successes
            {
                logger
                    .log_event(format!("[{log_id}] Container stabilized successfully").into())
                    .await?;

                return Ok(());
            }
        }

        logger
            .log_event(
                format!(
                    "[{log_id}] Container achieved stability threshold ({consecutive_successes}/{})",
                    healthcheck.required_successes
                ).into()
            )
            .await?;

        // TODO: This might hang or leave resources if Docker is unresponsive.
        if let Err(err) = self.docker.stop_container(container_id, None).await {
            logger
                .log_event(
                    StreamEvent::warn(format!(
                        "[{log_id}] Failed to stop container {container_id}: {err}"
                    ))
                    .into(),
                )
                .await?;
            anyhow::bail!("failed to stop container {container_id}: {err}");
        }

        anyhow::bail!("Container failed to pass health check")
    }
}

async fn is_container_path_healthy(
    log_id: &str,
    healthcheck: &DeploymentHealthCheck,
    logger: Option<&(dyn ActionLogger + Send + Sync)>,
    container: &ContainerInspectResponse,
) -> Result<bool> {
    if let Some(healthcheck_path) = &healthcheck.path {
        if let Some(network_settings) = &container.network_settings
            && let Some(ports) = &network_settings.ports
        {
            // This is a bit tricky but currently
            // a container can be binded to only one host/port, and the host is always 0.0.0.0 currently.
            if let Some(first_port) = ports.iter().next()
                && let Some(bindings) = first_port.1
                && let Some(first_bind) = bindings.first()
                && let Some(host_ip) = &first_bind.host_ip
                && let Some(host_port) = &first_bind.host_port
            {
                let healthcheck_url = format!(
                    "http://{}:{}/{}",
                    host_ip,
                    host_port,
                    healthcheck_path
                        .strip_prefix("/")
                        .unwrap_or(healthcheck_path)
                );

                let res = reqwest::get(healthcheck_url.clone())
                    .await
                    .map_err(|e| anyhow::anyhow!("path healthcheck failed: {}", e))?;

                let status = res.status();
                if status.is_server_error() || status.is_client_error() {
                    logger
                        .log_event(
                            StreamEvent::Error {
                                message: format!(
                                    "[{log_id}] Container path healthcheck failed. status: {}",
                                    status
                                ),
                            }
                            .into(),
                        )
                        .await?;
                    return Ok(false);
                }
            }
        } else {
            tracing::error!(
                "[{log_id}] Received healthcheck path but container network settings dont match: {:?}",
                container.network_settings
            );
        }
    }

    Ok(true)
}
