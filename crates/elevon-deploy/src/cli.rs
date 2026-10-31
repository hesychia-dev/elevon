pub mod agent;
pub mod init;

use clap::{
    Args, Parser, Subcommand,
    builder::styling::{AnsiColor, Styles},
};

const STYLES: Styles = Styles::styled()
    .header(AnsiColor::Yellow.on_default())
    .usage(AnsiColor::Green.on_default())
    .literal(AnsiColor::Green.on_default())
    .placeholder(AnsiColor::Green.on_default());

#[derive(Parser)]
#[command(
    name = "elevon-deploy",
    version,
    about,
    long_about = "Elevon Deploy: build and push container images for Elevon apps.",
    styles = STYLES
)]
pub struct Cli {
    #[command(flatten)]
    pub args: DeployArgs,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Args, Clone)]
pub struct DeployArgs {
    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "config",
        help = "Project name used to find deploy.<name>.yml"
    )]
    pub project: Option<String>,

    #[arg(
        short,
        long,
        global = true,
        conflicts_with = "project",
        help = "Path to the deploy configuration file (defaults to .elevon/deploy.yml)"
    )]
    pub config: Option<String>,
}

#[derive(Subcommand)]
pub enum Commands {
    #[command(about = "Initialize a configuration file template")]
    Init,

    #[command(about = "Checks for deployment needed assets")]
    Check,

    #[command(about = "Upgrade the CLI binary")]
    Upgrade(UpgradeArgs),

    #[command(about = "Build container images from the deploy config")]
    Build(BuildArgs),

    #[command(about = "Push configured images to the registry")]
    Push,

    #[command(about = "Deploy applications")]
    Deploy(AppArgs),

    #[command(about = "Rollback applications to previous deployment")]
    Rollback(AppRollbackArgs),

    #[command(about = "Agent related commands")]
    Agent {
        #[command(subcommand)]
        command: agent::Commands,
    },
}

#[derive(Debug, Args)]
pub struct UpgradeArgs {
    #[arg(long, help = "The version to upgrade to")]
    pub version: Option<String>,
}

#[derive(Debug, Default, Args)]
pub struct BuildArgs {
    #[arg(long, short = 'P', help = "Push the image to the registry")]
    pub push: bool,

    #[arg(long, help = "Buld the image with no caching")]
    pub no_cache: bool,
}

#[derive(Debug, Args)]
pub struct AppArgs {
    #[arg(long = "app", help = "Limit the operation to the specified app(s)")]
    pub apps: Vec<String>,
}

#[derive(Debug, Args)]
#[command(group(
    clap::ArgGroup::new("rollback_target")
        .required(true) // Forces the user to pick at least one path
        .args(["apps", "ids"]),
))]
pub struct AppRollbackArgs {
    #[arg(
        long = "app",
        value_name = "APP",
        help = "Limit the operation to the specified app(s)",
        num_args = 1..,
        help_heading = "Targets" 
    )]
    pub apps: Vec<String>,

    #[arg(
        long = "id",
        value_name = "ID",
        help = "Specific deployment ID or list of them to rollback app(s) to",
        num_args = 1..,
        help_heading = "Targets" 
    )]
    pub ids: Vec<String>,
}
