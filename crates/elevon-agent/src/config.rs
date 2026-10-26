use anyhow::{Context, Result};
use elevon_config::ElevonConfig;
use elevon_contracts::deploy::{TlsConfig, TlsType};
use elevon_fs::agent::{AppEnvOptions, tls::write_tls_file};
use serde::{Deserialize, Deserializer};

#[derive(Debug, Deserialize)]
pub struct AgentConfig {
    pub domain: String,
    pub port: u16,
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Deserialize)]
pub struct ProxyConfig {
    #[serde(
        default = "ProxyConfig::default_ports",
        deserialize_with = "coerce_string_port"
    )]
    pub port: String,
}

impl ProxyConfig {
    pub fn default_ports() -> String {
        "80:443".to_string()
    }
}

fn coerce_string_port<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum StringOrInt {
        String(String),
        Int(u64),
    }

    match StringOrInt::deserialize(deserializer)? {
        StringOrInt::String(s) => Ok(s),
        StringOrInt::Int(i) => Ok(i.to_string()),
    }
}

pub fn parse_port(ports: String) -> Result<(u16, Option<u16>)> {
    let mut parts = ports.split(":");

    let http = parts
        .next()
        .context("Missing HTTP port")?
        .parse::<u16>()
        .context("Invalid u16 for HTTP port")?;

    let https = parts
        .next()
        .map(|s| s.parse::<u16>().context("Invalid u16 for HTTPS port"))
        .transpose()?;

    Ok((http, https))
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub agent: AgentConfig,
    pub proxy: Option<ProxyConfig>,
    pub turso_remote_url: Option<String>,
}

impl Config {
    pub fn setup(&self) -> Result<()> {
        if let Some(tls) = &self.agent.tls {
            let cert_bytes = std::fs::read(tls.cert.clone())?;
            let key_bytes = std::fs::read(tls.key.clone())?;

            let options = AppEnvOptions::agent();

            write_tls_file(&cert_bytes, TlsType::Cert, options.clone())?;
            write_tls_file(&key_bytes, TlsType::Key, options)?;
        }

        Ok(())
    }
}

impl ElevonConfig for Config {}

#[cfg(test)]
mod tests {
    use super::*;

    impl ElevonConfig for ProxyConfig {}

    #[test]
    fn test_config_template() -> Result<()> {
        let template_str = include_str!("templates/config.yml");
        Config::from_str(template_str)?;

        Ok(())
    }

    #[test]
    fn test_config_tests() -> Result<()> {
        Config::from_file("tests/config_test_1.yml").context("config_test_1")?;
        Config::from_file("tests/config_test_2.yml").context("config_test_2")?;

        Ok(())
    }
}
