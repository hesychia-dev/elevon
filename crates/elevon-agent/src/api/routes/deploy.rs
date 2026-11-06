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
    deployment::{deploy_apps, rollback_apps},
    proxy::types::DeployAppData,
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
        deploy_apps(&tx, state, payload).await
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
        rollback_apps(&tx, state, payload).await
    }))
}

async fn local_containers(
    State(state): State<SharedApiState>,
) -> Result<AppJson<Vec<DeployAppData>>, AppError> {
    let result = state.get_running_route_containers().await?;
    Ok(AppJson(result))
}
