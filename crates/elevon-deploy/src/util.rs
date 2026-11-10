use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use ignore::{Walk, WalkBuilder};
use sha2::{Digest, Sha256};

use crate::image::util::BuildContextProps;

pub fn get_file_walker(build_context: &BuildContextProps) -> Walk {
    WalkBuilder::new(&build_context.context_dir)
        .hidden(false)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .filter_entry(|e| e.file_name() != ".git")
        .add_custom_ignore_filename(".dockerignore")
        .add_custom_ignore_filename(build_context.dockerignore_filename.clone())
        .build()
}

fn is_dotenv_file(file_name: &OsStr) -> bool {
    let Some(name) = file_name.to_str() else {
        return false;
    };
    name == ".env" || name.starts_with(".env.")
}

pub fn warn_if_dotenv_in_context(path: &Path) {
    if path.file_name().is_some_and(is_dotenv_file) {
        tracing::warn!(
            file = %path.display(),
            ".env file included in build context; add it to .dockerignore or .gitignore to keep secrets out of the image"
        );
    }
}

pub fn collect_context_files(build_context: &BuildContextProps) -> anyhow::Result<Vec<PathBuf>> {
    let mut paths: Vec<_> = get_file_walker(build_context)
        .filter_map(|result| result.ok())
        .filter(|entry| {
            entry
                .file_type()
                .is_some_and(|file_type| file_type.is_file())
        })
        .map(|entry| entry.into_path())
        .collect();

    paths.sort();
    Ok(paths)
}

/// Returns a short content hash of the build context (relative paths + file
/// bytes). Ignored files are excluded, so changes outside the context do not
/// change the tag.
pub fn get_image_tag(build_context: &BuildContextProps) -> anyhow::Result<String> {
    let paths = collect_context_files(build_context)?;

    let mut hasher = Sha256::new();
    for path in paths {
        let relative = path
            .strip_prefix(&build_context.context_dir)
            .unwrap_or(&path);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update([0]);
        hasher.update(std::fs::read(&path)?);
    }

    let digest = hasher.finalize();
    Ok(digest
        .iter()
        .take(6)
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

pub fn resolve_config_path(config: &Option<String>, project: &Option<String>) -> String {
    match (config, project) {
        (Some(path), None) => path.clone(),
        (None, Some(project)) => format!(".elevon/deploy.{project}.yml"),
        (None, None) => ".elevon/deploy.yml".to_string(),
        (Some(_), Some(_)) => unreachable!("clap rejects conflicting arguments"),
    }
}

#[cfg(test)]
mod tests {
    use elevon_config::ElevonConfig;

    use crate::{config::Config, image::util::get_build_context};

    use super::*;

    #[test]
    fn test_get_image_tag() -> anyhow::Result<()> {
        let config = Config::from_file("templates/config.yml")?;
        let build_context = get_build_context("./templates/config.yml", &config.build)?;

        let image_tag = get_image_tag(&build_context)?;

        assert_eq!(image_tag.len(), 12);
        assert!(image_tag.chars().all(|c| c.is_ascii_hexdigit()));

        Ok(())
    }

    #[test]
    fn test_is_dotenv_file() {
        assert!(is_dotenv_file(OsStr::new(".env")));
        assert!(is_dotenv_file(OsStr::new(".env.local")));
        assert!(is_dotenv_file(OsStr::new(".env.production")));
        assert!(!is_dotenv_file(OsStr::new("env")));
        assert!(!is_dotenv_file(OsStr::new(".environment")));
        assert!(!is_dotenv_file(OsStr::new("foo.env")));
    }
}
