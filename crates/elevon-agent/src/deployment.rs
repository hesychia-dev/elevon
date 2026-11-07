pub mod container;
mod errors;

use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result, bail};
use bollard::{
    plugin::ContainerCreateBody,
    query_parameters::{
        CreateContainerOptionsBuilder, DownloadFromContainerOptions, RemoveContainerOptions,
    },
};
use elevon_contracts::deploy::{
    AppPayload, AppRole, AppRollbackPayload, AppRuntimeOptions, StreamEvent, TlsType,
    config::{AppHealthCheckConfig, AppRoutingPayload, StaticConfig},
};
use elevon_fs::agent::{
    AgentPath, AppEnvOptions,
    env::{add_app_env_vars, load_app_env},
    remove_app_files,
    tls::write_tls_file,
};
use futures_util::stream::StreamExt;
use tar::Archive;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    api::{
        db::models::{App, Deployment, DeploymentStatus},
        state::SharedApiState,
        stream::{StreamSender, emit},
    },
    proxy::types::{AgentEvent, DeployAppData, DrainAppData},
    socket::Socket,
};

static ALLOCATED_PORTS: LazyLock<RwLock<HashSet<u16>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

pub struct DeploymentService {
    docker: Arc<bollard::Docker>,
    db: toasty::Db,
    proxy_socket: Arc<Socket>,
}

impl DeploymentService {
    pub fn new(docker: Arc<bollard::Docker>, db: toasty::Db, proxy_socket: Arc<Socket>) -> Self {
        Self {
            docker,
            db,
            proxy_socket,
        }
    }

    pub async fn deploy(&self, tx: StreamSender, apps: Vec<AppPayload>) -> Result<()> {
        for app in apps {
            emit(
                &tx,
                StreamEvent::log(format!("[{}] Deploying...", app.name)),
            )
            .await;

            deploy(self, &tx, app).await?;
        }

        Ok(())
    }

    pub async fn rollback(&self, tx: StreamSender, payload: AppRollbackPayload) -> Result<()> {
        if !payload.apps.is_empty() && !payload.deployment_ids.is_empty() {
            bail!("Both apps and deployment IDs can't be passed");
        }

        let mut db = self.db.clone();

        for requested_app in payload.apps {
            let app =
                App::get_by_project_and_name(&mut db, &requested_app.project, &requested_app.name)
                    .await?
                    .with_context(|| format!("App '{}' not found", requested_app.name))?;

            let deployment = Deployment::get_previous_deployment(&mut db, &app.id)
                .await?
                .with_context(|| format!("No previous deployment for '{}'", app.name))?;

            self.rollback_deployment(&tx, deployment).await?;
        }

        for id in payload.deployment_ids {
            let id = Uuid::parse_str(&id).context("Invalid deployment ID")?;

            let deployment = Deployment::filter(Deployment::fields().id().eq(id))
                .include(Deployment::fields().app())
                .include(Deployment::fields().options())
                .first()
                .exec(&mut db)
                .await?
                .with_context(|| format!("Deployment '{id}' not found"))?;

            self.rollback_deployment(&tx, deployment).await?;
        }

        Ok(())
    }

    async fn rollback_deployment(&self, tx: &StreamSender, deployment: Deployment) -> Result<()> {
        use crate::api::db::models::app::AppRoleDb;

        if deployment.status != DeploymentStatus::Drained {
            bail!("Only drained deployments can be rolled back");
        }

        let app = deployment.app.get();

        let options = deployment
            .options
            .get()
            .as_ref()
            .context("Deployment options are missing")?;

        let role = match options.role {
            AppRoleDb::Web => AppRole::Web,
            AppRoleDb::Worker => AppRole::Worker,
        };

        let routing = deployment
            .routing
            .as_ref()
            .map(|routing| routing.container_payload());

        if role == AppRole::Web
            && routing
                .as_ref()
                .is_some_and(|routing| {
                    routing.static_files.is_none()
                        && (routing.port.is_none() || routing.port == Some(0))
                })
        {
            bail!("Cannot roll back web deployment: original container port is unavailable");
        }

        let is_static = routing
            .as_ref()
            .is_some_and(|routing| routing.static_files.is_some());

        let vars = if !is_static {
            let app_id = deployment.app_id.to_string();
            let deployment_id = deployment.id.to_string();
            let options = AppEnvOptions::app(app_id.clone(), deployment_id.clone());
            let path = AgentPath::AppEnv(app_id, deployment_id).resolve();

            if !path.exists() {
                bail!("Deployment environment file is missing");
            }

            load_app_env(options)?
        } else {
            HashMap::new()
        };

        let payload = AppPayload {
            project: app.project.clone(),
            name: app.name.clone(),
            image_ref: deployment.image_ref.clone(),
            keep_releases: app.keep_releases,
            role,
            runtime_options: AppRuntimeOptions::from(&options.runtime),
            healthcheck: AppHealthCheckConfig::from(&options.healthcheck),
            routing,
            vars,
        };

        rollback_deployment(self, tx, payload, deployment).await
    }
}

fn find_free_port() -> Option<u16> {
    for port in 3334..=9998 {
        match elevon_http::check_port(port) {
            Some(p) => return Some(p),
            None => continue,
        }
    }
    None
}

async fn get_free_port() -> Result<u16> {
    let mut ports = ALLOCATED_PORTS.write().await;
    let port = find_free_port().ok_or_else(|| anyhow::anyhow!("No free port found"))?;

    ports.insert(port);

    Ok(port)
}

async fn clear_port(port: &u16) {
    let mut ports = ALLOCATED_PORTS.write().await;
    ports.remove(port);
}

#[derive(Debug, Default)]
struct DeploymentResources {
    port: Option<u16>,
    container_id: Option<String>,
}

struct ActionState<'a> {
    docker: Arc<bollard::Docker>,
    tx: &'a StreamSender,
    app_name: &'a str,
}

async fn deploy(
    service: &DeploymentService,
    tx: &StreamSender,
    app_config: AppPayload,
) -> Result<()> {
    if app_config.role == AppRole::Worker && app_config.routing.is_some() {
        bail!("Worker can't have routing configuration");
    }

    if app_config.role == AppRole::Web && app_config.routing.is_none() {
        bail!("Web must have routing configuration");
    }

    if let Some(routing) = &app_config.routing
        && routing.static_files.is_none()
        && (routing.port.is_none() || routing.port == Some(0))
    {
        bail!("A web container requires a valid container port");
    }

    let action_state = ActionState {
        docker: service.docker.clone(),
        tx,
        app_name: &app_config.name,
    };

    let mut app_vars = app_config.vars.clone();

    container::pull_image(&action_state, &app_config.image_ref, &mut app_vars).await?;

    let mut db = service.db.clone();
    let mut db_clone = db.clone();
    let mut db_tx = db_clone.transaction().await?;

    let previous =
        match App::get_by_project_and_name(&mut db, &app_config.project, &app_config.name).await? {
            Some(app) => Deployment::get_latest_deployment(&mut db, &app.id).await?,
            None => None,
        };
    let mut deployment = prepare_deployment(&mut db_tx, &app_config).await?;

    db_tx.commit().await?;

    let mut resources = DeploymentResources::default();

    let result = async {
        prepare_app_files(
            &deployment,
            service.docker.clone(),
            &mut resources,
            &app_config.name,
            &app_config.image_ref,
            app_vars.clone(),
            app_config.routing.as_ref(),
        )
        .await
        .context("Failed to prepare app files")?;

        activate_deployment(
            service,
            &action_state,
            &mut deployment,
            &app_config,
            &mut resources,
            app_vars,
        )
        .await
    }
    .await;

    if let Err(err) = result {
        let failures = errors::cleanup_failed_deployment(
            &action_state,
            &mut db,
            &mut deployment,
            &mut resources,
        )
        .await;

        if !failures.is_empty() {
            return Err(err.context(format!("Cleanup failures: {}", failures.join("; "))));
        }

        return Err(err.context("Deployment failed"));
    }

    drain_previous(service, tx, &app_config.name, previous).await?;

    if let Err(err) = prune_old_releases(
        &service.docker,
        &mut db,
        deployment.app_id,
        app_config.keep_releases,
    )
    .await
    {
        emit(
            tx,
            StreamEvent::warn(format!(
                "[{}] Failed to prune old releases: {err:#}",
                app_config.name
            )),
        )
        .await;
    }

    Ok(())
}

async fn rollback_deployment(
    service: &DeploymentService,
    tx: &StreamSender,
    app_config: AppPayload,
    mut deployment: Deployment,
) -> Result<()> {
    let action_state = ActionState {
        docker: service.docker.clone(),
        tx,
        app_name: &app_config.name,
    };

    let is_static = app_config
        .routing
        .as_ref()
        .is_some_and(|routing| routing.static_files.is_some());

    let mut app_vars = app_config.vars.clone();

    if is_static {
        let path =
            AgentPath::AppStaticDir(deployment.app_id.to_string(), deployment.id.to_string())
                .resolve();

        if !path.is_dir() {
            bail!("Previous deployment's static files are missing");
        }
    } else {
        let name = format!("{}-{}", app_config.name, deployment.id);

        match service.docker.inspect_container(&name, None).await {
            Ok(_) => bail!("Previous deployment is still draining; try rollback again later"),
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => {}
            Err(err) => return Err(err).context("Failed to inspect previous container"),
        }

        container::pull_image(&action_state, &app_config.image_ref, &mut app_vars).await?;
    }

    let mut db = service.db.clone();
    let previous = Deployment::get_latest_deployment(&mut db, &deployment.app_id).await?;
    let mut resources = DeploymentResources::default();

    let result = activate_deployment(
        service,
        &action_state,
        &mut deployment,
        &app_config,
        &mut resources,
        app_vars,
    )
    .await;

    if let Err(err) = result {
        let failures = errors::cleanup_failed_rollback(
            &action_state,
            &mut db,
            &mut deployment,
            &mut resources,
        )
        .await;

        if !failures.is_empty() {
            return Err(err.context(format!("Cleanup failures: {}", failures.join("; "))));
        }

        return Err(err.context("Rollback failed"));
    }

    drain_previous(service, tx, &app_config.name, previous).await
}

async fn drain_previous(
    service: &DeploymentService,
    tx: &StreamSender,
    app_name: &str,
    previous: Option<Deployment>,
) -> Result<()> {
    if let Some(mut previous) = previous {
        emit(
            tx,
            StreamEvent::log(format!("[{}] Draining previous deployment", app_name)),
        )
        .await;

        let drain_data = DrainAppData {
            deployment_id: previous.id.to_string(),
            container_id: previous.container_id.clone(),
            domain: previous
                .routing
                .as_ref()
                .map(|routing| routing.domain.clone()),
            port: previous.routing.as_ref().and_then(|routing| routing.port),
        };

        service
            .proxy_socket
            .send(AgentEvent::DrainApp(drain_data))
            .await
            .context(
                "New deployment is active, but failed to request draining the previous deployment",
            )?;

        let mut db = service.db.clone();

        previous
            .update()
            .status(DeploymentStatus::Drained)
            .exec(&mut db)
            .await
            .context(
                "New deployment is active, but failed to mark the previous deployment as drained",
            )?;
    }

    Ok(())
}

async fn prepare_deployment(
    db_tx: &mut toasty::Transaction<'_>,
    app_config: &AppPayload,
) -> Result<Deployment> {
    let app = App::get_or_create(db_tx, app_config).await?;

    Ok(Deployment::create_deployment(db_tx, &app, app_config).await?)
}

async fn activate_deployment(
    service: &DeploymentService,
    action_state: &ActionState<'_>,
    deployment: &mut Deployment,
    app_config: &AppPayload,
    resources: &mut DeploymentResources,
    app_vars: HashMap<String, String>,
) -> Result<()> {
    let mut db = service.db.clone();

    let is_static = app_config
        .routing
        .as_ref()
        .is_some_and(|routing| routing.static_files.is_some());

    if !is_static {
        let new_port = match app_config.role {
            AppRole::Web => Some(get_free_port().await?),
            _ => None,
        };

        resources.port = new_port;

        let container_id = container::create_container(
            action_state,
            &deployment,
            &app_config,
            &app_vars,
            new_port,
        )
        .await
        .context("Failed to create container")?;

        resources.container_id = Some(container_id.clone());

        let mut routing = deployment.routing.clone();

        if let Some(routing) = &mut routing {
            routing.port = new_port;
        }

        deployment
            .update()
            .container_id(container_id.clone())
            .routing(routing)
            .exec(&mut db)
            .await?;

        service
            .docker
            .start_container(&container_id, None)
            .await
            .context("Failed to start container")?;

        let deployment_status =
            container::check_health(action_state, &container_id, &app_config.healthcheck)
                .await
                .context("Failed to check container health")?;

        deployment
            .update()
            .status(deployment_status)
            .exec(&mut db)
            .await?;
    } else {
        let mut routing = deployment.routing.clone();

        if let Some(routing) = &mut routing {
            routing.port = None;
        }

        deployment
            .update()
            .routing(routing)
            .status(DeploymentStatus::Active)
            .exec(&mut db)
            .await?;
    }

    if app_config.role == AppRole::Web {
        send_upsert_route(service, action_state, deployment)
            .await
            .context("Failed to send upsert route request")?;
    }

    Ok(())
}

/// Populates environment variables, tls certs and static app files
async fn prepare_app_files(
    deployment: &Deployment,
    docker: Arc<bollard::Docker>,
    resources: &mut DeploymentResources,
    app_name: &str,
    image_reference: &str,
    app_vars: HashMap<String, String>,
    routing_config: Option<&AppRoutingPayload>,
) -> Result<()> {
    let app_env_options =
        AppEnvOptions::app(deployment.app_id.to_string(), deployment.id.to_string());

    if let Some(routing) = routing_config {
        if let Some(tls) = routing.tls.as_ref() {
            write_tls_file(tls.cert.as_bytes(), TlsType::Cert, app_env_options.clone())?;
            write_tls_file(tls.key.as_bytes(), TlsType::Key, app_env_options.clone())?;
        }

        if let Some(static_files) = &routing.static_files {
            handle_static_app_deployment(
                &docker,
                resources,
                image_reference,
                app_name,
                deployment.app_id.to_string(),
                deployment.id.to_string(),
                &static_files,
            )
            .await?;

            // Returning early because static apps can't read environment variables
            return Ok(());
        }
    }

    add_app_env_vars(app_vars, app_env_options)?;

    Ok(())
}

async fn send_upsert_route(
    service: &DeploymentService,
    action_state: &ActionState<'_>,
    deployment: &Deployment,
) -> Result<()> {
    emit(
        action_state.tx,
        StreamEvent::log(format!(
            "[{}] Updating proxy routing for traffic",
            action_state.app_name
        )),
    )
    .await;

    let app_data = DeployAppData {
        app_id: deployment.app_id.to_string(),
        deployment_id: deployment.id.to_string(),
        container_id: deployment.container_id.clone(),
        status: DeploymentStatus::Active,
        routing: deployment
            .routing
            .as_ref()
            .map(|r| AppRoutingPayload::from(r)),
    };

    service
        .proxy_socket
        .send(AgentEvent::UpsertRoute(app_data.clone()))
        .await
}

pub async fn drain_app(
    state: &SharedApiState,
    db: &mut toasty::Db,
    deployment: &mut Deployment,
    app_data: DrainAppData,
) -> Result<()> {
    state
        .proxy_socket
        .send(AgentEvent::DrainApp(app_data))
        .await?;

    deployment
        .update()
        .status(DeploymentStatus::Drained)
        .exec(db)
        .await?;

    Ok(())
}

async fn prune_old_releases(
    docker: &bollard::Docker,
    db: &mut toasty::Db,
    app_id: Uuid,
    keep_releases: u8,
) -> Result<()> {
    let deployments =
        Deployment::list_by_app_id(db, &app_id, u16::MAX as usize, keep_releases as usize).await?;

    for mut old_dep in deployments {
        if matches!(
            old_dep.status,
            DeploymentStatus::Active | DeploymentStatus::Pending | DeploymentStatus::Pruned
        ) {
            continue;
        }

        if old_dep.status == DeploymentStatus::Drained && old_dep.container_id.is_none() {
            continue;
        }

        if let Some(container_id) = &old_dep.container_id {
            match docker.inspect_container(container_id, None).await {
                Ok(_) => continue,
                Err(bollard::errors::Error::DockerResponseServerError {
                    status_code: 404, ..
                }) => {}
                Err(err) => return Err(err.into()),
            }
        }

        let dir = AgentPath::AppDeploymentDir(app_id.to_string(), old_dep.id.to_string()).resolve();
        if dir.exists() {
            let options = AppEnvOptions::app(app_id.to_string(), old_dep.id.to_string());
            remove_app_files(options).context("Failed to remove app files")?;
        }

        old_dep
            .update()
            .status(DeploymentStatus::Pruned)
            .exec(db)
            .await?;
    }

    Ok(())
}

async fn handle_static_app_deployment(
    docker: &bollard::Docker,
    resources: &mut DeploymentResources,
    image_reference: &str,
    app_name: &str,
    app_id: String,
    deployment_id: String,
    static_config: &StaticConfig,
) -> Result<()> {
    let out_dir = AgentPath::AppStaticDir(app_id, deployment_id.clone()).ensure_parent_dir()?;

    let tmp_name = format!("{}-{}-tmp", app_name, deployment_id);

    let options = CreateContainerOptionsBuilder::default()
        .name(&tmp_name)
        .build();

    let config = ContainerCreateBody {
        image: Some(image_reference.to_string()),
        cmd: Some(vec!["x".to_string()]),
        ..Default::default()
    };

    let container = docker.create_container(Some(options), config).await?;

    resources.container_id = Some(container.id.to_string());

    let downlod_options = DownloadFromContainerOptions {
        path: static_config.directory.to_string(),
    };

    let mut stream = docker.download_from_container(&container.id, Some(downlod_options));
    let mut tar_bytes = Vec::new();

    while let Some(chunk) = stream.next().await {
        let data = chunk.context("Error downloading file chunk")?;
        tar_bytes.extend_from_slice(&data);
    }

    let cursor = Cursor::new(tar_bytes);
    let mut archive = Archive::new(cursor);

    archive.unpack(out_dir).context("Failed to untar files")?;

    let remove_options = RemoveContainerOptions {
        force: true, // Forces removal even if it's somehow stuck running
        ..Default::default()
    };

    docker
        .remove_container(&container.id, Some(remove_options))
        .await?;

    // Resetting after removal so other failure won't try to remove already removed container
    resources.container_id = None;

    Ok(())
}
