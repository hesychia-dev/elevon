use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{BuildConfig, BuildOptions};

pub fn image_reference(registry_server: &str, image_name: &str, tag: &str) -> String {
    format!(
        "{}/{}:{}",
        registry_server.trim_end_matches('/'),
        image_name.trim_start_matches('/'),
        tag,
    )
}

#[derive(Debug)]
pub struct BuildContextProps {
    pub context_dir: PathBuf,
    pub dockerfile: String,
    pub dockerignore_path: Option<PathBuf>,
}

pub fn get_build_context(
    config_path: impl AsRef<Path>,
    build_config: &BuildConfig,
) -> Result<BuildContextProps> {
    let config_dir = config_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new("."));

    let resolve_context_dir = |context_path: &str| -> Result<PathBuf> {
        let context_dir = config_dir.join(context_path).canonicalize()?;

        anyhow::ensure!(
            context_dir.is_dir(),
            "build context not a directory: {}",
            context_dir.display()
        );

        Ok(context_dir)
    };

    let context = match build_config {
        BuildConfig::Path(path) => {
            let context_dir = resolve_context_dir(path)?;
            let dockerignore_path = config_dir.join(".dockerignore").canonicalize().ok();

            BuildContextProps {
                context_dir,
                dockerfile: "Dockerfile".to_string(),
                dockerignore_path,
            }
        }
        BuildConfig::Options(BuildOptions { path, dockerfile }) => {
            let context_dir = resolve_context_dir(path)?;
            let dockerfile = dockerfile
                .clone()
                .unwrap_or_else(|| "Dockerfile".to_string());

            // In case of 'Dockerfile' having a suffix
            let dockerignore_path = dockerfile
                .strip_suffix(".")
                .map(|s| context_dir.join(format!(".dockerignore.{s}")))
                .unwrap_or_else(|| context_dir.join(path).join(".dockerignore"))
                .canonicalize()
                .ok();

            BuildContextProps {
                context_dir,
                dockerfile,
                dockerignore_path,
            }
        }
    };

    Ok(context)
}

pub fn tar_context(build_context: BuildContextProps) -> Result<Vec<u8>> {
    use ignore::WalkBuilder;
    use std::fs::File;
    use tar::{Builder, EntryType, Header, HeaderMode};

    let mut walker = WalkBuilder::new(&build_context.context_dir);

    if let Some(dockerignore_path) = build_context.dockerignore_path {
        walker.add_custom_ignore_filename(dockerignore_path);
    }

    let walker = walker.build();

    let mut ar = Builder::new(Vec::new());
    ar.mode(HeaderMode::Deterministic);

    for result in walker {
        let entry = result.context("failed to get entry")?;
        let path = entry.path();
        let metadata = entry
            .metadata()
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;

        if !metadata.is_file() {
            continue;
        }

        let relative_path = path
            .strip_prefix(&build_context.context_dir)
            .context("failed to make archive path relative")?;

        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(0o644);
        header.set_size(metadata.len());
        header.set_path(relative_path)?;
        header.set_cksum();
        ar.append(&header, File::open(path)?)?;
    }

    Ok(ar.into_inner()?)
}

#[cfg(test)]
mod tests {
    use elevon_config::ElevonConfig;

    use crate::config::Config;

    use super::*;

    #[test]
    fn test_build_context() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let build_context = get_build_context("./templates/config.yml", &config.build)?;

        assert_eq!(
            build_context
                .context_dir
                .file_name()
                .and_then(|n| n.to_str()),
            Some("elevon-deploy")
        );

        assert!(build_context.dockerignore_path.is_none());

        Ok(())
    }
}
