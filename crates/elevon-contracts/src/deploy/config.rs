use elevon_config::{ConfigError, ResolveEnvCredentials, resolve_env_or_literal};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppHealthCheckConfig {
    #[serde(default = "AppHealthCheckConfig::default_start_period")]
    pub start_period: u64,

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
    pub fn default_start_period() -> u64 {
        5
    }

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
            start_period: AppHealthCheckConfig::default_start_period(),
            interval: AppHealthCheckConfig::default_interval(),
            timeout: AppHealthCheckConfig::default_timeout(),
            retries: AppHealthCheckConfig::default_retries(),
            stabilization: AppHealthCheckConfig::default_stabilization(),
            required_successes: AppHealthCheckConfig::default_required_successes(),
            path: None,
        }
    }
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct AppRoutingConfig {
    pub domain: String,

    #[serde(alias = "path")]
    pub path_prefix: Option<String>,

    pub port: Option<PortConfig>,

    #[serde(skip)]
    pub resolved_port: Option<u16>,

    #[serde(rename = "static")]
    pub static_files: Option<StaticConfig>,

    pub tls: Option<TlsConfig>,
}

impl ResolveEnvCredentials for AppRoutingConfig {
    type Output = Self;

    fn resolved_credentials(&self) -> Result<Self::Output, elevon_config::ConfigError> {
        let port = self
            .port
            .as_ref()
            .map(|port| -> Result<u16, elevon_config::ConfigError> {
                match port {
                    PortConfig::Literal(p) => Ok(*p),
                    PortConfig::Env(key) => {
                        let value =
                            resolve_env_or_literal(key).map_err(|e| ConfigError::InvalidValue {
                                field: "routing.port_env".to_string(),
                                value: e.to_string(),
                            })?;
                        value.parse::<u16>().map_err(|_| ConfigError::InvalidValue {
                            field: "routing.port".to_string(),
                            value,
                        })
                    }
                }
            })
            .transpose()?;

        Ok(Self {
            resolved_port: port,
            ..(*self).clone()
        })
    }
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticConfig {
    /// Work directory in the Dockerfile
    pub directory: String,

    /// Index file name that will be served as root
    #[serde(default = "StaticConfig::default_index")]
    pub index: String,
}

impl StaticConfig {
    fn default_index() -> String {
        "index.html".to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PortConfig {
    Literal(u16),
    Env(String),
}
