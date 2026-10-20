use std::str::FromStr;

use anyhow::Result;
use bollard::plugin::RestartPolicyNameEnum;
use elevon_contracts::deploy::{
    AppHealthCheckConfig, AppPayload, AppRuntimeOptions, StaticApp, WebApp,
};
use serde::{Deserialize, Serialize};
use strum_macros::{Display, EnumString};
use tabled::Tabled;

use crate::api::db::models::{App, TabledView, app::AppRoleDb};

#[derive(Debug, toasty::Model)]
pub struct Deployment {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[index]
    pub app_id: uuid::Uuid,

    #[index]
    pub image_ref: Option<String>,

    #[index]
    pub container_id: Option<String>,

    pub web: Option<DeploymentWeb>,

    pub static_web: Option<DeploymentStaticWeb>,

    #[default(DeploymentStatus::Pending)]
    pub status: DeploymentStatus,

    #[belongs_to(key = app_id, references = id)]
    pub app: toasty::Deferred<App>,

    #[has_one]
    pub options: toasty::Deferred<Option<DeploymentOption>>,

    #[auto]
    pub created_at: jiff::Timestamp,

    #[auto]
    pub updated_at: jiff::Timestamp,
}

impl Deployment {
    pub async fn create_deployment(
        db: &mut toasty::Db,
        app: App,
        payload: AppPayload,
        port: Option<u16>,
    ) -> Result<Self> {
        let web = match (&payload.web_app, port) {
            (Some(web_app), Some(new_port)) => Some(DeploymentWeb {
                domain: web_app.domain.clone(),
                port: new_port,
            }),
            _ => None,
        };

        let static_web = match &payload.static_app {
            Some(static_app) => Some(DeploymentStaticWeb {
                domain: static_app.domain.clone(),
                directory: static_app.directory.clone(),
                index: static_app.index.clone(),
                path: static_app.path.clone(),
            }),
            None => None,
        };

        let deployment = Deployment::create()
            .app_id(app.id)
            .web(web)
            .static_web(static_web)
            .exec(db)
            .await?;

        let runtime_options = DeploymentRuntimeOptions::from(&payload.runtime_options);

        DeploymentOption::create()
            .deployment_id(deployment.id)
            .runtime(runtime_options)
            .exec(db)
            .await?;

        Ok(deployment)
    }

    pub async fn get_latest_deployment(
        db: &mut toasty::Db,
        app_id: &uuid::Uuid,
    ) -> Result<Option<Deployment>> {
        let deployment = Deployment::_get_by_app_id(db, app_id, DeploymentStatus::Active).await?;
        Ok(deployment)
    }

    pub async fn get_previous_deployment(
        db: &mut toasty::Db,
        app_id: &uuid::Uuid,
    ) -> Result<Option<Deployment>> {
        let deployment = Deployment::_get_by_app_id(db, app_id, DeploymentStatus::Drained).await?;
        Ok(deployment)
    }

    pub async fn list_by_app_id(
        db: &mut toasty::Db,
        app_id: &uuid::Uuid,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Self>> {
        Ok(Deployment::filter_by_app_id(app_id)
            .latest_by(Deployment::fields().updated_at())
            .limit(limit)
            .offset(offset)
            .exec(db)
            .await?)
    }

    // It would be better this to be also checking the containers status just to make sure its running or not
    async fn _get_by_app_id(
        db: &mut toasty::Db,
        app_id: &uuid::Uuid,
        status: DeploymentStatus,
    ) -> Result<Option<Self>> {
        let deployment = Deployment::filter(
            Deployment::fields()
                .app_id()
                .eq(app_id)
                .and(Deployment::fields().status().eq(status)),
        )
        .include(Deployment::fields().app())
        .include(Deployment::fields().options())
        .latest_by(Deployment::fields().updated_at())
        .first()
        .exec(db)
        .await?;

        Ok(deployment)
    }
}

#[derive(
    Debug, Clone, Default, Serialize, Deserialize, toasty::Embed, PartialEq, Eq, Display, EnumString,
)]
#[strum(serialize_all = "lowercase")]
pub enum DeploymentStatus {
    #[default]
    Pending,
    Active,
    Drained,
    Failed,
}

#[derive(Debug, Clone, toasty::Embed)]
pub struct DeploymentWeb {
    pub domain: String,
    pub port: u16,
}

impl From<&DeploymentWeb> for WebApp {
    fn from(value: &DeploymentWeb) -> Self {
        Self {
            domain: value.domain.clone(),
            port: value.port,
        }
    }
}

#[derive(Debug, Clone, toasty::Embed)]
pub struct DeploymentStaticWeb {
    pub domain: String,
    pub directory: String,
    pub index: String,
    pub path: Option<String>,
}

impl From<&DeploymentStaticWeb> for StaticApp {
    fn from(value: &DeploymentStaticWeb) -> Self {
        Self {
            domain: value.domain.clone(),
            directory: value.directory.clone(),
            index: value.index.clone(),
            path: value.path.clone(),
        }
    }
}

#[derive(Clone, Tabled, Serialize, Deserialize)]
pub struct DeploymentTabled {
    pub id: String,
    pub app: String,
    pub container_id: String,
    pub domain: String,
    pub port: String,
    pub status: String,
    pub updated_at: String,
    pub created_at: String,
}

impl TabledView for Deployment {
    type TabledType = DeploymentTabled;

    fn to_tabled(&self) -> Self::TabledType {
        Self::TabledType {
            id: self.id.to_string(),
            app: self.app.get().name.clone(),
            container_id: self
                .container_id
                .as_deref()
                .map(|s| s[..12.min(s.len())].to_string())
                .unwrap_or_else(Self::default_value),
            domain: self
                .web
                .as_ref()
                .map(|w| w.domain.clone())
                .unwrap_or_else(Self::default_value),
            port: self
                .web
                .as_ref()
                .map(|w| w.port.to_string())
                .unwrap_or_else(Self::default_value),
            status: self.status.to_string(),
            updated_at: self.updated_at.to_string(),
            created_at: self.created_at.to_string(),
        }
    }
}

#[derive(Debug, toasty::Model)]
pub struct DeploymentOption {
    #[key]
    #[auto]
    pub id: uuid::Uuid,

    #[unique]
    pub deployment_id: uuid::Uuid,

    #[default(AppRoleDb::Web)]
    pub role: AppRoleDb,

    pub runtime: DeploymentRuntimeOptions,

    #[default(DeploymentHealthCheck::default())]
    pub healthcheck: DeploymentHealthCheck,

    #[belongs_to(key = deployment_id, references = id)]
    pub deployment: toasty::Deferred<Deployment>,
}

#[derive(Debug, Default, toasty::Embed)]
pub struct DeploymentRuntimeOptions {
    /// Comma separated string
    pub cmd: Option<String>,
    pub restart: Option<String>,
    pub memory_limit: Option<i64>,
    pub cpu_limit: Option<i64>,
    pub network: Option<String>,
}

impl From<&AppRuntimeOptions> for DeploymentRuntimeOptions {
    fn from(value: &AppRuntimeOptions) -> Self {
        Self {
            cmd: value.cmd.clone().map(|c| c.join(",")),
            restart: value.restart.as_ref().map(ToString::to_string),
            memory_limit: value.memory_limit,
            cpu_limit: value.cpu_limit,
            network: value.network.clone(),
        }
    }
}

impl From<&DeploymentRuntimeOptions> for AppRuntimeOptions {
    fn from(value: &DeploymentRuntimeOptions) -> Self {
        Self {
            cmd: value
                .cmd
                .clone()
                .map(|s| s.split(',').map(|item| item.trim().to_string()).collect()),
            restart: value
                .restart
                .clone()
                .map(|r| RestartPolicyNameEnum::from_str(&r).unwrap()),
            memory_limit: value.memory_limit,
            cpu_limit: value.cpu_limit,
            network: value.network.clone(),
        }
    }
}

#[derive(Debug, toasty::Embed)]
pub struct DeploymentHealthCheck {
    pub interval: u64,
    pub timeout: u64,
    pub retries: u64,
    pub stabilization: u64,
    pub required_successes: u64,
    pub path: Option<String>,
}

impl Default for DeploymentHealthCheck {
    fn default() -> Self {
        let default_config = AppHealthCheckConfig::default();
        DeploymentHealthCheck::from(&default_config)
    }
}

impl From<&AppHealthCheckConfig> for DeploymentHealthCheck {
    fn from(value: &AppHealthCheckConfig) -> Self {
        Self {
            interval: value.interval,
            timeout: value.timeout,
            retries: value.retries,
            stabilization: value.stabilization,
            required_successes: value.required_successes,
            path: value.path.clone(),
        }
    }
}

impl From<&DeploymentHealthCheck> for AppHealthCheckConfig {
    fn from(value: &DeploymentHealthCheck) -> Self {
        Self {
            interval: value.interval,
            timeout: value.timeout,
            retries: value.retries,
            stabilization: value.stabilization,
            required_successes: value.required_successes,
            path: value.path.clone(),
        }
    }
}
