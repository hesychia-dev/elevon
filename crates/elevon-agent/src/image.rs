use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::LazyLock,
};

use anyhow::{Context, Result};
use bollard::{
    auth::DockerCredentials,
    plugin::{ContainerCreateBody, HostConfig, PortBinding, PortMap, RestartPolicy},
    query_parameters::{
        CreateContainerOptionsBuilder, CreateImageOptionsBuilder, DownloadFromContainerOptions,
        RemoveContainerOptions, RemoveImageOptions,
    },
};
use elevon_contracts::deploy::{
    AppDeployPayload, AppHealthCheckConfig, AppPayload, AppRole, AppRollbackPayload,
    AppRuntimeOptions, RegistryVars, StaticApp, StreamEvent, StreamLogLevel, TlsType, WebApp,
};
use elevon_fs::agent::{
    AgentPath, AppEnvOptions,
    env::{add_app_env_vars, load_app_env},
    remove_app_files,
    tls::write_tls_file,
};
use futures_util::TryStreamExt;
use futures_util::stream::StreamExt;
use tar::Archive;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::{
    api::{
        db::models::{App, Deployment, DeploymentStatus, deployment::DeploymentHealthCheck},
        state::SharedApiState,
        stream::{StreamSender, emit},
    },
    proxy::types::{AgentEvent, DeployAppData},
};

static ALLOCATED_PORTS: LazyLock<RwLock<HashSet<u16>>> =
    LazyLock::new(|| RwLock::new(HashSet::new()));

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

pub async fn deploy_apps(
    tx: &StreamSender,
    state: SharedApiState,
    payload: AppDeployPayload,
) -> Result<()> {
    for app in payload.apps {
        deploy_app(tx, state.clone(), app, None, None).await?;
    }

    Ok(())
}

pub async fn drain_app(
    state: &SharedApiState,
    db: &mut toasty::Db,
    mut deployment: Deployment,
    container_id: String,
    web_app: Option<WebApp>,
) -> Result<()> {
    toasty::update!(deployment {
        status: DeploymentStatus::Drained
    })
    .exec(db)
    .await?;

    state
        .proxy_socket
        .send(AgentEvent::DrainApp(container_id, web_app))
        .await?;

    Ok(())
}

pub async fn rollback_apps(
    tx: &StreamSender,
    state: SharedApiState,
    payload: AppRollbackPayload,
) -> Result<()> {
    // Prevents executing with both 'apps' and 'deployment_ids' present
    if !payload.apps.is_empty() && !payload.deployment_ids.is_empty() {
        emit(
            tx,
            StreamEvent::Error {
                message: "Both apps and deployment ids can't be present".to_string(),
            },
        )
        .await;
        return Ok(());
    }

    for app in payload.apps {
        rollback_with_app(tx, state.clone(), &app.project, &app.name).await?;
    }

    for deployment_id_str in payload.deployment_ids {
        rollback_with_deployment(tx, state.clone(), deployment_id_str).await?;
    }

    Ok(())
}

async fn deploy_app(
    tx: &StreamSender,
    state: SharedApiState,
    app_config: AppPayload,
    // this present = rollback
    deployment_to_run: Option<Deployment>,
    deployment_to_drain: Option<Deployment>,
) -> Result<()> {
    let mut db = state.db.get();
    let mut cloned_db = db.clone();

    let is_rollback = deployment_to_run.is_some();

    emit(
        tx,
        StreamEvent::log(format!("[{}] Deploying...", app_config.name)),
    )
    .await;

    let new_port = match app_config.web_app {
        Some(_) => Some(get_free_port().await?),
        _ => None,
    };

    if let Some(port) = new_port {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Starting running a container on port {}",
                app_config.name, port
            )),
        )
        .await;
    }

    let db_app = App::get_or_create(&mut db, &app_config).await?;

    let mut db_tx = cloned_db.transaction().await?;

    let deployment = match deployment_to_run {
        Some(v) => v,
        None => Deployment::create_deployment(&mut db_tx, &db_app, &app_config, new_port).await?,
    };

    let web_deployment = deployment.web.clone();
    let static_web_deployment = deployment.static_web.clone();

    let options = PullImageAndRunContainerOptions {
        db_tx: &mut db_tx,
        new_port,
        app_config: &app_config,
    };

    let new_deployment = pull_image_and_run_container(tx, &state, deployment, options).await?;

    let app_data = DeployAppData {
        app_id: new_deployment.app_id.to_string(),
        deployment_id: new_deployment.id.to_string(),
        name: app_config.name.to_string(),
        container_id: new_deployment.container_id,
        web_app: web_deployment.map(|w| WebApp::from(&w)),
        static_app: static_web_deployment.map(|s| StaticApp::from(&s)),
        status: DeploymentStatus::Active,
    };

    if AppRole::Web == app_config.role {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Updating proxy routing for traffic",
                app_config.name
            )),
        )
        .await;

        // TODO: In case of any failure in here the container is kept running,
        // need to be deleted.

        state
            .proxy_socket
            .send(AgentEvent::UpsertRoute(app_data.clone()))
            .await
            .context("Failed to send upsert route request")?;
    }

    let deployment_to_drain = match deployment_to_drain {
        Some(v) => Some(v),
        None => Deployment::get_latest_deployment(&mut db, &db_app.id).await?,
    };
    let deployment_to_drain_id = deployment_to_drain.as_ref().map(|d| d.id);

    // TODO: Static app draining
    // Deploying second time doesnt actually create files, it just creates the Deployment in db

    // Marks the current deployment as draining, so no new requests will be handled by it,
    // and later the DrainJanitor service will terminate the container.
    if let Some(deployment_to_drain) = deployment_to_drain
        && let Some(container_id) = deployment_to_drain.container_id.clone()
    {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Found previously released container, draining it...",
                app_config.name,
            )),
        )
        .await;

        drain_app(
            &state,
            &mut db,
            deployment_to_drain,
            container_id,
            app_data.web_app,
        )
        .await?;
    }

    if !is_rollback
        && let Err(err) =
            prune_old_releases(&state.docker, &mut db, &db_app, deployment_to_drain_id).await
    {
        emit(
            tx,
            StreamEvent::Log {
                level: StreamLogLevel::Warn,
                message: format!(
                    "[{}] Failed to prune old releases: {}",
                    app_config.name, err
                ),
            },
        )
        .await;
    }

    db_tx.commit().await?;

    Ok(())
}

struct PullImageAndRunContainerOptions<'cfg, 'tx> {
    db_tx: &'tx mut toasty::Transaction<'cfg>,
    new_port: Option<u16>,
    app_config: &'cfg AppPayload,
}

/// Creates Deployment, sets TLS files, sets Environment variables,
/// and then runs the container. Afterwards updates Deployment's status.
async fn pull_image_and_run_container(
    tx: &StreamSender,
    state: &SharedApiState,
    mut deployment: Deployment,
    options: PullImageAndRunContainerOptions<'_, '_>,
) -> Result<Deployment> {
    let env_options = AppEnvOptions::app(deployment.app_id.to_string(), deployment.id.to_string());

    // Populating env variables
    if let Some(vars) = options.app_config.vars.clone() {
        add_app_env_vars(vars, env_options.clone())?;
    }

    let mut app_env_vars = load_app_env(env_options.clone())?;

    let pull_image_options = PullImageOptions {
        name: &options.app_config.name,
        image_ref: &options.app_config.image_ref,
        app_vars: &mut app_env_vars,
    };

    pull_image(tx, &state.docker, pull_image_options).await.inspect_err(
        |err| tracing::error!(app = %options.app_config.name, error = %err, "failed to pull image"),
    )?;

    if let Some(tls) = options.app_config.tls.as_ref() {
        write_tls_file(tls.cert.as_bytes(), TlsType::Cert, env_options.clone())?;
        write_tls_file(tls.key.as_bytes(), TlsType::Key, env_options)?;
    }

    if let Some(static_app) = options.app_config.static_app.as_ref() {
        handle_static_app_deployment(
            &state.docker,
            &options.app_config.image_ref,
            &options.app_config.name,
            deployment.app_id.to_string(),
            deployment.id.to_string(),
            static_app,
        )
        .await?;

        deployment
            .update()
            .status(DeploymentStatus::Active)
            .exec(options.db_tx)
            .await?;

        // TODO: Add log
        return Ok(deployment);
    }

    let container_id = run_container(
        &state.docker,
        &deployment,
        options.app_config,
        app_env_vars,
        options.new_port,
    )
    .await;

    let res = match container_id {
        Err(err) => {
            if let Some(port) = options.new_port {
                clear_port(&port).await;
            }

            emit(
                tx,
                StreamEvent::log(format!(
                    "[{}] Container failed to start",
                    options.app_config.name
                )),
            )
            .await;

            deployment
                .update()
                .image_ref(Some(options.app_config.image_ref.clone()))
                .status(DeploymentStatus::Failed)
                .exec(options.db_tx)
                .await?;

            Err(err)
        }
        Ok(container_id) => {
            emit(
                tx,
                StreamEvent::log(format!(
                    "[{}] Checking container health. interval = {}, stabilization = {}",
                    options.app_config.name,
                    options.app_config.healthcheck.interval,
                    options.app_config.healthcheck.stabilization
                )),
            )
            .await;

            state
                .check_container_health(
                    &container_id,
                    Some(&options.app_config.name),
                    &DeploymentHealthCheck::from(&options.app_config.healthcheck),
                    Some(tx),
                )
                .await?;

            let deployment_status = state.check_container_state(&container_id).await;

            deployment
                .update()
                .image_ref(Some(options.app_config.image_ref.clone()))
                .container_id(container_id.clone())
                .status(deployment_status)
                .exec(options.db_tx)
                .await?;

            Ok(deployment)
        }
    }?;

    Ok(res)
}

struct PullImageOptions<'a> {
    pub name: &'a str,
    pub image_ref: &'a str,
    pub app_vars: &'a mut HashMap<String, String>,
}

// TODO: When deploying multiple apps within the same project image is pulled on each app's deployment separately
async fn pull_image(
    tx: &StreamSender,
    docker: &bollard::Docker,
    options: PullImageOptions<'_>,
) -> Result<()> {
    emit(
        tx,
        StreamEvent::log(format!(
            "[{}] Pulling the image from registry",
            options.name
        )),
    )
    .await;

    let registry_vars = RegistryVars::get_from_vars(options.app_vars)?;

    let image_options = CreateImageOptionsBuilder::new()
        .from_image(options.image_ref)
        .build();

    let credentials = Some(DockerCredentials {
        username: Some(registry_vars.username),
        password: Some(registry_vars.password),
        serveraddress: Some(registry_vars.server),
        ..Default::default()
    });

    let mut pull_stream = docker.create_image(Some(image_options), None, credentials);

    while let Some(event) = pull_stream.try_next().await? {
        tracing::debug!(?event, "docker pull event");
    }

    emit(
        tx,
        StreamEvent::log(format!("[{}] Image was pulled", options.name)),
    )
    .await;

    Ok(())
}

async fn run_container(
    docker: &bollard::Docker,
    deployment: &Deployment,
    app_config: &AppPayload,
    app_vars: HashMap<String, String>,
    new_port: Option<u16>,
) -> Result<String> {
    let container_name = format!("{}-{}", app_config.name, deployment.id);
    let options = CreateContainerOptionsBuilder::new()
        .name(&container_name)
        .build();

    let app_env = app_vars
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();

    let mut port_bindings = PortMap::new();

    if let (Some(web_app), Some(port)) = (&app_config.web_app, new_port) {
        port_bindings.insert(
            format!("{}/tcp", web_app.port),
            Some(vec![PortBinding {
                host_ip: Some("0.0.0.0".to_string()),
                host_port: Some(port.to_string()),
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

    let container = docker.create_container(Some(options), config).await?;

    docker.start_container(&container.id, None).await?;

    Ok(container.id)
}

async fn prune_old_releases(
    docker: &bollard::Docker,
    db: &mut toasty::Db,
    app: &App,
    just_drained_id: Option<uuid::Uuid>,
) -> Result<()> {
    let deployments =
        Deployment::list_by_app_id(db, &app.id, 0, app.keep_releases as usize).await?;

    for old_dep in deployments {
        if old_dep.status == DeploymentStatus::Active || Some(old_dep.id) == just_drained_id {
            continue; // never prune the one currently serving
        }

        if let Some(container_id) = &old_dep.container_id {
            // best-effort, DrainJanitor may have already reaped it
            let _ = docker.remove_container(container_id, None).await;
        }

        let app_env_options = AppEnvOptions::app(app.id.to_string(), old_dep.id.to_string());
        remove_app_files(app_env_options).context("Failed to remove app files")?;

        if let Some(image_ref) = &old_dep.image_ref {
            let still_referenced = Deployment::filter(
                Deployment::fields()
                    .image_ref()
                    .eq(image_ref)
                    .and(Deployment::fields().id().ne(old_dep.id)),
            )
            .first()
            .exec(db)
            .await?
            .is_some();

            if !still_referenced {
                let remove_options = RemoveImageOptions::default();
                if let Err(err) = docker
                    .remove_image(image_ref, Some(remove_options), None)
                    .await
                {
                    tracing::warn!(%image_ref, %err, "failed to remove unreferenced image");
                }
            }
        }

        Deployment::delete_by_id(db, old_dep.id).await?;
    }

    Ok(())
}

async fn handle_static_app_deployment(
    docker: &bollard::Docker,
    image_reference: &str,
    app_name: &str,
    app_id: String,
    deployment_id: String,
    static_app: &StaticApp,
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

    let downlod_options = DownloadFromContainerOptions {
        path: static_app.directory.to_string(),
    };

    let mut stream = docker.download_from_container(&container.id, Some(downlod_options));
    let mut tar_bytes = Vec::new();

    while let Some(chunk) = stream.next().await {
        let data = chunk.expect("Error downloading file chunk");
        tar_bytes.extend_from_slice(&data);
    }

    let cursor = Cursor::new(tar_bytes);
    let mut archive = Archive::new(cursor);

    archive.unpack(out_dir).expect("Failed to untar files");

    let remove_options = RemoveContainerOptions {
        force: true, // Forces removal even if it's somehow stuck running
        ..Default::default()
    };
    docker
        .remove_container(&container.id, Some(remove_options))
        .await?;

    Ok(())
}

async fn rollback_with_app(
    tx: &StreamSender,
    state: SharedApiState,
    project: &str,
    app_name: &str,
) -> Result<()> {
    let mut db = state.db.get();

    let Some(app) = App::get_by_project_and_name(&mut db, project, app_name).await? else {
        emit(
            tx,
            StreamEvent::log(format!("[{}] App not found, skipping...", app_name)),
        )
        .await;
        return Ok(());
    };

    let previous_deployment = Deployment::get_previous_deployment(&mut db, &app.id).await?;

    let Some(previous_deployment) = previous_deployment else {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Previous deployment doesn't exist, skipping...",
                app.name
            )),
        )
        .await;
        return Ok(());
    };

    rollback_deployment(tx, state, previous_deployment, Some(&app)).await?;

    Ok(())
}

async fn rollback_with_deployment(
    tx: &StreamSender,
    state: SharedApiState,
    deployment_id_str: String,
) -> Result<()> {
    let mut db = state.db.get();

    let Ok(deployment_id) = Uuid::parse_str(&deployment_id_str) else {
        emit(
            tx,
            StreamEvent::log(format!("[{}] Invalid UUID, skipping...", deployment_id_str)),
        )
        .await;
        return Ok(());
    };

    let deployment = Deployment::filter(Deployment::fields().id().eq(deployment_id))
        .include(Deployment::fields().app())
        .include(Deployment::fields().options())
        .first()
        .exec(&mut db)
        .await?;

    let Some(deployment) = deployment else {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Deployment not found, skipping...",
                deployment_id
            )),
        )
        .await;
        return Ok(());
    };

    rollback_deployment(tx, state, deployment, None).await?;

    Ok(())
}

async fn rollback_deployment(
    tx: &StreamSender,
    state: SharedApiState,
    target_deployment: Deployment,
    app: Option<&App>,
) -> Result<()> {
    let mut db = state.db.get();
    let app = match app {
        Some(v) => v,
        _ => target_deployment.app.get(),
    };

    let deployment_options = target_deployment.options.get();

    let (Some(image_ref), Some(deployment_options)) =
        (&target_deployment.image_ref, deployment_options)
    else {
        emit(
            tx,
            StreamEvent::log(format!(
                "[{}] Previous deployment doesn't have image properties, skipping...",
                app.name
            )),
        )
        .await;
        return Ok(());
    };

    let runtime_options = AppRuntimeOptions::from(&deployment_options.runtime);
    let deployment_to_drain = Deployment::get_latest_deployment(&mut db, &app.id).await?;

    let app_payload = AppPayload {
        project: app.project.clone(),
        name: app.name.clone(),
        image_ref: image_ref.to_string(),
        keep_releases: app.keep_releases,
        runtime_options: runtime_options.clone(),
        web_app: target_deployment.web.as_ref().map(|web| WebApp {
            port: web.port,
            domain: web.domain.clone(),
        }),
        healthcheck: AppHealthCheckConfig::from(&deployment_options.healthcheck),
        ..Default::default()
    };

    deploy_app(
        tx,
        state.clone(),
        app_payload,
        Some(target_deployment),
        deployment_to_drain,
    )
    .await?;

    Ok(())
}
