mod actions;
mod app;
mod cli;
mod commands;
mod events;
mod extras;
mod history;
mod host;
mod models;
mod sessions;
mod view;

use std::{
    io::{IsTerminal, Write},
    path::PathBuf,
    process::ExitCode,
};

use clap::Parser;
use futures_util::StreamExt;
use nanocode::{
    ModelSelection, ServiceTier, Thinking,
    agent::events::AgentEventKind,
    tools::attachment::{RunnerConfig, RunnerEndpoint, run_runner, run_runner_with_tools},
};

use crate::{
    cli::{Cli, Command, HarnessArg, RunnerArgs, SessionArgs, SessionsArgs},
    host::{Host, HostConfig, Result},
};

pub fn main() -> ExitCode {
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("nanocodex: failed to start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let outcome = runtime.block_on(async move {
        match cli.command {
            Some(Command::Runner(args)) => runner(args).await,
            Some(Command::Sessions(args)) => list_sessions(args).await,
            Some(Command::Exec(args)) => exec(args).await,
            Some(Command::Models) => {
                print_models();
                Ok(())
            }
            None => interactive(cli.session).await,
        }
    });
    runtime.shutdown_timeout(std::time::Duration::from_secs(1));
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("nanocodex: {error}");
            ExitCode::FAILURE
        }
    }
}

struct Prepared {
    host: Host,
    model: ModelSelection,
    thinking: Thinking,
    tier: ServiceTier,
    home: PathBuf,
}

async fn prepare(args: &SessionArgs, notify: &mut dyn FnMut(&str)) -> Result<Prepared> {
    let workspace = match &args.workspace {
        Some(path) => std::fs::canonicalize(path)
            .map_err(|error| format!("workspace {}: {error}", path.display()))?,
        None => std::env::current_dir()?,
    };
    let home = host::home_dir(args.home.as_deref())?;
    let model = match (&args.model, args.harness) {
        (Some(model), harness) => {
            let model = host::parse_model(model)?;
            if let Some(harness) = harness
                && family(harness) != model.family()
            {
                return Err(format!(
                    "model {model} does not belong to the {} harness",
                    family(harness)
                )
                .into());
            }
            model
        }
        (None, Some(harness)) => family(harness).default_model(),
        (None, None) => nanocode::HarnessFamily::Codex.default_model(),
    };
    let defaulted = args.model.is_none() && args.harness.is_none();
    let capabilities = model.capabilities(nanocode::ModelTransport::Native);
    let thinking = match &args.thinking {
        Some(value) => host::parse_thinking(value)?,
        None if defaulted && capabilities.supports_thinking(Thinking::Xhigh) => Thinking::Xhigh,
        None => model.default_thinking(),
    };
    model
        .capabilities(nanocode::ModelTransport::Native)
        .check_thinking(thinking)?;
    let tier = match &args.tier {
        Some(value) => host::parse_tier(value)?,
        None if defaulted && capabilities.supports_service_tier(ServiceTier::Fast) => {
            ServiceTier::Fast
        }
        None => ServiceTier::Standard,
    };
    model
        .capabilities(nanocode::ModelTransport::Native)
        .check_service_tier(tier)?;
    let store = if args.ephemeral {
        None
    } else {
        Some({
            let home = home.clone();
            tokio::task::spawn_blocking(move || host::open_store(&home)).await??
        })
    };
    let remote = host::attach_remote(&args.remote, &workspace, notify).await?;
    let config = HostConfig {
        workspace,
        store,
        subagents: !args.no_subagents,
        remote,
    };
    let host = Host::build(config, &model, thinking)?;
    Ok(Prepared {
        host,
        model,
        thinking,
        tier,
        home,
    })
}

const fn family(harness: HarnessArg) -> nanocode::HarnessFamily {
    match harness {
        HarnessArg::Codex => nanocode::HarnessFamily::Codex,
        HarnessArg::Claude => nanocode::HarnessFamily::Claude,
    }
}

async fn interactive(args: SessionArgs) -> Result<()> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(
            "the interactive interface needs a terminal; use `nanocodex exec <prompt>` for scripts"
                .into(),
        );
    }
    let mut terminal =
        nanocode_tui::terminal::TerminalSession::enter(nanocode_tui::terminal::TerminalOptions {
            mouse_capture: !args.no_mouse,
            keyboard_enhancement: true,
        })?;
    let theme = nanocode_tui::theme::Theme::default();
    terminal.draw(|frame| view::splash(frame, &theme, "starting…"))?;
    nanocode_tui::highlight::prewarm();
    let prepared = {
        let mut notify =
            |message: &str| drop(terminal.draw(|frame| view::splash(frame, &theme, message)));
        prepare(&args, &mut notify).await
    };
    let prepared = match prepared {
        Ok(prepared) => prepared,
        Err(error) => {
            drop(terminal);
            return Err(error);
        }
    };
    let (resume, pick_session) = match args.resume.as_deref() {
        Some("") => (None, true),
        Some(id) => (Some(id.to_owned()), false),
        None if args.continue_last => match latest_session(&prepared.host).await {
            Ok(id) => (Some(id), false),
            Err(error) => {
                drop(terminal);
                return Err(error);
            }
        },
        None => (None, false),
    };
    if (resume.is_some() || pick_session) && prepared.host.config.store.is_none() {
        drop(terminal);
        return Err("--resume and --continue require durability; remove --ephemeral".into());
    }
    let remote_label = prepared
        .host
        .config
        .remote
        .as_ref()
        .map(|remote| remote.label.clone());
    app::run(
        app::Launch {
            host: prepared.host,
            model: prepared.model,
            thinking: prepared.thinking,
            tier: prepared.tier,
            resume,
            pick_session,
            prompt: args.prompt,
            remote_label,
            history_path: Some(prepared.home.join("history.jsonl")),
            home: prepared.home.clone(),
            thinking_override: args
                .thinking
                .as_deref()
                .map(host::parse_thinking)
                .transpose()?,
            tier_override: args.tier.as_deref().map(host::parse_tier).transpose()?,
        },
        terminal,
    )
    .await
}

async fn latest_session(host: &Host) -> Result<String> {
    let store = host
        .config
        .store
        .as_ref()
        .ok_or("--continue requires durability")?;
    store
        .list()
        .await?
        .into_iter()
        .find(|session| {
            sessions::user_facing(session.record.lineage.origin)
                && session.record.workspace.as_deref() == Some(host.config.workspace.as_path())
        })
        .map(|session| session.record.session_id)
        .ok_or_else(|| "no saved session for this workspace".into())
}

async fn exec(args: SessionArgs) -> Result<()> {
    let prompt = args
        .prompt
        .clone()
        .filter(|prompt| !prompt.trim().is_empty())
        .ok_or("exec requires a prompt")?;
    let prepared = prepare(&args, &mut |message| eprintln!("nanocodex: {message}")).await?;
    let host = prepared.host;
    let (agent, mut events) = match (&args.resume, args.continue_last) {
        (Some(id), _) if !id.is_empty() => host.open(id).await?,
        (_, true) => host.open(&latest_session(&host).await?).await?,
        _ => host.start(&prepared.model, prepared.thinking).await?,
    };
    app::apply_tier(&agent, &prepared.model, prepared.tier).await?;
    if let Some(reason) = host.runtime.computer_use_unavailable() {
        eprintln!("nanocodex: computer use unavailable, continuing without it: {reason}");
    }
    tokio::spawn(async move { while events.recv().await.is_some() {} });
    let mut turn = agent.prompt(prompt.as_str()).await?;
    let mut stderr = std::io::stderr();
    while let Some(event) = turn.next().await {
        let payload =
            serde_json::from_str::<serde_json::Value>(event.payload.get()).unwrap_or_default();
        match event.kind {
            AgentEventKind::ToolCall => {
                drop(writeln!(
                    stderr,
                    "· {}",
                    payload["tool"].as_str().unwrap_or("tool")
                ));
            }
            AgentEventKind::RunError if payload["observational"] == true => {
                drop(writeln!(
                    stderr,
                    "warning: {}",
                    payload["error"].as_str().unwrap_or_default()
                ));
            }
            _ => {}
        }
    }
    let result = turn.await?;
    println!("{}", result.final_message());
    drop(writeln!(stderr, "session {}", agent.session_id()));
    agent.flush().await?;
    host.runtime.shutdown().await?;
    Ok(())
}

async fn list_sessions(args: SessionsArgs) -> Result<()> {
    let home = host::home_dir(args.home.as_deref())?;
    let store = host::open_store(&home)?;
    let workspace = std::env::current_dir()?;
    for session in store.list().await? {
        let record = &session.record;
        if !sessions::user_facing(record.lineage.origin) {
            continue;
        }
        if !args.all && record.workspace.as_deref() != Some(workspace.as_path()) {
            continue;
        }
        println!(
            "{}  {:>8}  {:<20}  {}",
            record.session_id,
            sessions::age(record.updated_at_ms),
            record.model.to_string(),
            sessions::title(&session)
        );
    }
    Ok(())
}

fn print_models() {
    for (family, models) in models::catalog() {
        println!("{}", app::family_label(&family));
        for model in models {
            let capabilities = model.capabilities(nanocode::ModelTransport::Native);
            let thinking = capabilities
                .thinking()
                .map(Thinking::as_str)
                .collect::<Vec<_>>()
                .join("/");
            let features = models::features(&model);
            println!(
                "  {:<20} thinking {:<30} default {:<7} {}",
                model.to_string(),
                thinking,
                capabilities.default_thinking().as_str(),
                features
            );
        }
    }
}

async fn runner(args: RunnerArgs) -> Result<()> {
    let workspace = match &args.workspace {
        Some(path) => std::fs::canonicalize(path)?,
        None => std::env::current_dir()?,
    };
    let endpoint = match (args.listen, args.connect) {
        (Some(address), _) => RunnerEndpoint::Listen(address),
        (None, Some(url)) => RunnerEndpoint::Connect(url),
        (None, None) => return Err("pass --listen <ADDR> or --connect <WS_URL>".into()),
    };
    let config = RunnerConfig {
        endpoint,
        token: args.token,
        workspace: workspace.clone(),
        name: args.name.clone(),
    };
    if args.cua {
        let computer =
            nanocode::ComputerTools::connect(nanocode::ComputerConfig::installed()?).await?;
        let tools = nanocode::Tools::builder()
            .without_defaults()
            .tool(computer.js())
            .tool(computer.reset())
            .build()?;
        eprintln!(
            "nanocodex runner {} serving {} with computer use",
            args.name,
            workspace.display()
        );
        run_runner_with_tools(config, tools).await?;
    } else {
        eprintln!(
            "nanocodex runner {} serving {}",
            args.name,
            workspace.display()
        );
        run_runner(config).await?;
    }
    Ok(())
}
