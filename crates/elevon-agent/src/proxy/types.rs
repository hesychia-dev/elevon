use std::{path::PathBuf, sync::atomic::AtomicUsize, time::Instant};

use elevon_contracts::deploy::{StaticApp, WebApp};
use serde::{Deserialize, Serialize};

use crate::api::db::models::DeploymentStatus;

#[derive(Serialize, Deserialize)]
#[serde(tag = "event", content = "data")]
pub enum AgentEvent {
    UpsertRoute(DeployAppData),
    DrainApp(String, Option<WebApp>),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeployAppData {
    pub app_id: String,
    pub deployment_id: String,
    pub name: String,
    pub status: DeploymentStatus,
    pub container_id: Option<String>,
    pub web_app: Option<WebApp>,
    pub static_app: Option<StaticApp>,
}

#[derive(Debug, Default)]
pub struct RouteBackendRuntime {
    pub port: u16,
    pub inflight: AtomicUsize,
    pub drain_started_at: Option<Instant>,
}

#[derive(Debug, Default)]
pub struct BackendRuntime {
    pub container_id: String,
    pub status: DeploymentStatus,
    pub route: Option<RouteBackendRuntime>,
}

#[derive(Debug)]
pub struct StaticAppResponseData {
    pub file_path: PathBuf,
    pub mime_type: String,
}
