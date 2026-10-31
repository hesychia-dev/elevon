use std::{io::Write, path::PathBuf};

use anyhow::{Result, bail};
use elevon_contracts::deploy::TlsType;

use crate::agent::{AgentPath, AppEnvOptions, AppType};

pub fn get_tls_file(tls_type: TlsType, options: AppEnvOptions) -> Result<PathBuf> {
    let path = match options.app_type {
        AppType::Agent => match tls_type {
            TlsType::Cert => AgentPath::AgentTlsCert,
            TlsType::Key => AgentPath::AgentTlsKey,
        },
        AppType::App => {
            let Some(deployment_id) = options.deployment_id else {
                bail!("No deployment ID was specified");
            };

            match tls_type {
                TlsType::Cert => AgentPath::AppTlsCert(options.app_id, deployment_id),
                TlsType::Key => AgentPath::AppTlsKey(options.app_id, deployment_id),
            }
        }
    }
    .ensure_parent_dir()?;

    Ok(path)
}

pub fn write_tls_file(content: &[u8], tls_type: TlsType, options: AppEnvOptions) -> Result<()> {
    let path = get_tls_file(tls_type, options)?;

    let mut file = AgentPath::create_and_restrict_permissions(path, true)?;

    file.write_all(content)?;

    Ok(())
}
