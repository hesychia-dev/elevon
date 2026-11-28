use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use elevon_contracts::deploy::AppRoutingPayload;
use serde::{Deserialize, Serialize};

use crate::api::db::models::DeploymentStatus;

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum AgentEvent {
    UpsertRoute(DeployAppData),
    DrainApp(DrainAppData),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployAppData {
    pub app_id: String,
    pub deployment_id: String,
    pub status: DeploymentStatus,
    pub container_id: Option<String>,
    pub routing: Option<AppRoutingPayload>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DrainAppData {
    pub deployment_id: String,
    pub container_id: Option<String>,
    pub domain: Option<String>,
    pub port: Option<u16>,
}

#[derive(Debug, Clone, Default)]
pub struct BackendRuntime {
    pub inflight: Arc<AtomicUsize>,
    pub drain_started_at: Option<Instant>,
}

impl BackendRuntime {
    pub fn increase_count(&self) {
        self.inflight.fetch_add(1, Ordering::Relaxed);
    }

    pub fn decrease_count(&self) {
        self.inflight.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn mark_as_draining(&mut self) {
        self.drain_started_at = Some(Instant::now());
    }
}
