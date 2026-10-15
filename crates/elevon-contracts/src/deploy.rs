use std::collections::HashMap;

use anyhow::Result;
use bollard::plugin::RestartPolicyNameEnum;
use bytes::Bytes;
use elevon_config::{ConfigError, ResolveEnvCredentials, resolve_env_or_literal};
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebApp {
    pub port: u16,
    pub domain: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRuntimeOptions {
    pub cmd: Option<Vec<String>>,
    pub restart: Option<RestartPolicyNameEnum>,
    pub memory_limit: Option<i64>,
    pub cpu_limit: Option<i64>,
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TlsConfig {
    pub cert: String,
    pub key: String,
}

impl ResolveEnvCredentials for TlsConfig {
    type Output = Self;

    fn resolved_credentials(&self) -> Result<Self::Output, ConfigError> {
        Ok(Self {
            cert: resolve_env_or_literal(&self.cert)?,
            key: resolve_env_or_literal(&self.key)?,
        })
    }
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
    pub vars: Option<HashMap<String, String>>,
    pub tls: Option<TlsConfig>,
    pub web_app: Option<WebApp>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppDeployPayload {
    pub apps: Vec<AppPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHealthCheckConfig {
    #[serde(default = "AppHealthCheckConfig::default_interval")]
    pub interval: u64,

    #[serde(default = "AppHealthCheckConfig::default_timeout")]
    pub timeout: u64,

    #[serde(default = "AppHealthCheckConfig::default_retries")]
    pub retries: u64,

    #[serde(default = "AppHealthCheckConfig::default_stabilization")]
    pub stabilization: u64,

    #[serde(default = "AppHealthCheckConfig::default_required_successes")]
    pub required_successes: u64,

    pub path: Option<String>,
}

impl AppHealthCheckConfig {
    fn default_interval() -> u64 {
        5
    }

    fn default_timeout() -> u64 {
        30
    }

    fn default_retries() -> u64 {
        3
    }

    fn default_stabilization() -> u64 {
        5
    }

    fn default_required_successes() -> u64 {
        1
    }
}

impl Default for AppHealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: AppHealthCheckConfig::default_interval(),
            timeout: AppHealthCheckConfig::default_timeout(),
            retries: AppHealthCheckConfig::default_retries(),
            stabilization: AppHealthCheckConfig::default_stabilization(),
            required_successes: AppHealthCheckConfig::default_required_successes(),
            path: None,
        }
    }
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

#[derive(Clone, PartialEq, Eq)]
pub enum TlsType {
    Cert,
    Key,
}

impl TlsType {
    pub fn get_file(&self) -> &str {
        match self {
            TlsType::Cert => "cert.pem",
            TlsType::Key => "key.pem",
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct AppStopPayload {
    pub project: Option<String>,
    pub apps: Vec<String>,
    pub force: bool,
}
