use anyhow::Result;
use elevon_config::ElevonConfig;
use elevon_deploy::{
    config::{Config, registry::RegistryConfig},
    image::{build_image, push_image},
};

#[tokio::test]
async fn test_build_image() -> Result<()> {
    let config = Config::from_file("../../tests/test_app/config.yml")?;
    let registry_config = RegistryConfig {
        server: "localhost:5000".to_string(),
        username: "test".to_string(),
        password: "test".to_string(),
    };

    let image = build_image(
        &config.image,
        &registry_config,
        &config.build,
        "../../tests/test_app/config.yml",
    )
    .await?;

    println!("res: {}", image);

    Ok(())
}
