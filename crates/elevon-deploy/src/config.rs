pub mod app;
pub mod elevon;
pub mod env;
pub mod registry;

use std::collections::HashMap;

use anyhow::Result;
use elevon_config::{ConfigError, ElevonConfig, ResolveEnvCredentials, resolve_env_or_literal};
use elevon_contracts::deploy::{AppRole, StaticApp, TlsConfig};
use serde::Deserialize;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    agent::{
        AgentClient, DeployService, prepare_apps_deploy_payload, prepare_apps_rollback_payload,
    },
    cli::AppRollbackArgs,
    config::{app::AppConfig, env::EnvConfig, registry::RegistryConfig},
    image::progress::print_success,
};

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    pub name: String,
    pub image: String,

    #[serde(default = "Config::default_keep_releases")]
    pub keep_releases: Option<u8>,

    pub elevon: elevon::ElevonConfig,

    pub registry: registry::RegistryConfig,

    pub build: BuildConfig,

    pub routing: RoutingConfig,

    pub env: Option<EnvConfig>,

    pub apps: HashMap<String, AppConfig>,
}

impl ElevonConfig for Config {}

impl Config {
    pub fn default_keep_releases() -> Option<u8> {
        Some(5)
    }

    pub fn prepare_project_env_vars(
        &self,
        registry_config: &RegistryConfig,
    ) -> Result<HashMap<String, String>> {
        let mut vars = self
            .env
            .as_ref()
            .map_or_else(|| Ok(HashMap::default()), |env| env.resolved_credentials())?;

        vars.extend(registry_config.vars());

        Ok(vars)
    }

    pub async fn run_build(
        &self,
        registry_config: &RegistryConfig,
        config_path: String,
        push: bool,
    ) -> Result<()> {
        let built =
            crate::image::build_image(&self.image, registry_config, &self.build, &config_path)
                .await?;

        if built && push {
            self.run_push(registry_config, &config_path).await?;
            return Ok(());
        }

        Ok(())
    }

    pub async fn run_push(
        &self,
        registry_config: &RegistryConfig,
        config_path: &str,
    ) -> Result<()> {
        crate::image::push_image(&self.image, registry_config, &self.build, config_path).await?;
        Ok(())
    }

    pub fn get_selected_apps(&self, arg_apps: &[String]) -> Result<Vec<(String, AppConfig)>> {
        if arg_apps.is_empty() {
            if !self.apps.is_empty() {
                return Ok(self
                    .apps
                    .iter()
                    .map(|(k, v)| {
                        (
                            k.clone(),
                            AppConfig {
                                tls: self.routing.tls.clone(),
                                ..v.clone()
                            },
                        )
                    })
                    .collect());
            } else {
                return Ok(vec![(
                    "web".to_string(),
                    AppConfig {
                        role: AppRole::Web,
                        tls: self.routing.tls.clone(),
                        ..Default::default()
                    },
                )]);
            }
        }

        arg_apps
            .iter()
            .map(|name| {
                self.apps
                    .get(name)
                    .cloned()
                    .map(|cfg| {
                        (
                            name.clone(),
                            AppConfig {
                                tls: self.routing.tls.clone(),
                                ..cfg
                            },
                        )
                    })
                    .ok_or_else(|| anyhow::anyhow!("unknown app `{name}`"))
            })
            .collect()
    }

    #[tracing::instrument(name = "deploy", skip_all)]
    pub async fn run_deploy(
        &self,
        agent_client: AgentClient,
        app_names: &[String],
        registry_config: RegistryConfig,
        config_path: &str,
    ) -> Result<()> {
        let selected_apps = self.get_selected_apps(app_names)?;

        self.run_build(&registry_config, config_path.to_string(), true)
            .await?;

        let app_names: Vec<_> = selected_apps.iter().map(|(key, _)| key).collect();
        tracing::Span::current().pb_set_message(&format!("deploying {}", self.name));
        tracing::info!("Deploying apps: {:?}", app_names);

        let payload = prepare_apps_deploy_payload(
            self,
            &registry_config,
            selected_apps,
            config_path,
            &self.build,
        )?;

        let deploy_service = DeployService::new(agent_client);
        deploy_service.push_deploy(payload).await?;

        print_success(&format!("Deployed {}", self.name));

        Ok(())
    }

    #[tracing::instrument(name = "rollback", skip_all)]
    pub async fn run_rollback(
        &self,
        agent_client: AgentClient,
        args: AppRollbackArgs,
    ) -> Result<()> {
        let selected_apps = self.get_selected_apps(&args.apps)?;

        let app_names: Vec<_> = selected_apps.iter().map(|(key, _)| key).collect();
        tracing::Span::current().pb_set_message(&format!("rolling back {}", self.name));
        tracing::info!("Rolling back apps: {:?}", app_names);

        let payload = prepare_apps_rollback_payload(self, selected_apps, args.ids)?;

        let deploy_service = DeployService::new(agent_client);
        deploy_service.push_rollback(payload).await?;

        print_success(&format!("Rolled back {}", self.name));

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum PortConfig {
    Literal(u16),
    Env(String),
}

impl Default for PortConfig {
    fn default() -> Self {
        Self::Literal(8000)
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RoutingConfig {
    pub domain: String,
    pub port: Option<PortConfig>,
    pub tls: Option<TlsConfig>,
    #[serde(alias = "static")]
    pub static_files: Option<StaticConfig>,
}

#[derive(Debug)]
pub struct ResolvedRoutingConfig {
    pub domain: String,
    pub port: Option<u16>,
    pub tls: Option<TlsConfig>,
    pub static_files: Option<StaticConfig>,
}

impl ResolveEnvCredentials for RoutingConfig {
    type Output = ResolvedRoutingConfig;

    fn resolved_credentials(&self) -> Result<Self::Output, elevon_config::ConfigError> {
        let port = self
            .port
            .as_ref()
            .map(|port| -> Result<u16> {
                match port {
                    PortConfig::Literal(port) => Ok(*port),
                    PortConfig::Env(key) => {
                        let value =
                            resolve_env_or_literal(key).map_err(|e| ConfigError::InvalidValue {
                                field: "routing.port_env".to_string(),
                                value: e.to_string(),
                            })?;
                        let port = value
                            .parse::<u16>()
                            .map_err(|_| ConfigError::InvalidValue {
                                field: "routing.port".to_string(),
                                value,
                            })?;
                        Ok(port)
                    }
                }
            })
            .transpose()
            .map_err(|e| ConfigError::InvalidValue {
                field: "routing.port".to_string(),
                value: e.to_string(),
            })?;

        Ok(ResolvedRoutingConfig {
            domain: self.domain.clone(),
            port,
            tls: self.tls.clone(),
            static_files: self.static_files.clone(),
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct StaticConfig {
    pub dir: String,

    /// Index file name that will be served as root
    #[serde(default = "StaticConfig::default_index")]
    pub index: String,

    /// Domain path that the directory will be served at
    pub path: Option<String>,
}

impl StaticConfig {
    fn default_index() -> String {
        "index.html".to_string()
    }

    pub fn to_static_app(&self, domain: String) -> StaticApp {
        StaticApp {
            domain,
            directory: self.dir.clone(),
            index: self.index.clone(),
            path: self.path.clone(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
pub enum BuildConfig {
    Path(String),
    Options(BuildOptions),
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig::Path(".".to_string())
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(default)]
pub struct BuildOptions {
    pub path: String,
    pub dockerfile: Option<String>,
}

impl Default for BuildOptions {
    fn default() -> Self {
        Self {
            path: ".".to_string(),
            dockerfile: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ElevonConfig for BuildConfig {}

    #[test]
    fn test_template() -> Result<()> {
        let template_str = include_str!("../templates/config.yml");
        Config::from_str(template_str)?;
        Ok(())
    }

    #[test]
    fn test_build_config() -> Result<()> {
        let build_config_str = r#"
            build:
                path: ./dev
                dockerfile: ./Dockerfile.dev
        "#;

        BuildConfig::from_str(build_config_str)?;

        Ok(())
    }
}
