mod error;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;
use std::path::Path;

pub use error::ConfigError;

/// Expand `$VAR` fields in place.
pub trait ResolveEnv {
    fn resolve_env(&mut self) -> Result<(), ConfigError>;
}

/// Resolve `$VAR`/env refs into a new map; config is left unchanged.
pub trait ResolveEnvCredentials {
    type Output;
    fn resolved_credentials(&self) -> Result<Self::Output, ConfigError>;
}

pub trait ElevonConfig: DeserializeOwned + Sized
where
    Self: 'static,
{
    fn from_str(yaml: &str) -> Result<Self, ConfigError> {
        let config: Self = noyalib::from_str(yaml)?;
        Ok(config)
    }

    fn from_file(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_str(&text)
    }
}

pub fn resolve_env_or_literal(value: &str) -> Result<String, ConfigError> {
    if let Some(name) = value.strip_prefix('$') {
        std::env::var(name).map_err(|_| ConfigError::MissingEnv {
            name: name.to_string(),
        })
    } else {
        Ok(value.to_string())
    }
}

pub fn deserialize_string_map<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: Deserializer<'de>,
{
    let raw: HashMap<String, noyalib::Value> = HashMap::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(k, v)| {
            let s = match v {
                noyalib::Value::String(s) => s,
                noyalib::Value::Number(n) => n.to_string(),
                noyalib::Value::Bool(b) => b.to_string(),
                noyalib::Value::Null => String::new(),
                other => {
                    return Err(serde::de::Error::custom(format!(
                        "env var `{k}` must be a scalar, got {other:?}"
                    )));
                }
            };
            Ok((k, s))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_env_or_literal() -> Result<(), ConfigError> {
        unsafe {
            std::env::set_var("TEST_ENV", "test");
        }

        let result_1 = resolve_env_or_literal("$TEST_ENV")?;
        let result_2 = resolve_env_or_literal("LITERAL_VALUE")?;

        assert_eq!(result_1, "test");
        assert_eq!(result_2, "LITERAL_VALUE");

        Ok(())
    }
}
