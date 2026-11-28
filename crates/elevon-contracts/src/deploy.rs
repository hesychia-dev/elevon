pub mod config;

use std::collections::HashMap;

use anyhow::Result;
use bollard::plugin::RestartPolicyNameEnum;
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Deserializer, Serialize};
use strum_macros::{Display, EnumString};

use crate::deploy::config::{
    AppHealthCheckConfig, AppRoutingConfig, StaticRoutingConfig, TlsConfig,
};

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

    pub fn error(message: impl Into<String>) -> Self {
        Self::Log {
            level: StreamLogLevel::Error,
            message: message.into(),
        }
    }
}

pub async fn log_stream_events<S>(mut event_stream: S) -> anyhow::Result<()>
where
    S: Stream<Item = Result<Bytes, reqwest::Error>> + Unpin,
{
    let mut buffer = Vec::new();
    let mut done = false;

    while let Some(chunk_result) = event_stream.next().await {
        let chunk = match chunk_result {
            Ok(c) => c,
            Err(err) => {
                tracing::error!(error = %err, "Stream connection error");
                return Err(err.into());
            }
        };
        buffer.extend_from_slice(&chunk);

        while let Some(end) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=end).collect();
            let line = std::str::from_utf8(&line)?.trim_end_matches(['\r', '\n']);
            if line.trim().is_empty() {
                continue;
            }

            if line.starts_with(":") {
                continue;
            }

            if let Some(payload) = line.strip_prefix("data: ") {
                tracing::debug!("log_stream_events-data: {}", payload);

                if payload == "[DONE]" {
                    done = true;
                    continue;
                }

                match serde_json::from_str::<StreamEvent>(payload) {
                    Ok(StreamEvent::Log { level, message }) => match level {
                        StreamLogLevel::Info => tracing::info!(message),
                        StreamLogLevel::Warn => tracing::warn!(message),
                        _ => tracing::error!(message),
                    },
                    Ok(StreamEvent::Error { message }) => {
                        anyhow::bail!("{message}");
                    }
                    Ok(StreamEvent::Done) => {
                        done = true;
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                }
            }
        }
    }

    if !done {
        anyhow::bail!("Stream ended before the operation completed");
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AppRuntimeOptions {
    pub cmd: Option<Vec<String>>,
    pub restart: Option<RestartPolicyNameEnum>,
    pub memory_limit: Option<i64>,
    pub cpu_limit: Option<i64>,
    pub network: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AppRoutingConfigPayload {
    Server(u16),
    Static(StaticRoutingConfig),
}

pub trait AppRoutingConfigTrait {
    type StaticConfig;

    fn port(&self) -> Option<u16>;
    fn static_config(&self) -> Option<Self::StaticConfig>;
}

impl AppRoutingConfigTrait for AppRoutingConfigPayload {
    type StaticConfig = StaticRoutingConfig;

    fn port(&self) -> Option<u16> {
        match self {
            Self::Server(port) => Some(port.clone()),
            Self::Static(_) => None,
        }
    }

    fn static_config(&self) -> Option<Self::StaticConfig> {
        match self {
            Self::Static(static_config) => Some(static_config.clone()),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRoutingPayload {
    pub domain: String,
    #[serde(default, deserialize_with = "deserialize_stripped_prefix")]
    pub path_prefix: Option<String>,
    pub tls: Option<TlsConfig>,
    pub config: AppRoutingConfigPayload,
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
    pub routing: Option<AppRoutingPayload>,
    pub vars: HashMap<String, String>,
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

impl TryFrom<&AppRoutingConfig> for AppRoutingPayload {
    type Error = anyhow::Error;

    fn try_from(value: &AppRoutingConfig) -> std::prelude::v1::Result<Self, Self::Error> {
        let config = match (value.static_config.as_ref(), value.port.as_ref()) {
            (Some(static_config), None) => AppRoutingConfigPayload::Static(static_config.clone()),
            (None, Some(port)) => AppRoutingConfigPayload::Server(port.resolve()?),
            _ => {
                anyhow::bail!("Invalid routing configuration");
            }
        };

        Ok(Self {
            domain: value.domain.clone(),
            path_prefix: value.path_prefix.clone(),
            tls: value.tls.clone(),
            config,
        })
    }
}

fn deserialize_stripped_prefix<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.map(|prefix| prefix.trim_end_matches('/').to_string()))
}

#[cfg(test)]
mod tests {
    use futures_util::FutureExt;

    use super::*;

    #[test]
    fn stream_reports_errors_split_across_chunks() {
        let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![
            Ok(Bytes::from_static(b"data: {\"type\":\"Err")),
            Ok(Bytes::from_static(
                b"or\",\"message\":\"deployment failed\"}\n\n",
            )),
        ];

        let result = log_stream_events(futures_util::stream::iter(chunks))
            .now_or_never()
            .unwrap();

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("deployment failed")
        );
    }

    #[test]
    fn stream_requires_completion() {
        let chunks: Vec<Result<Bytes, reqwest::Error>> =
            vec![Ok(Bytes::from_static(b"data: {\"type\":\"Done\"}\n\n"))];
        assert!(
            log_stream_events(futures_util::stream::iter(chunks))
                .now_or_never()
                .unwrap()
                .is_ok()
        );

        let chunks: Vec<Result<Bytes, reqwest::Error>> = vec![];
        assert!(
            log_stream_events(futures_util::stream::iter(chunks))
                .now_or_never()
                .unwrap()
                .is_err()
        );
    }
}
