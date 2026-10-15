use anyhow::{Context, Result, bail};
use clap::{Args, Subcommand};
use elevon_contracts::{
    cli::ListQueryParams, cli_verify_action_with_items, deploy::log_stream_events,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    api::{db::models::AuthKey, routes::cli::KeyListPayload, stream::ActionLogger},
    cli::CliCommand,
    socket::UnixClient,
};

#[derive(Args, Serialize, Deserialize)]
pub struct KeyCreateArgs {
    #[arg(
        value_name = "NAME",
        default_value = "Default",
        help = "Name of the key"
    )]
    pub name: String,
}

#[derive(Args, Clone, Serialize, Deserialize)]
pub struct KeyRevokeArgs {
    #[arg(
        value_name = "KEY",
        required = true,
        help = "A key ID/name or list of key IDs/names to revoke"
    )]
    pub keys: Vec<String>,

    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Args, Clone, Serialize, Deserialize)]
pub struct KeyDeleteArgs {
    #[arg(
        value_name = "KEY",
        required = true,
        help = "A key ID/name or list of key IDs/names to delete"
    )]
    pub keys: Vec<String>,

    #[arg(long, short = 'y')]
    pub yes: bool,
}

#[derive(Subcommand, Serialize, Deserialize)]
pub enum KeyCommands {
    #[command(about = "Create new auth keys")]
    Create(KeyCreateArgs),

    #[command(about = "List all keys")]
    List(ListQueryParams),

    #[command(about = "Revoke one or more keys")]
    Revoke(KeyRevokeArgs),

    #[command(about = "Delete one or more keys")]
    Delete(KeyDeleteArgs),
}

impl CliCommand for KeyCommands {
    async fn run(command: Self) -> Result<()> {
        let unix_client = UnixClient::new()?;
        const BASE_PATH: &str = "/cli/keys/local";

        match command {
            KeyCommands::Create(args) => {
                let event_stream = unix_client
                    .client
                    .post(unix_client.resolve_url(BASE_PATH))
                    .json(&json!({
                        "name": &args.name
                    }))
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(event_stream).await?;
            }
            KeyCommands::List(params) => {
                let path = format!("{BASE_PATH}/list");
                let event_stream = unix_client
                    .client
                    .get(unix_client.resolve_url(&path))
                    .query(&params)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(event_stream).await?;
            }
            KeyCommands::Revoke(args) => {
                cli_verify_action_with_items(args.yes, &args.keys, "This will revoke auth key(s)")?;

                let path = format!("{BASE_PATH}/revoke");
                let payload = KeyListPayload { data: args.keys };

                let event_stream = unix_client
                    .client
                    .post(unix_client.resolve_url(&path))
                    .json(&payload)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(event_stream).await?;
            }
            KeyCommands::Delete(args) => {
                cli_verify_action_with_items(args.yes, &args.keys, "This will delete auth key(s)")?;

                let path = format!("{BASE_PATH}/delete");
                let payload = KeyListPayload { data: args.keys };

                let event_stream = unix_client
                    .client
                    .post(unix_client.resolve_url(&path))
                    .json(&payload)
                    .send()
                    .await?
                    .bytes_stream();

                log_stream_events(event_stream).await?;
            }
        }

        Ok(())
    }
}

pub async fn handle_key_revoke(
    keys: Vec<String>,
    db: &mut toasty::Db,
    logger: &impl ActionLogger,
) -> Result<()> {
    for key in keys {
        let mut auth_key = AuthKey::get_by_name_or_id(db, &key)
            .await
            .context("failed to find key")?;

        if auth_key.revoked_at.is_some() {
            bail!("key is already revoked");
        }

        auth_key
            .update()
            .enabled(false)
            .revoked_at(jiff::Timestamp::now())
            .exec(db)
            .await?;

        logger
            .log(format!("Revoked auth key: '{key}'").into())
            .await?;
    }

    Ok(())
}

pub async fn handle_key_delete(
    keys: Vec<String>,
    db: &mut toasty::Db,
    logger: &impl ActionLogger,
) -> Result<()> {
    for key in keys {
        let auth_key = AuthKey::get_by_name_or_id(db, &key)
            .await
            .context("failed to find key")?;

        auth_key
            .delete()
            .exec(db)
            .await
            .context(format!("failed to delete key: {key}"))?;

        logger
            .log(format!("Deleted auth key: '{key}'").into())
            .await?;
    }

    Ok(())
}
