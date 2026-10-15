use bollard::plugin::RestartPolicyNameEnum;
use bytesize::ByteSize;
use elevon_contracts::deploy::{AppHealthCheckConfig, AppRole, TlsConfig};
use serde::{Deserialize, Deserializer};

use crate::config::env::EnvConfig;

#[derive(Debug, Default, Clone, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub role: AppRole,

    pub cmd: Option<String>,

    pub runtime: Option<AppRuntimeConfig>,

    pub env: Option<EnvConfig>,

    pub healthcheck: AppHealthCheckConfig,

    #[serde(skip)]
    pub tls: Option<TlsConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AppRuntimeConfig {
    pub restart: RestartPolicyNameEnum,

    #[serde(alias = "cpu_limit")]
    pub cpu: Option<CpuLimit>,

    #[serde(alias = "memory_limit")]
    pub memory: Option<MemoryLimit>,

    pub network: Option<String>,
}

impl Default for AppRuntimeConfig {
    fn default() -> Self {
        Self {
            restart: RestartPolicyNameEnum::UNLESS_STOPPED,
            cpu: None,
            memory: None,
            network: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CpuLimit(pub f64);

impl CpuLimit {
    /// Converts core count into Docker nanoCPUs (i64)
    pub fn nano_cpus(&self) -> i64 {
        (self.0 * 1_000_000_000.0) as i64
    }
}

impl<'de> Deserialize<'de> for CpuLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Float(f64),
            Str(String),
        }

        match Value::deserialize(deserializer)? {
            Value::Float(f) => Ok(CpuLimit(f)),
            Value::Str(s) => {
                let parsed: f64 = s.trim().parse().map_err(serde::de::Error::custom)?;
                Ok(CpuLimit(parsed))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryLimit(pub i64);

impl MemoryLimit {
    pub fn bytes(&self) -> i64 {
        self.0
    }
}

impl<'de> Deserialize<'de> for MemoryLimit {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Value {
            Num(i64),
            Str(String),
        }

        match Value::deserialize(deserializer)? {
            Value::Num(n) => Ok(MemoryLimit(n)),
            Value::Str(s) => {
                let bytes = s
                    .parse::<ByteSize>()
                    .map_err(serde::de::Error::custom)?
                    .as_u64();
                Ok(MemoryLimit(bytes as i64))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use elevon_config::ElevonConfig;

    const TEMPLATE_STR: &str = include_str!("../../templates/config.yml");

    impl ElevonConfig for AppConfig {}

    fn get_apps_section(yaml_str: &str) -> Option<String> {
        let mut in_apps = false;
        let mut result = Vec::new();

        for line in yaml_str.lines() {
            // Find the start of the 'apps:' section
            if !in_apps {
                let trimmed = line.trim();
                if trimmed.starts_with("# apps:") {
                    in_apps = true;
                    result.push(line);
                }
                continue;
            }

            // Stop when hitting the next unindented top-level key or block
            let is_comment = line.trim_start().starts_with('#');
            let leading_spaces = line.len() - line.trim_start().len();

            if !line.trim().is_empty() && leading_spaces == 0 && !is_comment && line.contains(':') {
                break;
            }

            result.push(line);
        }

        if result.is_empty() {
            None
        } else {
            Some(result.join("\n"))
        }
    }

    fn uncomment_yaml_block(commented_str: &str) -> String {
        commented_str
            .lines()
            .map(|line| {
                let trimmed = line.trim_start();
                if let Some(stripped) = trimmed.strip_prefix('#') {
                    // If there's a space after '#', strip it to preserve exact nested indentation
                    if let Some(stripped_str) = stripped.strip_prefix(' ') {
                        stripped_str
                    } else {
                        stripped
                    }
                } else {
                    line
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_apps_from_template() -> anyhow::Result<()> {
        let app_section = get_apps_section(TEMPLATE_STR).unwrap();
        let app_block = uncomment_yaml_block(&app_section);

        AppConfig::from_str(&app_block)?;

        Ok(())
    }
}
