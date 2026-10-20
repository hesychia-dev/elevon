use std::{collections::HashMap, path::Path};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use elevon_config::ResolveEnvCredentials;
use elevon_contracts::deploy::{
    AppDeployPayload, AppPayload, AppRole, AppRollback, AppRollbackPayload, AppRuntimeOptions,
    StaticApp, WebApp, log_stream_events,
};
use reqwest::{
    Client, Url,
    header::{AUTHORIZATION, HeaderMap},
};

use crate::config::{BuildConfig, registry::RegistryConfig};
use crate::{
    config::{Config, app::AppConfig},
    util::get_image_tag,
};

#[async_trait]
pub trait DeployApi: Send + Sync {
    async fn deploy(&self, payload: AppDeployPayload) -> Result<()>;
    async fn rollback(&self, payload: AppRollbackPayload) -> Result<()>;
}

pub struct AgentClient {
    pub client: Client,
    base_url: Url,
    api_key: String,
}

impl AgentClient {
    pub fn new(base_url: &str, api_key: &str) -> Result<Self> {
        let base_url = Url::parse(base_url).context("[elevon.agent.url] invalid base URL")?;
        let client = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .build()?;

        Ok(Self {
            client,
            base_url,
            api_key: api_key.to_string(),
        })
    }

    pub fn absolute_url(&self, endpoint: &str) -> Url {
        self.base_url
            .join(endpoint)
            .expect("Failed to append endpoint")
    }

    pub fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            format!("Bearer {}", self.api_key).parse().unwrap(),
        );
        headers
    }
}

#[async_trait]
impl DeployApi for AgentClient {
    async fn deploy(&self, payload: AppDeployPayload) -> Result<()> {
        let resp = self
            .client
            .post(self.absolute_url("/deploy"))
            .headers(self.headers())
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;

        let stream = resp.bytes_stream();
        log_stream_events(stream).await?;

        Ok(())
    }

    async fn rollback(&self, payload: AppRollbackPayload) -> Result<()> {
        let resp = self
            .client
            .post(self.absolute_url("/deploy/rollback"))
            .headers(self.headers())
            .json(&payload)
            .send()
            .await?
            .error_for_status()?;

        let stream = resp.bytes_stream();
        log_stream_events(stream).await?;

        Ok(())
    }
}

pub struct DeployService<A: DeployApi> {
    api: A,
}

impl<A: DeployApi> DeployService<A> {
    pub fn new(api: A) -> Self {
        Self { api }
    }

    pub async fn push_deploy(&self, payload: AppDeployPayload) -> Result<()> {
        if payload.apps.is_empty() {
            bail!("No apps were found/selected")
        }
        self.api.deploy(payload).await
    }

    pub async fn push_rollback(&self, payload: AppRollbackPayload) -> Result<()> {
        self.api.rollback(payload).await
    }
}

pub fn prepare_apps_deploy_payload(
    config: &Config,
    registry_config: &RegistryConfig,
    selected_apps: Vec<(String, AppConfig)>,
    config_path: impl AsRef<Path>,
    build_config: &BuildConfig,
) -> Result<AppDeployPayload> {
    let build_context = crate::image::util::get_build_context(config_path, build_config)?;
    let image_tag = get_image_tag(&build_context).context("failed to get image tag")?;
    let project_vars = config.prepare_project_env_vars(registry_config)?;

    let apps_payload: Vec<AppPayload> = selected_apps
        .iter()
        .map(|(name, cfg)| -> Result<Option<AppPayload>> {
            let mut vars = cfg
                .env
                .as_ref()
                .map_or_else(|| Ok(HashMap::default()), |env| env.resolved_credentials())?;

            vars.extend(project_vars.clone());

            let tls_with_credentials = cfg
                .tls
                .as_ref()
                .map(|tls| tls.resolved_credentials())
                .transpose()?;

            let (web_app, static_app): (Option<WebApp>, Option<StaticApp>) = match &cfg.role {
                AppRole::Web => {
                    let resolved_routing = config.routing.resolved_credentials()?;

                    if let Some(static_files) = resolved_routing.static_files {
                        (
                            None,
                            Some(static_files.to_static_app(resolved_routing.domain)),
                        )
                    } else {
                        if let Some(port) = resolved_routing.port {
                            (
                                Some(WebApp {
                                    port,
                                    domain: resolved_routing.domain,
                                }),
                                None,
                            )
                        } else {
                            (None, None)
                        }
                    }
                }
                AppRole::Worker => (None, None),
            };

            let cmd = cfg.cmd.as_ref().and_then(|c| shlex::split(c));
            let runtime_config = cfg.runtime.clone().unwrap_or_default();

            let runtime_options = AppRuntimeOptions {
                cmd,
                restart: Some(runtime_config.restart),
                cpu_limit: runtime_config.cpu.as_ref().map(|c| c.nano_cpus()),
                memory_limit: runtime_config.memory.as_ref().map(|m| m.bytes()),
                network: runtime_config.network,
            };

            Ok(Some(AppPayload {
                image_ref: crate::image::util::image_reference(
                    &config.registry.server,
                    &config.image,
                    &image_tag,
                ),
                project: config.name.clone(),
                name: name.to_string(),
                role: cfg.role.clone(),
                healthcheck: cfg.healthcheck.clone(),
                keep_releases: config
                    .keep_releases
                    .unwrap_or_else(|| Config::default_keep_releases().unwrap()),
                runtime_options,
                vars: Some(vars),
                tls: tls_with_credentials,
                web_app,
                static_app,
            }))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    Ok(AppDeployPayload { apps: apps_payload })
}

pub fn prepare_apps_rollback_payload(
    config: &Config,
    selected_apps: Vec<(String, AppConfig)>,
    deployment_ids: Vec<String>,
) -> Result<AppRollbackPayload> {
    let apps_payload: Vec<AppRollback> = if deployment_ids.is_empty() {
        selected_apps
            .iter()
            .map(|(name, _)| AppRollback {
                project: config.name.clone(),
                name: name.clone(),
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(AppRollbackPayload {
        apps: apps_payload,
        deployment_ids,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use elevon_config::ElevonConfig;

    use super::*;

    #[derive(Clone, Default)]
    struct FakeApi {
        pub deploy_calls: Arc<Mutex<Vec<AppDeployPayload>>>,
        pub rollback_calls: Arc<Mutex<Vec<AppRollbackPayload>>>,
    }

    #[async_trait]
    impl DeployApi for FakeApi {
        async fn deploy(&self, payload: AppDeployPayload) -> Result<()> {
            self.deploy_calls.lock().unwrap().push(payload);
            Ok(())
        }

        async fn rollback(&self, payload: AppRollbackPayload) -> Result<()> {
            self.rollback_calls.lock().unwrap().push(payload);
            Ok(())
        }
    }

    fn set_config_env_variables() {
        unsafe {
            std::env::set_var("BASE_API_URL", "test");
            std::env::set_var("DATABASE_URL", "test");
            std::env::set_var("TLS_CERT", "test");
            std::env::set_var("TLS_KEY", "test");
        }
    }

    #[tokio::test]
    async fn test_push_deploy_rejects_empty_apps() {
        let fake = FakeApi::default();
        let service = DeployService::new(fake);

        let payload = AppDeployPayload { apps: vec![] };
        let err = service.push_deploy(payload).await.unwrap_err();

        assert!(err.to_string().contains("No apps"));
    }

    #[tokio::test]
    async fn test_prepare_apps_deploy_payload() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let config_path = "templates/config.yml".to_string();

        let selected_apps = config.get_selected_apps(&[])?;

        let registry_config = RegistryConfig {
            server: "test".to_string(),
            username: "test".to_string(),
            password: "test".to_string(),
        };

        set_config_env_variables();
        let payload = prepare_apps_deploy_payload(
            &config,
            &registry_config,
            selected_apps,
            config_path,
            &config.build,
        )?;

        assert_eq!(payload.apps.len(), 1);
        assert_eq!(payload.apps[0].name, "web");
        assert_eq!(payload.apps[0].role, AppRole::Web);

        Ok(())
    }

    #[tokio::test]
    async fn test_prepare_apps_rollback_payload() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let selected_apps = config.get_selected_apps(&[])?;

        let payload = prepare_apps_rollback_payload(&config, selected_apps, vec![])?;

        assert_eq!(payload.apps.len(), 1);
        assert_eq!(payload.apps[0].project, "project-name");
        assert_eq!(payload.apps[0].name, "web");

        Ok(())
    }

    #[tokio::test]
    async fn test_app_deploy_with_template_config() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let config_path = "templates/config.yml".to_string();

        let selected_apps = config.get_selected_apps(&[])?;

        let registry_config = RegistryConfig {
            server: "test".to_string(),
            username: "test".to_string(),
            password: "test".to_string(),
        };

        set_config_env_variables();
        let payload = prepare_apps_deploy_payload(
            &config,
            &registry_config,
            selected_apps,
            config_path,
            &config.build,
        )?;

        let fake = FakeApi::default();
        let service = DeployService::new(fake);

        service.push_deploy(payload).await?;

        assert_eq!(
            service.api.deploy_calls.lock().unwrap()[0].apps[0].name,
            "web"
        );

        Ok(())
    }

    #[tokio::test]
    async fn test_app_rollback_with_template_config() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let selected_apps = config.get_selected_apps(&[])?;

        let payload = prepare_apps_rollback_payload(&config, selected_apps, vec![])?;

        let fake = FakeApi::default();
        let service = DeployService::new(fake);

        service.push_rollback(payload).await?;

        assert_eq!(
            service.api.rollback_calls.lock().unwrap()[0].apps[0].name,
            "web"
        );

        Ok(())
    }
}
