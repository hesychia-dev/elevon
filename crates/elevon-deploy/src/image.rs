pub(crate) mod progress;
pub mod util;

use std::path::Path;

use anyhow::{Result, bail};
use bollard::{
    Docker,
    auth::DockerCredentials,
    body_full,
    query_parameters::{BuildImageOptionsBuilder, PushImageOptionsBuilder},
};
use tracing_indicatif::span_ext::IndicatifSpanExt;

use crate::{
    config::{BuildConfig, registry::RegistryConfig},
    util::get_image_tag,
};

use self::progress::{ProgressMode, drain_progress_stream, print_success};

#[tracing::instrument(
    name = "build",
    skip_all,
    fields(image = %image_name, registry = %registry_config.server)
)]
pub async fn build_image(
    image_name: &str,
    registry_config: &RegistryConfig,
    build_config: &BuildConfig,
    config_path: impl AsRef<Path>,
) -> Result<bool> {
    let build_context = util::get_build_context(config_path, build_config);
    let docker = Docker::connect_with_local_defaults()?;

    let image_tag = get_image_tag(&build_context)?;

    let image_reference = util::image_reference(&registry_config.server, image_name, &image_tag);

    let credentials = Some(DockerCredentials {
        username: Some(registry_config.username.clone()),
        password: Some(registry_config.password.clone()),
        serveraddress: Some(registry_config.server.clone()),
        ..Default::default()
    });

    let image_exists = match docker
        .inspect_registry_image(&image_reference, credentials)
        .await
    {
        Ok(_) => true,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => false,
        Err(err) => bail!("failed to inspect registry image: {}", err),
    };

    if image_exists && image_tag != "modified" {
        tracing::info!("Image is already pushed to registry");
        return Ok(false);
    }

    let status_msg = format!("building {image_reference}");
    tracing::Span::current().pb_set_message(&status_msg);

    let options = BuildImageOptionsBuilder::default()
        .dockerfile(&build_context.dockerfile)
        .t(&image_reference)
        .rm(true)
        .build();

    let tar = util::tar_context(build_context)?;
    let stream = docker.build_image(options, None, Some(body_full(tar.into())));

    drain_progress_stream(stream, ProgressMode::Build, "build").await?;

    print_success(&format!("Built {image_reference}"));

    Ok(true)
}

#[tracing::instrument(
    name = "push",
    skip_all,
    fields(image = %image_name, registry = %creds.server)
)]
pub async fn push_image(
    image_name: &str,
    creds: RegistryConfig,
    build_config: &BuildConfig,
    config_path: impl AsRef<Path>,
) -> Result<()> {
    let docker = Docker::connect_with_local_defaults()?;

    let build_context = util::get_build_context(config_path, build_config);
    let image_tag = get_image_tag(&build_context)?;

    let image_reference = util::image_reference(&creds.server, image_name, &image_tag);

    let options = PushImageOptionsBuilder::default().tag(&image_tag).build();
    let credentials = DockerCredentials {
        username: Some(creds.username),
        password: Some(creds.password),
        serveraddress: Some(creds.server.clone()),
        ..Default::default()
    };

    let status_msg = format!("pushing {image_reference}");
    tracing::Span::current().pb_set_message(&status_msg);

    let stream = docker.push_image(&image_reference, Some(options), Some(credentials));
    drain_progress_stream(stream, ProgressMode::Push, "push").await?;

    print_success(&format!("Pushed {image_reference}"));

    Ok(())
}
