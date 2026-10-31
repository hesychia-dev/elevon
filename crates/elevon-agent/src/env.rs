use std::collections::HashMap;

use anyhow::Result;
use elevon_fs::agent::{
    AppEnvOptions,
    env::{add_app_env, load_app_env},
};
use strum::IntoEnumIterator;
use strum_macros::{EnumIter, IntoStaticStr};

use crate::config::{Config, ProxyConfig, parse_port};

#[derive(Debug, Clone)]
pub struct ElevonEnv {
    pub agent_domain: Option<String>,
    pub agent_port: u16,
    pub proxy_http_port: u16,
    pub proxy_https_port: Option<u16>,
    pub turso_remote_url: Option<String>,
}

impl Default for ElevonEnv {
    fn default() -> Self {
        Self {
            agent_domain: None,
            agent_port: 3333,
            proxy_http_port: 6188,
            proxy_https_port: None,
            turso_remote_url: None,
        }
    }
}

impl ElevonEnv {
    pub fn new(config: Config) -> Result<Self> {
        let proxy_ports = config
            .proxy
            .as_ref()
            .map(|c| c.port.clone())
            .unwrap_or(ProxyConfig::default_ports());
        let proxy_ports = parse_port(proxy_ports)?;

        let mut elevon_env = Self {
            agent_domain: Some(config.agent.domain.clone()),
            agent_port: config.agent.port,
            proxy_http_port: proxy_ports.0,
            proxy_https_port: proxy_ports.1,
            turso_remote_url: config.turso_remote_url,
        };
        elevon_env.set_env_values()?;

        Ok(elevon_env)
    }

    pub fn load() -> Result<Self> {
        let vars = load_app_env(AppEnvOptions::agent())?;
        let mut env = Self::default();

        for key in ElevonEnvKey::all() {
            if let Some(value) = key.read_from(&vars) {
                env.apply_value(key, value)?;
            } else if key == ElevonEnvKey::ProxyHttpPort {
                let default_ports = ProxyConfig::default_ports();
                let (http_port, https_port) = parse_port(default_ports)?;

                env.apply_value(ElevonEnvKey::ProxyHttpPort, http_port.to_string())?;
                env.apply_value(
                    ElevonEnvKey::ProxyHttpsPort,
                    https_port.unwrap().to_string(),
                )?;
            }
        }

        Ok(env)
    }

    // updates the in-memory field only, without touching disk
    fn apply_value(&mut self, key: ElevonEnvKey, value: String) -> Result<()> {
        match key {
            ElevonEnvKey::AgentDomain => self.agent_domain = Some(value),
            ElevonEnvKey::AgentPort => self.agent_port = value.parse::<u16>()?,
            ElevonEnvKey::ProxyHttpPort => self.proxy_http_port = value.parse::<u16>()?,
            ElevonEnvKey::ProxyHttpsPort => self.proxy_https_port = Some(value.parse::<u16>()?),
            ElevonEnvKey::TursoRemoteUrl => self.turso_remote_url = Some(value),
        }

        Ok(())
    }

    fn set_value(&mut self, key: ElevonEnvKey, value: String) -> Result<()> {
        let persisted = value.clone();

        self.apply_value(key, value)?;

        add_app_env(
            key.bare_name().to_string(),
            persisted,
            AppEnvOptions::agent(),
        )?;

        Ok(())
    }

    fn set_env_values(&mut self) -> Result<()> {
        if let Some(agent_domain) = &self.agent_domain {
            self.set_value(ElevonEnvKey::AgentDomain, agent_domain.clone())?;
        }

        self.set_value(ElevonEnvKey::AgentPort, self.agent_port.to_string())?;

        self.set_value(
            ElevonEnvKey::ProxyHttpPort,
            self.proxy_http_port.to_string(),
        )?;

        if let Some(proxy_https_port) = self.proxy_https_port {
            self.set_value(ElevonEnvKey::ProxyHttpsPort, proxy_https_port.to_string())?;
        }

        if let Some(turso_remote_url) = &self.turso_remote_url {
            self.set_value(ElevonEnvKey::TursoRemoteUrl, turso_remote_url.clone())?;
        }

        Ok(())
    }
}

#[derive(Debug, Copy, Clone, EnumIter, IntoStaticStr, PartialEq, Eq)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum ElevonEnvKey {
    AgentDomain,
    AgentPort,
    ProxyHttpPort,
    ProxyHttpsPort,
    TursoRemoteUrl,
}

impl ElevonEnvKey {
    /// Returns an iterator over all variants.
    pub fn all() -> impl Iterator<Item = Self> {
        Self::iter()
    }

    /// Returns the environment variable name string.
    pub fn bare_name(&self) -> &'static str {
        self.into()
    }

    fn read_from(&self, vars: &HashMap<String, String>) -> Option<String> {
        vars.get(&self.bare_name().to_string()).cloned()
    }

    pub fn from_bare_name(s: &str) -> Option<Self> {
        Self::all().into_iter().find(|k| k.bare_name() == s)
    }
}

impl std::str::FromStr for ElevonEnvKey {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_bare_name(s).ok_or_else(|| anyhow::anyhow!("unknown env key: {s}"))
    }
}
