use std::sync::Arc;

use anyhow::{Context, Result};
use bollard::query_parameters::RemoveContainerOptions;
use elevon_contracts::deploy::config::AppRoutingPayload;
use elevon_fs::agent::{AgentPath, AppEnvOptions, remove_app_files};
use tokio::sync::RwLock;

use crate::api::db::AgentDb;
use crate::api::db::models::{Deployment, DeploymentStatus};
use crate::deployment::container::check_container_state;
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

        let proxy_socket = Socket::new(SocketType::Proxy, false)?;

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
        let mut db = self.db.get();
        let pending =
            Deployment::filter(Deployment::fields().status().eq(DeploymentStatus::Pending))
                .include(Deployment::fields().app())
                .exec(&mut db)
                .await?;

        for mut deployment in pending {
            let app_name = deployment.app.get().name.clone();
            let name = format!("{}-{}", app_name, deployment.id);
            let temporary_name = format!("{name}-tmp");

            for container in [name.as_str(), temporary_name.as_str()] {
                let options = RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                };

                match self.docker.remove_container(container, Some(options)).await {
                    Ok(())
                    | Err(bollard::errors::Error::DockerResponseServerError {
                        status_code: 404,
                        ..
                    }) => {}
                    Err(err) => {
                        return Err(err)
                            .context("Failed to remove interrupted deployment container");
                    }
                }
            }

            let app_id = deployment.app_id.to_string();
            let deployment_id = deployment.id.to_string();
            let dir = AgentPath::AppDeploymentDir(app_id.clone(), deployment_id.clone()).resolve();
            if dir.exists() {
                remove_app_files(AppEnvOptions::app(app_id, deployment_id))
                    .context("Failed to remove interrupted deployment files")?;
            }

            deployment
                .update()
                .status(DeploymentStatus::Failed)
                .exec(&mut db)
                .await?;
        }

        self.get_running_route_containers().await?;
        Ok(())
    }

    pub async fn get_running_route_containers(&self) -> Result<Vec<DeployAppData>> {
        let mut db = self.db.get();

        let deployments =
            Deployment::filter(Deployment::fields().status().eq(DeploymentStatus::Active))
                .include(Deployment::fields().app())
                .exec(&mut db)
                .await?;

        let mut routes: Vec<DeployAppData> = Vec::new();

        for mut deployment in deployments {
            let app = deployment.app.get();

            let deployment_status = if let Some(container_id) = &deployment.container_id {
                check_container_state(&self.docker, container_id, false).await
            } else if deployment
                .routing
                .as_ref()
                .is_some_and(|routing| routing.static_files.is_some())
            {
                deployment.status.clone()
            } else {
                DeploymentStatus::Failed
            };

            if deployment_status != DeploymentStatus::Active {
                deployment
                    .update()
                    .status(deployment_status)
                    .exec(&mut db)
                    .await?;

                // TODO: Maybe add a debug log or something

                continue;
            }

            if deployment.routing.is_none() {
                continue;
            }

            let route_config = DeployAppData {
                app_id: app.id.to_string(),
                deployment_id: deployment.id.to_string(),
                status: deployment_status,
                container_id: deployment.container_id.clone(),
                routing: deployment.routing.as_ref().map(AppRoutingPayload::from),
            };

            routes.push(route_config);
        }

        Ok(routes)
    }
}
