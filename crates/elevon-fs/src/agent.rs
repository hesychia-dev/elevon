pub mod env;
pub mod tls;

use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{OpenOptions, create_dir_all},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Result, bail};
const API_SYSTEMD_SERVICE: &str = "elevon-agent-api.service";
const PROXY_SYSTEMD_SERVICE: &str = "elevon-agent-proxy.service";

#[derive(Debug, Clone)]
pub enum AppType {
    Agent,
    App,
}

#[derive(Debug, Clone)]
pub struct AppEnvOptions {
    pub app_type: AppType,
    pub app_id: String,
    pub deployment_id: Option<String>,
}

impl AppEnvOptions {
    pub fn agent() -> Self {
        Self {
            app_type: AppType::Agent,
            app_id: "agent".to_string(),
            deployment_id: None,
        }
    }

    pub fn app(app_id: impl Into<String>, deployment_id: impl Into<String>) -> Self {
        Self {
            app_type: AppType::App,
            app_id: app_id.into(),
            deployment_id: Some(deployment_id.into()),
        }
    }
}

fn dev_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        Some(PathBuf::from("./devroot"))
    } else {
        None
    }
}

pub enum AgentPath {
    /// `/var/lib/elevon-agent`
    StateDir,

    /// `AgentPath::StateDir/{app_id}`
    ///
    /// `{app_id}` - Either `agent` or `apps/{app.id}`
    UploadsDir(String),

    /// `/run/elevon-agent`
    RuntimeDir,

    /// `/etc/systemd/system`
    SystemdDir,

    /// `AgentPath::StateDir/db/agent.db`
    Database,

    /// `AgentPath::UploadsDir("agent")`
    AgentDir,

    /// `AgentPath::AgentDir/.env`
    AgentEnv,

    /// `AgentPath::AgentDir/tls/cert.pem`
    AgentTlsCert,

    /// `AgentPath::AgentDir/tls/key.pem`
    AgentTlsKey,

    /// `AgentPath::UploadsDir/{deployment_id}`
    AppDeploymentDir(String, String),

    /// `AgentPath::AppDeploymentDir/.env`
    AppEnv(String, String),

    /// `AgentPath::AppDeploymentDir/static`,
    AppStaticDir(String, String),

    /// `AgentPath::AppDeploymentDir/tls/cert.pem`
    AppTlsCert(String, String),

    /// `AgentPath::AppDeploymentDir/tls/key.pem`
    AppTlsKey(String, String),

    /// `AgentPath::RuntimeDir/agent.sock`
    AgentSocket,

    /// `AgentPath::RuntimeDir/proxy.sock`
    ProxySocket,

    /// `AgentPath::SystemdDir/{API_SYSTEMD_SERVICE}`
    ApiSystemdPath,

    /// `AgentPath::SystemdDir/{PROXY_SYSTEMD_SERVICE}`
    ProxySystemdPath,

    /// `AgentPath::RuntimeDir/deploy.lock`
    DeployLock,
}

impl AgentPath {
    pub fn resolve(&self) -> PathBuf {
        let base = match dev_root() {
            Some(root) => root,
            None => PathBuf::from("/"),
        };

        match self {
            AgentPath::StateDir => base.join("var").join("lib").join("elevon-agent"),

            AgentPath::UploadsDir(app_id) => {
                AgentPath::StateDir.resolve().join("uploads").join(app_id)
            }

            AgentPath::RuntimeDir => base.join("run").join("elevon-agent"),

            AgentPath::SystemdDir => base.join("etc").join("systemd").join("system"),

            AgentPath::Database => AgentPath::StateDir.resolve().join("db").join("agent.db"),

            AgentPath::AgentDir => AgentPath::UploadsDir("agent".to_string()).resolve(),

            AgentPath::AgentEnv => AgentPath::AgentDir.resolve().join(".env"),

            AgentPath::AgentTlsCert => AgentPath::AgentDir.resolve().join("tls").join("cert.pem"),

            AgentPath::AgentTlsKey => AgentPath::AgentDir.resolve().join("tls").join("key.pem"),

            AgentPath::AppDeploymentDir(app_id, deploment_id) => {
                AgentPath::UploadsDir("apps".to_string())
                    .resolve()
                    .join(app_id)
                    .join(deploment_id)
            }

            AgentPath::AppEnv(app_id, deployment_id) => {
                AgentPath::AppDeploymentDir(app_id.clone(), deployment_id.clone())
                    .resolve()
                    .join(".env")
            }

            AgentPath::AppStaticDir(app_id, deployment_id) => {
                AgentPath::AppDeploymentDir(app_id.clone(), deployment_id.clone())
                    .resolve()
                    .join("static")
            }

            AgentPath::AppTlsCert(app_id, deployment_id) => {
                AgentPath::AppDeploymentDir(app_id.clone(), deployment_id.clone())
                    .resolve()
                    .join("tls")
                    .join("cert.pem")
            }

            AgentPath::AppTlsKey(app_id, deployment_id) => {
                AgentPath::AppDeploymentDir(app_id.clone(), deployment_id.clone())
                    .resolve()
                    .join("tls")
                    .join("key.pem")
            }

            AgentPath::AgentSocket => AgentPath::RuntimeDir.resolve().join("agent.sock"),

            AgentPath::ProxySocket => AgentPath::RuntimeDir.resolve().join("proxy.sock"),

            AgentPath::ApiSystemdPath => AgentPath::SystemdDir.resolve().join(API_SYSTEMD_SERVICE),

            AgentPath::ProxySystemdPath => {
                AgentPath::SystemdDir.resolve().join(PROXY_SYSTEMD_SERVICE)
            }

            AgentPath::DeployLock => AgentPath::RuntimeDir.resolve().join("deploy.lock"),
        }
    }

    pub fn ensure_dir(&self) -> Result<PathBuf> {
        let path = self.resolve();
        create_dir_all(&path)?;
        Ok(path)
    }

    pub fn ensure_parent_dir(&self) -> Result<PathBuf> {
        let path = self.resolve();

        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }

        Ok(path)
    }

    pub fn resolve_and_delete_if_exists(&self) -> PathBuf {
        let path = self.resolve();

        if path.exists() {
            let _ = std::fs::remove_file(&path);
        }

        path
    }

    pub fn create_and_restrict_permissions(
        path: impl AsRef<Path>,
        truncate: bool,
    ) -> Result<std::fs::File> {
        Ok(OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(truncate)
            .mode(0o600)
            .open(path)?)
    }

    pub fn restrict_permissions(path: impl AsRef<Path>) -> Result<()> {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o660))?;

        Ok(())
    }
}

pub fn get_database_path() -> Result<PathBuf> {
    let db_path = AgentPath::Database.ensure_parent_dir()?;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(&db_path)?;

    Ok(db_path)
}

pub fn get_api_systemd_content(exec: &str) -> String {
    format!(
        "
        [Unit]
        Description=Elevon Agent API
        After=network.target docker.socket
        Wants=docker.socket

        [Service]
        User=elevon-agent
        Group=elevon-agent

        ExecStart={exec} api

        SupplementaryGroups=docker
        StateDirectory=elevon-agent
        UMask=0007

        [Install]
        WantedBy=multi-user.target
        "
    )
}

pub fn get_proxy_systemd_content(exec: &str) -> String {
    format!(
        "
        [Unit]
        Description=Elevon Agent Proxy
        After=network.target {API_SYSTEMD_SERVICE}
        Wants={API_SYSTEMD_SERVICE}

        [Service]
        User=elevon-agent
        Group=elevon-agent
        ExecStart={exec} proxy

        TimeoutStopSec=40
        KillSignal=SIGTERM
        Restart=on-failure

        AmbientCapabilities=CAP_NET_BIND_SERVICE
        CapabilityBoundingSet=CAP_NET_BIND_SERVICE

        SupplementaryGroups=docker
        StateDirectory=elevon-agent
        UMask=0007

        [Install]
        WantedBy=multi-user.target
        "
    )
}

pub fn install_proxy_unit(exec: &Path) -> Result<()> {
    let unit = get_proxy_systemd_content(&exec.display().to_string());
    let dest = AgentPath::ProxySystemdPath.ensure_parent_dir()?;
    std::fs::write(dest, unit)?;
    Ok(())
}

pub fn install_api_unit(exec: &Path) -> Result<()> {
    let unit = get_api_systemd_content(&exec.display().to_string());
    let dest = AgentPath::ApiSystemdPath.ensure_parent_dir()?;
    std::fs::write(dest, unit)?;
    Ok(())
}

pub fn remove_files(
    keep_database: bool,
    keep_env_files: bool,
    keep_systemd: bool,
    temp_file_config: &str,
) -> Result<()> {
    let runtime_dir = AgentPath::RuntimeDir.resolve();
    let database_file_path = AgentPath::Database.resolve();
    let state_dir = AgentPath::StateDir.resolve();

    let api_systemd_path = AgentPath::ApiSystemdPath.resolve();
    let proxy_systemd_path = AgentPath::ProxySystemdPath.resolve();

    let mut services = Vec::new();
    if api_systemd_path.exists() {
        services.push(API_SYSTEMD_SERVICE);
    }
    if proxy_systemd_path.exists() {
        services.push(PROXY_SYSTEMD_SERVICE);
    }

    if !services.is_empty() {
        println!("Stopping Elevon services...");
        let status = Command::new("systemctl")
            .args(["stop"])
            .args(&services)
            .status()?;

        if !status.success() {
            bail!("failed to stop Elevon services: {status}");
        }
    }

    if !keep_systemd {
        println!("Removing systemd units...");
        if !services.is_empty() {
            let status = Command::new("systemctl")
                .args(["disable"])
                .args(&services)
                .status()?;

            if !status.success() {
                bail!("failed to disable Elevon services: {status}");
            }
        }

        if api_systemd_path.exists() {
            std::fs::remove_file(&api_systemd_path)?;
        }
        if proxy_systemd_path.exists() {
            std::fs::remove_file(&proxy_systemd_path)?;
        }
        if Path::new(temp_file_config).exists() {
            std::fs::remove_file(temp_file_config)?;
        }

        let status = Command::new("systemctl").args(["daemon-reload"]).status()?;

        if !status.success() {
            bail!("failed to reload systemd services: {status}");
        }
    }

    if !keep_systemd && runtime_dir.exists() {
        println!("Removing runtime directory...");
        std::fs::remove_dir_all(runtime_dir)?;
    }

    if !keep_database {
        println!("Removing database...");
        if database_file_path.exists() {
            std::fs::remove_file(&database_file_path)?;
        }

        if let Some(db_dir) = database_file_path.parent() {
            let _ = std::fs::remove_dir(db_dir);
        }
    } else {
        println!("Keeping database.");
    }

    if !keep_env_files && state_dir.exists() {
        println!("Removing environment and TLS files...");
        std::fs::remove_dir_all(state_dir)?;
    } else if keep_env_files {
        println!("Keeping environment and TLS files.");
    }

    if !keep_database && !keep_env_files {
        println!("Removing remaining agent state...");
        let state_dir = AgentPath::StateDir.resolve();
        let _ = std::fs::remove_dir_all(state_dir);
    }

    println!("Elevon uninstalled.");
    Ok(())
}

pub fn remove_app_files(options: AppEnvOptions) -> Result<()> {
    if !matches!(options.app_type, AppType::App) {
        bail!("This is not an app to remove");
    }

    let Some(deployment_id) = options.deployment_id else {
        bail!("No deployment ID was specified");
    };

    let dir = AgentPath::AppDeploymentDir(options.app_id, deployment_id).resolve();

    let _ = std::fs::remove_dir_all(dir);

    Ok(())
}
