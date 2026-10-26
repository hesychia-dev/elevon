pub mod db;
mod middleware;
pub mod routes;
pub mod state;
pub mod stream;

use std::{net::SocketAddr, sync::Arc};

use anyhow::{Context, Result};
use axum::Router;
use elevon_fs::agent::AgentPath;
use tokio::task::JoinSet;

use crate::api::middleware::UdsConnectInfo;
use crate::{cli::ApiArgs, env::ElevonEnv};

pub async fn run_api_server(args: ApiArgs, env: &ElevonEnv) -> Result<()> {
    let state = Arc::new(state::ApiState::new(env).await?);
    let cloned_state = state.clone();

    let router = routes::create_router(cloned_state);
    let app = Router::new().merge(router);

    let mut set = JoinSet::new();

    let addr = format!("127.0.0.1:{}", args.port);

    let tcp = tokio::net::TcpListener::bind(&addr)
        .await
        .context(format!("failed to bind API listener to {}", addr))?;

    let socket_path = AgentPath::AgentSocket.resolve_and_delete_if_exists();
    let unix = tokio::net::UnixListener::bind(&socket_path).context(format!(
        "failed to bind API listener to UNIX Socket {}",
        socket_path.display(),
    ))?;

    AgentPath::restrict_permissions(socket_path)?;

    tracing::info!("listening on {}", tcp.local_addr()?);

    let tcp_app = app.clone();
    set.spawn(async move {
        if let Err(err) = axum::serve(
            tcp,
            tcp_app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        {
            tracing::error!(error = %err, "API server failed");
        }
    });

    set.spawn(async move {
        if let Err(err) = axum::serve(
            unix,
            app.into_make_service_with_connect_info::<UdsConnectInfo>(),
        )
        .await
        {
            tracing::error!(error = %err, "API UNIX server failed");
        }
    });

    if let Some(result) = set.join_next().await {
        result?;
    }

    Ok(())
}
