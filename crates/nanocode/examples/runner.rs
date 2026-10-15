#[cfg(not(target_family = "wasm"))]
mod native {
    use std::{error::Error, path::PathBuf, time::Duration};

    use nanocode::code_mode::{
        Tools,
        attachment::AttachmentTarget,
        remote::{SocketPolicy, serve_accept, serve_connect},
    };

    type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

    const HELP: &str = "Machine-backed Code Mode executor for a separate durable brain.

Usage: cargo run --locked -p nanocode --example runner -- [OPTIONS]

Options:
  --listen ADDRESS     Accept the brain connection on a loopback HOST:PORT
  --connect URL        Connect to a listening brain over ws:// or wss://
  --workspace PATH     Machine workspace (default: current directory)
  --token-env NAME     Credential environment variable (default: NANOCODE_RUNNER_TOKEN)
  --max-frame-bytes N   Explicit maximum inbound/outbound frame size
  --timeout-ms N        Explicit handshake, request and callback deadline
  -h, --help           Show help

Choose exactly one of --listen and --connect. Both policy values are required.
Use a TLS proxy for connections across machines. Share the token out of band.
The machine runs the evaluator and workspace tools. Inference, subagents and
durability remain on the brain. Stop this runner with Ctrl-C.
";

    struct Args {
        listen: Option<String>,
        connect: Option<String>,
        workspace: PathBuf,
        token_env: String,
        policy: SocketPolicy,
    }

    fn parse() -> Result<Option<Args>> {
        let values: Vec<String> = std::env::args().skip(1).collect();
        if values.iter().any(|arg| arg == "--help" || arg == "-h") {
            print!("{HELP}");
            return Ok(None);
        }
        let mut listen = None;
        let mut connect = None;
        let mut workspace = None;
        let mut token_env = None;
        let mut max_frame_bytes = None;
        let mut timeout_ms = None;
        let mut values = values.into_iter();
        while let Some(arg) = values.next() {
            let slot = match arg.as_str() {
                "--listen" => &mut listen,
                "--connect" => &mut connect,
                "--workspace" => &mut workspace,
                "--token-env" => &mut token_env,
                "--max-frame-bytes" => &mut max_frame_bytes,
                "--timeout-ms" => &mut timeout_ms,
                _ => return Err(format!("unknown option {arg}; see --help").into()),
            };
            let value = values
                .next()
                .ok_or_else(|| format!("{arg} requires a value"))?;
            if value.trim().is_empty() {
                return Err(format!("{arg} requires a nonempty value").into());
            }
            if slot.replace(value).is_some() {
                return Err(format!("{arg} was specified more than once").into());
            }
        }
        if listen.is_some() == connect.is_some() {
            return Err("choose exactly one of --listen and --connect".into());
        }
        let max_frame_bytes = max_frame_bytes
            .ok_or("--max-frame-bytes is required; choose a size for your tool inputs and outputs")?
            .parse::<usize>()?;
        let timeout_ms = timeout_ms
            .ok_or("--timeout-ms is required; choose a deadline for your workload")?
            .parse::<u64>()?;
        if max_frame_bytes == 0 || timeout_ms == 0 {
            return Err("frame size and timeout must be positive".into());
        }
        Ok(Some(Args {
            listen,
            connect,
            workspace: match workspace {
                Some(path) => path.into(),
                None => std::env::current_dir()?,
            },
            token_env: token_env.unwrap_or_else(|| "NANOCODE_RUNNER_TOKEN".into()),
            policy: SocketPolicy {
                max_frame_bytes,
                request_timeout: Duration::from_millis(timeout_ms),
            },
        }))
    }

    async fn run(args: Args) -> Result<()> {
        let token = std::env::var(&args.token_env)
            .map_err(|_| format!("set {} to the shared runner credential", args.token_env))?;
        if token.trim().is_empty() {
            return Err(format!("{} must contain a nonempty credential", args.token_env).into());
        }
        let workspace = tokio::fs::canonicalize(&args.workspace).await?;
        let tools = Tools::builder()
            .web_search(false)
            .image_generation(false)
            .add(nanocode::tools::WorkspaceTools::new(workspace.clone()))
            .build()?;
        if let Some(address) = args.listen {
            let listener = tokio::net::TcpListener::bind(&address).await?;
            if !listener.local_addr()?.ip().is_loopback() {
                return Err("plaintext listeners require a loopback address; use a TLS proxy for remote access".into());
            }
            eprintln!(
                "runner listening ws://{} workspace {}",
                listener.local_addr()?,
                workspace.display()
            );
            tokio::select! {
                result = serve_accept(&listener, &token, &workspace, tools, args.policy) => result?,
                result = tokio::signal::ctrl_c() => result?,
            }
        } else if let Some(url) = args.connect {
            let target = AttachmentTarget::new(url, token)?;
            eprintln!(
                "runner connecting {} workspace {}",
                target.endpoint(),
                workspace.display()
            );
            tokio::select! {
                result = serve_connect(target, &workspace, tools, args.policy) => result?,
                result = tokio::signal::ctrl_c() => result?,
            }
        }
        Ok(())
    }

    pub fn main() -> Result<()> {
        let Some(args) = parse()? else {
            return Ok(());
        };
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(run(args))
    }
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    if let Err(error) = native::main() {
        eprintln!("runner: {error}\nUse --help for invocation details.");
        std::process::exit(1);
    }
}

#[cfg(target_family = "wasm")]
fn main() {}
