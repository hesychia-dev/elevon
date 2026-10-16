use std::path::{Path, PathBuf};

use anyhow::Context;

use crate::config::{BuildConfig, BuildOptions};

pub fn image_reference(registry_server: &str, image_name: &str, tag: &str) -> String {
    format!(
        "{}/{}:{}",
        registry_server.trim_end_matches('/'),
        image_name.trim_start_matches('/'),
        tag,
    )
}

pub struct BuildContextProps {
    pub contenxt_dir: PathBuf,
    pub dockerfile: String,
    pub dockerfile_path: PathBuf,
}

pub fn get_build_context(
    config_path: impl AsRef<Path>,
    build_config: &BuildConfig,
) -> BuildContextProps {
    let config_dir = config_path
        .as_ref()
        .parent()
        .unwrap_or_else(|| Path::new("."));

    match build_config {
        BuildConfig::Path(path) => BuildContextProps {
            contenxt_dir: config_dir.join(path),
            dockerfile: "Dockerfile".to_string(),
            dockerfile_path: config_dir.join(path).join(".dockerignore"),
        },
        BuildConfig::Options(BuildOptions { path, dockerfile }) => {
            let dockerfile = dockerfile
                .clone()
                .unwrap_or_else(|| "Dockerfile".to_string());
            // In case of Docker file having
            let dockerfile_path = dockerfile
                .strip_suffix(".")
                .map(|s| config_dir.join(path).join(format!(".dockerignore.{s}")))
                .unwrap_or_else(|| config_dir.join(path).join(".dockerignore"));

            BuildContextProps {
                contenxt_dir: config_dir.join(path),
                dockerfile,
                dockerfile_path,
            }
        }
    }
}

pub fn tar_context(build_context: BuildContextProps) -> anyhow::Result<Vec<u8>> {
    use ignore::WalkBuilder;
    use std::fs::File;
    use tar::{Builder, EntryType, Header, HeaderMode};

    let root = build_context.contenxt_dir.canonicalize()?;

    anyhow::ensure!(
        root.is_dir(),
        "build context not a directory: {}",
        root.display()
    );

    let walker = WalkBuilder::new(&root)
        .add_custom_ignore_filename(build_context.dockerfile_path)
        .build();

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
            .strip_prefix(&root)
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
