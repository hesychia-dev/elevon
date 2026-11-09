use axum::{
    Json, Router,
    extract::State,
    middleware,
    response::Result,
    routing::{get, post},
};
use elevon_contracts::deploy::{AppDeployPayload, AppRollbackPayload};
use elevon_http::error::{AppError, AppJson};
use reqwest::StatusCode;

use crate::{
    api::{
        db::models::AuthKey,
        middleware::require_unix_socket,
        state::SharedApiState,
        stream::{StreamResponse, lock_action, spawn_streaming_task},
    },
    deployment::DeploymentService,
    proxy::types::DeployAppData,
    socket::{Socket, SocketType},
};

pub fn router() -> Router<SharedApiState> {
    let local_router = Router::new()
        .route("/containers", get(local_containers))
        .route_layer(middleware::from_fn(require_unix_socket));

    Router::new()
        .route("/", post(deploy))
        .route("/rollback", post(rollback))
        .nest("/local", local_router)
}

async fn deploy(
    _: AuthKey,
    State(state): State<SharedApiState>,
    Json(payload): Json<AppDeployPayload>,
) -> StreamResponse {
    spawn_streaming_task(move |tx| async move {
        let _lock_file = lock_action()?;
        let service = DeploymentService::new(
            std::sync::Arc::new(state.docker.clone()),
            state.db.get(),
            std::sync::Arc::new(Socket::new(SocketType::Proxy, false)?),
        );
        service.deploy(tx, payload.apps).await
    })
}

async fn rollback(
    _: AuthKey,
    State(state): State<SharedApiState>,
    Json(payload): Json<AppRollbackPayload>,
) -> Result<StreamResponse, AppError> {
    if !payload.apps.is_empty() && !payload.deployment_ids.is_empty() {
        return Err(AppError::client(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Both apps and deployment IDs can't be passed",
        ));
    }

    Ok(spawn_streaming_task(move |tx| async move {
        let _lock_file = lock_action()?;
        let service = DeploymentService::new(
            std::sync::Arc::new(state.docker.clone()),
            state.db.get(),
            std::sync::Arc::new(Socket::new(SocketType::Proxy, false)?),
        );
        service.rollback(tx, payload).await
    }))
}

async fn local_containers(
    State(state): State<SharedApiState>,
) -> Result<AppJson<Vec<DeployAppData>>, AppError> {
    let result = state.get_running_route_containers().await?;
    Ok(AppJson(result))
}
