use std::{net::SocketAddr, path::PathBuf};

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "nanocodex",
    version,
    about = "Interactive coding agent for Codex and Claude"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub session: SessionArgs,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Runner(RunnerArgs),
    Sessions(SessionsArgs),
    Exec(SessionArgs),
    Models,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HarnessArg {
    Codex,
    Claude,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum RunnerModeArg {
    #[default]
    Alongside,
    Replace,
}

#[derive(Clone, Debug, Args)]
pub struct SessionArgs {
    pub prompt: Option<String>,

    #[arg(short, long, env = "NANOCODEX_MODEL")]
    pub model: Option<String>,

    #[arg(long, value_enum)]
    pub harness: Option<HarnessArg>,

    #[arg(short, long)]
    pub thinking: Option<String>,

    #[arg(long, value_parser = ["standard", "fast", "ultrafast"])]
    pub tier: Option<String>,

    #[arg(long)]
    pub pro: bool,

    #[arg(short = 'C', long = "cd")]
    pub workspace: Option<PathBuf>,

    #[arg(short, long, num_args = 0..=1, default_missing_value = "")]
    pub resume: Option<String>,

    #[arg(short = 'c', long = "continue")]
    pub continue_last: bool,

    #[arg(long)]
    pub ephemeral: bool,

    #[arg(long, env = "NANOCODEX_HOME")]
    pub home: Option<PathBuf>,

    #[arg(long)]
    pub no_mouse: bool,

    #[arg(long)]
    pub no_subagents: bool,

    #[command(flatten)]
    pub remote: RemoteArgs,
}

#[derive(Clone, Debug, Default, Args)]
pub struct RemoteArgs {
    #[arg(long, value_name = "WS_URL", conflicts_with = "runner_host")]
    pub runner_connect: Option<String>,

    #[arg(long, value_name = "ADDR")]
    pub runner_host: Option<SocketAddr>,

    #[arg(long, env = "NANOCODEX_RUNNER_TOKEN", hide_env_values = true)]
    pub runner_token: Option<String>,

    #[arg(long, default_value = "runner")]
    pub runner_name: String,

    #[arg(long, value_name = "PATH")]
    pub runner_workspace: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t)]
    pub runner_mode: RunnerModeArg,
}

#[derive(Clone, Debug, Args)]
pub struct RunnerArgs {
    #[arg(
        long,
        value_name = "ADDR",
        conflicts_with = "connect",
        required_unless_present = "connect"
    )]
    pub listen: Option<SocketAddr>,

    #[arg(long, value_name = "WS_URL")]
    pub connect: Option<String>,

    #[arg(long, env = "NANOCODEX_RUNNER_TOKEN", hide_env_values = true)]
    pub token: String,

    #[arg(short = 'C', long = "cd")]
    pub workspace: Option<PathBuf>,

    #[arg(long, default_value = "runner")]
    pub name: String,
}

#[derive(Clone, Debug, Args)]
pub struct SessionsArgs {
    #[arg(long, env = "NANOCODEX_HOME")]
    pub home: Option<PathBuf>,

    #[arg(long)]
    pub all: bool,
}
