pub mod config;

use std::collections::HashMap;

use anyhow::Result;
use bollard::plugin::RestartPolicyNameEnum;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};

use crate::deploy::config::{AppHealthCheckConfig, AppRoutingConfig};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamLogLevel {
    #[default]
    Info,
    Warn,
    Error,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum StreamEvent {
    Log {
        level: StreamLogLevel,
        message: String,
    },
    Done,
    Error {
        message: String,
    },
}

impl StreamEvent {
    pub fn log(message: impl Into<String>) -> Self {
        Self::Log {
            level: StreamLogLevel::default(),
            message: message.into(),
        }
    }

    pub fn warn(message: impl Into<String>) -> Self {
        Self::Log {
            level: StreamLogLevel::Warn,
            message: message.into(),
        }
    }
}

pub async fn log_stream_events<S>(mut event_stream: S) -> anyhow::Result<()>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    while let Some(chunk_result) = event_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(error = %err, "Stream connection error");
                return Err(err.into());
            }
        };
        let text = String::from_utf8_lossy(&chunk);

        for line in text.lines() {
            if line.trim().is_empty() {
                continue;
            }

            if line.starts_with(":") {
                continue;
            }

            if let Some(payload) = line.strip_prefix("data: ") {
                tracing::debug!("log_stream_events-data: {}", payload);

                if payload == "[DONE]" {
                    tracing::debug!("Stream sent [DONE] payload");
                    continue;
                }

                match serde_json::from_str::<StreamEvent>(payload) {
                    Ok(StreamEvent::Log { level, message }) => match level {
                        StreamLogLevel::Info => tracing::info!(message),
                        StreamLogLevel::Warn => tracing::warn!(message),
                        _ => tracing::error!(message),
                    },
                    Ok(StreamEvent::Error { message }) => {
                        tracing_indicatif::indicatif_println!("\x1b[31m✗ {message}\x1b[0m");
                    }
                    Ok(other_event) => {
                        tracing::debug!(?other_event, "Unhandled stream event variant");
                    }
                    Err(err) => {
                        tracing::warn!(
                            error = %err,
                            raw_payload = payload,
                            "Failed to parse SSE payload as StreamEvent"
                        );
                    }
                }
            }
        }
    }

    Ok(())
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "lowercase")]
#[serde(rename_all = "lowercase")]
pub enum AppRole {
    #[default]
    Web,
    Worker,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseAppRoleError;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRuntimeOptions {
    pub cmd: Option<Vec<String>>,
    pub restart: Option<RestartPolicyNameEnum>,
    pub memory_limit: Option<i64>,
    pub cpu_limit: Option<i64>,
    pub network: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppPayload {
    pub image_ref: String,
    pub project: String,
    pub name: String,
    pub keep_releases: u8,
    pub role: AppRole,
    pub runtime_options: AppRuntimeOptions,
    pub healthcheck: AppHealthCheckConfig,
    pub routing: Option<AppRoutingConfig>,
    pub vars: Option<HashMap<String, String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppDeployPayload {
    pub apps: Vec<AppPayload>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppRollback {
    pub project: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppRollbackPayload {
    pub apps: Vec<AppRollback>,
    pub deployment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TlsType {
    Cert,
    Key,
}

#[derive(Serialize, Deserialize)]
pub struct AppStopPayload {
    pub project: Option<String>,
    pub apps: Vec<String>,
    pub force: bool,
}

#[derive(Debug)]
pub struct RegistryVars {
    pub server: String,
    pub username: String,
    pub password: String,
}

impl RegistryVars {
    pub const KEY_SERVER: &'static str = "REGISTRY_SERVER";
    pub const KEY_USERNAME: &'static str = "REGISTRY_USERNAME";
    pub const KEY_PASSWORD: &'static str = "REGISTRY_PASSWORD";

    pub fn resolve_env_vars(&self) -> HashMap<String, String> {
        HashMap::from([
            (Self::KEY_SERVER.to_string(), self.server.clone()),
            (Self::KEY_USERNAME.to_string(), self.username.clone()),
            (Self::KEY_PASSWORD.to_string(), self.password.clone()),
        ])
    }

    pub fn get_from_vars(vars: &mut HashMap<String, String>) -> Result<Self> {
        let server = vars
            .remove(Self::KEY_SERVER)
            .ok_or_else(|| anyhow::anyhow!("Missing environment variable: {}", Self::KEY_SERVER))?;

        let username = vars.remove(Self::KEY_USERNAME).ok_or_else(|| {
            anyhow::anyhow!("Missing environment variable: {}", Self::KEY_USERNAME)
        })?;

        let password = vars.remove(Self::KEY_PASSWORD).ok_or_else(|| {
            anyhow::anyhow!("Missing environment variable: {}", Self::KEY_PASSWORD)
        })?;

        Ok(Self {
            server,
            username,
            password,
        })
    }
}
