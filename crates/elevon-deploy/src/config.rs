pub mod app;
pub mod elevon;
pub mod env;
pub mod registry;

use std::collections::HashMap;

use anyhow::Result;
use elevon_config::{ElevonConfig, ResolveEnvCredentials};
use elevon_contracts::deploy::{AppRole, config::AppRoutingConfig};
use serde::Deserialize;
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    agent::{
        AgentClient, DeployService, prepare_apps_deploy_payload, prepare_apps_rollback_payload,
    },
    cli::{AppRollbackArgs, BuildArgs},
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

    pub routing: Option<AppRoutingConfig>,

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
        args: BuildArgs,
    ) -> Result<()> {
        let built = crate::image::build_image(
            &self.image,
            registry_config,
            &self.build,
            &config_path,
            &args,
        )
        .await?;

        if built && args.push {
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

    #[tracing::instrument(name = "deploy", skip_all)]
    pub async fn run_deploy(
        &self,
        agent_client: AgentClient,
        app_names: &[String],
        registry_config: RegistryConfig,
        config_path: &str,
    ) -> Result<()> {
        let selected_apps = self.get_selected_apps(app_names)?;
        let build_args = BuildArgs {
            push: true,
            ..Default::default()
        };

        self.run_build(&registry_config, config_path.to_string(), build_args)
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
