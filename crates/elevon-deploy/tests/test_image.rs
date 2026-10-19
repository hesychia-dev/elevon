use anyhow::Result;
use elevon_config::ElevonConfig;
use elevon_deploy::{
    config::{Config, registry::RegistryConfig},
    image::{build_image, push_image},
};

const CONFIG_PATH: &str = "../../tests/test_app/config.yml";

fn get_config() -> Result<(Config, RegistryConfig)> {
    let config = Config::from_file(CONFIG_PATH)?;

    let registry_config = RegistryConfig {
        server: "localhost:5000".to_string(),
        username: "test".to_string(),
        password: "test".to_string(),
    };

    Ok((config, registry_config))
}

#[tokio::test]
#[cfg(feature = "heavy_tests")]
async fn test_build_and_push_image() -> Result<()> {
    let (config, registry_config) = get_config()?;

    build_image(&config.image, &registry_config, &config.build, CONFIG_PATH).await?;

    push_image(&config.image, &registry_config, &config.build, CONFIG_PATH).await?;

    let url = format!(
        "http://localhost:5000/v2/{}/manifests/{}",
        config.image, config.name
    );
    let client = reqwest::Client::new();

    let response = client
        .head(&url)
        .basic_auth("test", Some("test"))
        .send()
        .await?;

    println!("response: {:?}", response);

    Ok(())
}
