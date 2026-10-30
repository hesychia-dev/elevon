use std::collections::HashMap;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::{
    fs::{OpenOptions, create_dir_all},
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Ok, Result, bail};
use elevon_contracts::deploy::TlsType;

const API_SYSTEMD_SERVICE: &str = "elevon-agent-api.service";
const PROXY_SYSTEMD_SERVICE: &str = "elevon-agent-proxy.service";

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

#[derive(Clone)]
pub struct AppEnvOptions {
    pub project: String,
    pub app: Option<String>,
    pub deployment_id: Option<String>,
    pub bypass_default: Option<bool>,
}

impl AppEnvOptions {
    pub fn elevon() -> Self {
        Self {
            project: "agent".to_string(),
            app: None,
            deployment_id: None,
            bypass_default: Some(true),
        }
    }

    pub fn app(project: &str, app: &str, deployment_id: &str) -> Self {
        Self {
            project: project.to_string(),
            app: Some(app.to_string()),
            deployment_id: Some(deployment_id.to_string()),
            bypass_default: None,
        }
    }
}

pub fn get_app_env(options: AppEnvOptions) -> Result<PathBuf> {
    let bypass = options.bypass_default.unwrap_or(false);

    if options.project == "default" && !bypass && dev_root().is_none() {
        bail!("App can't be named 'default'");
    }

    let path = AgentPath::AppEnv(options.project, options.app, options.deployment_id)
        .ensure_parent_dir()?;

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .mode(0o600)
        .open(&path)?;

    drop(file);

    Ok(path)
}

pub fn load_app_env(options: AppEnvOptions) -> Result<HashMap<String, String>> {
    let app_path = get_app_env(options)?;

    let mut env = HashMap::new();
    env.extend(read_env_file(app_path)?);

    Ok(env)
}

pub fn load_app_string_env(options: AppEnvOptions) -> Result<Vec<String>> {
    Ok(load_app_env(options)?
        .iter()
        .map(|(key, val)| format!("{}={}", key, val))
        .collect())
}

pub fn add_app_env(key: String, value: String, options: AppEnvOptions) -> Result<()> {
    let mut env = load_app_env(options.clone())?;

    env.insert(key, value);

    let path = get_app_env(options).context("failed to resolve env file path")?;
    write_env_file(&path, &env)
        .with_context(|| format!("failed to write env file {}", path.display()))?;

    Ok(())
}

pub fn get_app_static_file(
    app_name: &str,
    deployment_id: &str,
    file_path: &str,
) -> Option<PathBuf> {
    let dir = AgentPath::AppStaticDir(app_name.to_string(), deployment_id.to_string())
        .ensure_parent_dir()
        .ok()?;
    let path = dir.join(file_path);
    path.is_file().then_some(path)
}

#[derive(Clone)]
pub struct TlsOptions {
    pub project: String,
    pub app: Option<String>,
}

pub fn get_tls_file(ty: TlsType, options: TlsOptions) -> Result<PathBuf> {
    let file_dir_name = match options.app {
        Some(app) => format!("projects/{}/{app}", options.project),
        None => options.project,
    };

    let path = AgentPath::ProjectTlsDir(format!("{file_dir_name}/{}", ty.get_file()))
        .ensure_parent_dir()?;

    Ok(path)
}

pub fn write_tls_file(content: &[u8], ty: TlsType, options: TlsOptions) -> Result<()> {
    let path = get_tls_file(ty, options)?;

    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .mode(0o600)
        .open(&path)?
        .write_all(content)?;

    Ok(())
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

fn read_env_file(path: impl AsRef<Path>) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();

    for item in dotenvy::from_path_iter(path)? {
        let (key, value) = item?;
        vars.insert(key, value);
    }

    Ok(vars)
}

fn write_env_file(path: impl AsRef<Path>, env: &HashMap<String, String>) -> Result<()> {
    let mut contents = String::new();

    for (key, value) in env {
        contents.push_str(&format!("{key}={value}\n"));
    }

    std::fs::write(path, contents)?;
    Ok(())
}

fn dev_root() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        Some(PathBuf::from("./devroot"))
    } else {
        None
    }
}

pub enum AgentPath {
    StateDir,
    UploadsDir,
    RuntimeDir,
    SystemdDir,
    Database,
    AppEnv(String, Option<String>, Option<String>),
    AppStaticDir(String, String),
    ProjectTlsDir(String),
    ProxySocket,
    ApiSocket,
    ApiSystemdPath,
    ProxySystemdPath,
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

            AgentPath::UploadsDir => {
                let dir = AgentPath::StateDir.resolve();
                dir.join("uploads")
            }

            AgentPath::RuntimeDir => base.join("run").join("elevon-agent"),

            AgentPath::SystemdDir => base.join("etc").join("systemd").join("system"),

            AgentPath::Database => {
                let dir = AgentPath::StateDir.resolve();
                dir.join("db").join("agent.db")
            }

            AgentPath::AppEnv(project, app, deployment_id) => {
                let dir = AgentPath::UploadsDir.resolve();
                let mut path = dir.join("env").join(project);

                if let Some(app) = app {
                    path = path.join(app);
                }

                if let Some(deployment_id) = deployment_id {
                    path = path.join(format!("dep-{deployment_id}"));
                }

                path.set_extension("env");
                path
            }

            AgentPath::AppStaticDir(app, deployment_id) => {
                let dir = AgentPath::UploadsDir.resolve();
                dir.join(app).join(deployment_id)
            }

            AgentPath::ProjectTlsDir(project) => {
                let dir = AgentPath::UploadsDir.resolve();
                dir.join("tls").join(project)
            }

            AgentPath::ProxySocket => {
                let dir = AgentPath::RuntimeDir.resolve();
                dir.join("proxy.sock")
            }

            AgentPath::ApiSocket => {
                let dir = AgentPath::RuntimeDir.resolve();
                dir.join("agent.sock")
            }

            AgentPath::ApiSystemdPath => {
                let dir = AgentPath::SystemdDir.resolve();
                dir.join(API_SYSTEMD_SERVICE)
            }

            AgentPath::ProxySystemdPath => {
                let dir = AgentPath::SystemdDir.resolve();
                dir.join(PROXY_SYSTEMD_SERVICE)
            }

            AgentPath::DeployLock => {
                let dir = AgentPath::RuntimeDir.resolve();
                dir.join("deploy.lock")
            }
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
}

pub fn remove_files(
    keep_database: bool,
    keep_env_files: bool,
    keep_systemd: bool,
    temp_file_config: &str,
) -> Result<()> {
    let runtime_dir = AgentPath::RuntimeDir.resolve();
    let database_file_path = AgentPath::Database.resolve();
    let uploads_dir = AgentPath::UploadsDir.resolve();

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

    if !keep_env_files && uploads_dir.exists() {
        println!("Removing environment and TLS files...");
        std::fs::remove_dir_all(uploads_dir)?;
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
