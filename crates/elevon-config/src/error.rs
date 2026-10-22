#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read config file `{path}`")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to parse config")]
    Parse(#[from] noyalib::Error),
    #[error("environment variable `{name}` is not set")]
    MissingEnv { name: String },
    #[error("invalid value for `{field}`: `{value}`")]
    InvalidValue { field: String, value: String },
}
