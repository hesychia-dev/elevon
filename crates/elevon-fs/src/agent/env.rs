use std::{
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::agent::{AgentPath, AppEnvOptions, AppType};

fn read_env_file(path: impl AsRef<Path>) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();

    for item in dotenvy::from_path_iter(path)? {
        let (key, value) = item?;
        vars.insert(key, value);
    }

    Ok(vars)
}

fn write_env_file(path: impl AsRef<Path>, env: &HashMap<String, String>) -> Result<()> {
    let mut file = AgentPath::create_and_restrict_permissions(path, true)?;

    for (key, value) in env {
        writeln!(file, "{key}={value}")?;
    }

    Ok(())
}

fn get_app_env(options: AppEnvOptions) -> Result<PathBuf> {
    // This most likely wont happen but just making sure anyway
    if matches!(options.app_type, AppType::App) && options.app_id == "agent" {
        bail!("App cannot be named 'agent'");
    }

    let path = match options.app_type {
        AppType::Agent => AgentPath::AgentEnv,
        AppType::App => {
            let Some(deployment_id) = options.deployment_id else {
                bail!("No deployment ID was specified");
            };

            AgentPath::AppEnv(options.app_id, deployment_id)
        }
    }
    .ensure_parent_dir()?;

    if !path.exists() {
        AgentPath::create_and_restrict_permissions(&path, true)?;
    }

    Ok(path)
}

pub fn load_app_env(options: AppEnvOptions) -> Result<HashMap<String, String>> {
    let path = get_app_env(options).context("failed to resolve env file path while loading env")?;

    let mut env = HashMap::new();
    env.extend(read_env_file(path)?);

    Ok(env)
}

pub fn add_app_env(key: String, value: String, options: AppEnvOptions) -> Result<()> {
    let mut env = load_app_env(options.clone()).map_err(|e| {
        anyhow::anyhow!(
            "failed to resolve env file path while loading env trying to update env, {}",
            e.to_string()
        )
    })?;

    env.insert(key, value);

    let path = get_app_env(options.clone()).with_context(|| {
        format!(
            "failed to resolve env file path while updating env, {:?}",
            options
        )
    })?;

    write_env_file(&path, &env)
        .with_context(|| format!("failed to write env file {}", path.display()))?;

    Ok(())
}

pub fn add_app_env_vars(vars: HashMap<String, String>, options: AppEnvOptions) -> Result<()> {
    for (key, value) in vars {
        add_app_env(key, value, options.clone())?;
    }

    Ok(())
}
