use git2::Repository;
use ignore::{Walk, WalkBuilder};
use sha2::{Digest, Sha256};
use std::collections::HashSet;

use crate::image::util::BuildContextProps;

// TODO: hash (relative path + bytes) of every walked file instead of git status,
// so deleted/renamed files also change the tag.

// TODO(walker):
// - Disable global gitignore and `.git/info/exclude`, since Docker only reads `.dockerignore`.
// - Decide whether `.gitignore` should still filter, since Docker ignores it.
// - Warn when a `.env*` file ends up in the context.
pub fn get_file_walker(build_context: &BuildContextProps) -> Walk {
    WalkBuilder::new(&build_context.context_dir)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .add_custom_ignore_filename(".dockerignore")
        .add_custom_ignore_filename(build_context.dockerignore_filename.clone())
        .build()
}

// TODO: preserve file modes (exec bit) and handle symlinks instead of forcing 0o644.
//
/// Returns an image tag with the following style:
/// {image_repository}:{git_commit_sha}-{Option<{git diff sha}>}
/// On non-git repositories the image is being beilt every time
pub fn get_image_tag(build_context: &BuildContextProps) -> anyhow::Result<String> {
    let mut _commit_sha = "modified".to_string();

    let (is_dirty, mut changed_paths) = match Repository::discover(&build_context.context_dir) {
        Ok(repo) => {
            let workdir = repo
                .workdir()
                .ok_or_else(|| anyhow::anyhow!("Repository has no working directory"))?;

            let commit_sha = repo
                .head()
                .and_then(|h| h.peel_to_commit())
                .map(|c| c.id().to_string()[..7].to_string())
                .unwrap_or_else(|_| "modified".to_string()); // Assigns this String to commit_sha if it fails

            _commit_sha = commit_sha;

            let mut opts = git2::StatusOptions::new();
            opts.include_untracked(true).recurse_untracked_dirs(true);

            let mut paths = Vec::new();

            match repo.statuses(Some(&mut opts)) {
                Ok(statuses) => {
                    let walker = get_file_walker(build_context);

                    let valid_paths: HashSet<String> = walker
                        .filter_map(|result| result.ok())
                        .filter_map(|entry| entry.path().to_str().map(|s| s.to_string()))
                        .collect();

                    statuses.iter().for_each(|i| {
                        if let Ok(path) = i.path() {
                            let full_path = workdir.join(path);
                            if let Some(path_str) = full_path.to_str()
                                && valid_paths.contains(path_str)
                            {
                                paths.push(full_path);
                            }
                        }
                    });

                    (!paths.is_empty(), paths)
                }
                _ => (true, vec![]),
            }
        }
        // Not a git repository
        _ => (true, vec![]),
    };

    if !is_dirty || changed_paths.is_empty() {
        return Ok(_commit_sha);
    }

    changed_paths.sort();

    let mut hasher = Sha256::new();
    for path in changed_paths {
        if let Ok(bytes) = std::fs::read(&path) {
            hasher.update(&bytes);
        }
    }

    let hash_result = hasher.finalize();
    let diff_hash: String = hash_result
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();

    let short_diff_hash = &diff_hash[..7];

    if _commit_sha == "modified" {
        Ok(format!("modified-{short_diff_hash}"))
    } else {
        Ok(format!("{_commit_sha}-{short_diff_hash}"))
    }
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

        assert!(image_tag != "modified");

        Ok(())
    }
}
