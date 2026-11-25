use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use pingora::{server::ShutdownWatch, services::background::BackgroundService};

use crate::proxy::state::ProxyState;
use crate::proxy::types::AgentEvent;

async fn run_socket_listener(state: Arc<ProxyState>) -> Result<()> {
    tracing::info!("starting proxy socket listener");

    state
        .proxy_socket
        .listener(state.clone(), |cloned_state, msg: AgentEvent| async move {
            match msg {
                AgentEvent::UpsertRoute(route) => {
                    cloned_state.registry.upsert(route)?;
                    // cloned_state.upsert_route(route).map_err(
                    //     |err| -> Box<dyn std::error::Error + Send + Sync> { err.into() },
                    // )?;
                }
                AgentEvent::DrainApp(_route) => {
                    // cloned_state.drain_app(route);
                }
            }

            Ok(Some(()))
        })
        .await
        .context("proxy socket listener failed")?;

    Ok(())
}

pub struct SocketService(pub Arc<ProxyState>);

#[async_trait]
impl BackgroundService for SocketService {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        tokio::select! {
            _ = shutdown.changed() => {}
            _ = run_socket_listener(self.0.clone()) => {}
        }
    }
}
