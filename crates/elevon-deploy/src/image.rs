pub(crate) mod progress;
pub mod util;

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
    image::util::BuildContextProps,
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
    config_path: &str,
) -> Result<bool> {
    let docker = Docker::connect_with_local_defaults()?;

    let image_props = ImageProps::get_props(
        image_name,
        registry_config,
        build_config,
        config_path.to_string(),
    )?;

    let image_exists = match docker
        .inspect_registry_image(
            &image_props.image_reference,
            Some(image_props.credentials.clone()),
        )
        .await
    {
        Ok(_) => true,
        Err(bollard::errors::Error::DockerResponseServerError {
            status_code: 404, ..
        }) => false,
        Err(err) => bail!("failed to inspect registry image: {}", err),
    };

    if image_exists && image_props.image_tag != "modified" {
        return Ok(false);
    }

    let status_msg = format!("building {}", image_props.image_reference);
    tracing::Span::current().pb_set_message(&status_msg);

    let options = BuildImageOptionsBuilder::default()
        .dockerfile(&image_props.build_context.dockerfile)
        .t(&image_props.image_reference)
        .rm(true)
        .build();

    let tar = util::tar_context(image_props.build_context)?;
    let stream = docker.build_image(options, None, Some(body_full(tar.into())));

    drain_progress_stream(stream, ProgressMode::Build, "build").await?;

    print_success(&format!("Built {}", image_props.image_reference));

    Ok(true)
}

#[tracing::instrument(
    name = "push",
    skip_all,
    fields(image = %image_name, registry = %registry_config.server)
)]
pub async fn push_image(
    image_name: &str,
    registry_config: &RegistryConfig,
    build_config: &BuildConfig,
    config_path: &str,
) -> Result<()> {
    let docker = Docker::connect_with_local_defaults()?;

    let image_props = ImageProps::get_props(
        image_name,
        registry_config,
        build_config,
        config_path.to_string(),
    )?;

    let options = PushImageOptionsBuilder::default()
        .tag(&image_props.image_tag)
        .build();

    let status_msg = format!("pushing {}", image_props.image_reference);

    tracing::Span::current().pb_set_message(&status_msg);

    let stream = docker.push_image(
        &image_props.image_reference,
        Some(options),
        Some(image_props.credentials),
    );
    drain_progress_stream(stream, ProgressMode::Push, "push").await?;

    print_success(&format!("Pushed {}", image_props.image_reference));

    Ok(())
}

#[derive(Debug)]
struct ImageProps {
    build_context: BuildContextProps,
    image_tag: String,
    image_reference: String,
    credentials: DockerCredentials,
}

impl ImageProps {
    fn get_props(
        image_name: &str,
        registry_config: &RegistryConfig,
        build_config: &BuildConfig,
        config_path: String,
    ) -> Result<Self> {
        let build_context = util::get_build_context(config_path, build_config)?;
        let image_tag = get_image_tag(&build_context)?;

        let image_reference =
            util::image_reference(&registry_config.server, image_name, &image_tag);

        let credentials = DockerCredentials {
            username: Some(registry_config.username.clone()),
            password: Some(registry_config.password.clone()),
            serveraddress: Some(registry_config.server.clone()),
            ..Default::default()
        };

        Ok(Self {
            build_context,
            image_tag,
            image_reference,
            credentials,
        })
    }
}

#[cfg(test)]
mod tests {
    use elevon_config::ElevonConfig;

    use crate::config::Config;

    use super::*;

    #[test]
    fn test_image_props() -> Result<()> {
        let config = Config::from_file("templates/config.yml")?;

        let image_props = ImageProps::get_props(
            &config.name,
            &config.registry,
            &config.build,
            "templates/config.yml".to_string(),
        )?;

        assert_eq!(
            image_props.credentials.serveraddress,
            Some("ghcr.io".to_string())
        );

        Ok(())
    }
}
