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
                    cloned_state.upsert_route(route);
                }
                AgentEvent::DrainApp(route) => {
                    cloned_state.drain_app(route);
                }
            }

            Ok(Option::<()>::None)
        })
        .await
        .context("proxy socket listener failed")?;

    Ok(())
}

pub struct SocketControl {
    pub state: Arc<ProxyState>,
}

#[async_trait]
impl BackgroundService for SocketControl {
    async fn start(&self, mut shutdown: ShutdownWatch) {
        tokio::select! {
            _ = shutdown.changed() => {}
            _ = run_socket_listener(self.state.clone()) => {}
        }
    }
}
