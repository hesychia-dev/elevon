use bollard::query_parameters::RemoveContainerOptions;
use elevon_fs::agent::{AppEnvOptions, remove_app_files};

use super::{ActionState, DeploymentResources, clear_port};
use crate::api::db::models::{Deployment, DeploymentStatus};

pub(super) async fn cleanup_failed_deployment(
    action_state: &ActionState<'_>,
    db: &mut toasty::Db,
    deployment: &mut Deployment,
    resources: &mut DeploymentResources,
) -> Vec<String> {
    let mut failures = cleanup_resources(
        action_state,
        db,
        deployment,
        resources,
        DeploymentStatus::Failed,
    )
    .await;

    let app_env_options =
        AppEnvOptions::app(deployment.app_id.to_string(), deployment.id.to_string());

    if let Err(err) = remove_app_files(app_env_options) {
        let already_removed = err
            .downcast_ref::<std::io::Error>()
            .is_some_and(|err| err.kind() == std::io::ErrorKind::NotFound);

        if !already_removed {
            failures.push(format!("Failed to remove app files: {err:#}"));
        }
    }

    failures
}

pub(super) async fn cleanup_failed_rollback(
    action_state: &ActionState<'_>,
    db: &mut toasty::Db,
    deployment: &mut Deployment,
    resources: &mut DeploymentResources,
) -> Vec<String> {
    cleanup_resources(
        action_state,
        db,
        deployment,
        resources,
        DeploymentStatus::Drained,
    )
    .await
}

async fn cleanup_resources(
    action_state: &ActionState<'_>,
    db: &mut toasty::Db,
    deployment: &mut Deployment,
    resources: &mut DeploymentResources,
    status: DeploymentStatus,
) -> Vec<String> {
    let mut failures = Vec::new();

    if let Err(err) = deployment.update().status(status).exec(db).await {
        failures.push(format!("Failed to update deployment status: {err:#}"));
    }

    if let Some(port) = resources.port {
        clear_port(&port).await;

        resources.port = None;
    }

    if let Some(container_id) = resources.container_id.as_ref() {
        let options = RemoveContainerOptions {
            force: true,
            ..Default::default()
        };

        if let Err(err) = action_state
            .docker
            .remove_container(container_id, Some(options))
            .await
        {
            failures.push(format!("Failed to remove container: {err:#}"));
        } else {
            resources.container_id = None;
        }
    }

    failures
}
