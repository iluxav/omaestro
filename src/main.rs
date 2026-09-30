//! `om`: the omaestro daemon and the thin client that talks to it.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::mpsc;

mod api;
mod backend;
mod chord;
mod config;
mod doctor;
mod ipc;
mod runtime;
#[cfg(test)]
mod testutil;

use backend::Backends;
use backend::hypr::{Instance, InstanceError};
use ipc::{Request, Response, Status, TriggerRow};
use runtime::{Event, Exit, Info, Runtime};

/// Exit code for "started outside the session": restarting cannot fix it,
/// so the systemd unit does not retry on this one.
const EXIT_NO_SESSION: u8 = 2;

#[derive(Parser)]
#[command(
    name = "om",
    version,
    about = "omaestro: Lua automation rules for Hyprland"
)]
struct Cli {
    /// Control socket [default: $XDG_RUNTIME_DIR/omaestro.sock]
    #[arg(long, global = true, env = "OMAESTRO_SOCKET", value_name = "PATH")]
    socket: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the daemon
    Daemon {
        /// Log for a terminal (timestamps, colors) instead of the journal
        #[arg(long)]
        foreground: bool,
        /// Directory with init.lua and rules.d/ [default: ~/.config/omaestro]
        #[arg(long, env = "OMAESTRO_CONFIG_DIR", value_name = "DIR")]
        config_dir: Option<PathBuf>,
    },
    /// Fire a trigger by name
    Trigger { id: String },
    /// Reload the rule files now
    Reload,
    /// List the registered triggers
    List,
    /// Run a Lua chunk inside the daemon and print what it returns
    Eval { chunk: String },
    /// Show what the daemon is doing
    Status,
    /// Check the session and the tools the daemon needs
    Doctor {
        /// Remove binds left behind by a daemon that did not exit cleanly
        #[arg(long)]
        clear: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")
        .and_then(|rt| rt.block_on(run(cli)));
    match result {
        Ok(code) => code,
        Err(err) => {
            eprintln!("om: {err:#}");
            if err.downcast_ref::<InstanceError>().is_some() {
                ExitCode::from(EXIT_NO_SESSION)
            } else {
                ExitCode::FAILURE
            }
        }
    }
}

async fn run(cli: Cli) -> Result<ExitCode> {
    let socket = match cli.socket {
        Some(path) => path,
        None => config::default_socket()?,
    };
    match cli.command {
        Command::Daemon {
            foreground,
            config_dir,
        } => daemon(&socket, config_dir, foreground).await?,
        Command::Trigger { id } => print_message(ask(&socket, Request::Trigger { id }).await?)?,
        Command::Reload => print_message(ask(&socket, Request::Reload).await?)?,
        Command::List => print_list(ask(&socket, Request::List).await?)?,
        Command::Eval { chunk } => print_values(ask(&socket, Request::Eval { chunk }).await?)?,
        Command::Status => print_status(ask(&socket, Request::Status).await?)?,
        Command::Doctor { clear } => {
            if !doctor::run(&socket, clear).await {
                return Ok(ExitCode::FAILURE);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn daemon(socket: &Path, config_dir: Option<PathBuf>, foreground: bool) -> Result<()> {
    init_logging(foreground);
    let instance = Instance::from_env()?;
    let config_dir = match config_dir {
        Some(dir) => dir,
        None => config::default_config_dir()?,
    };
    fs::create_dir_all(&config_dir)
        .with_context(|| format!("creating {}", config_dir.display()))?;

    let listener = ipc::bind(socket)?;
    let _socket_file = RemoveOnDrop(socket.to_path_buf());
    let (events, inbox) = mpsc::channel(64);
    // Watch before the first load so a save during startup is not missed.
    let _watcher = runtime::watch::spawn(&config_dir, events.clone())?;
    backend::hypr::events::listen(&instance, events.clone()).await?;

    let loader = {
        let dir = config_dir.clone();
        Box::new(move || runtime::source::load(&dir))
    };
    // Hyprland runs this on a hotkey. It has to reach this daemon, so a
    // socket other than the default one is spelled out.
    let exe = std::env::current_exe().context("finding the path of this binary")?;
    let custom_socket =
        (config::default_socket().ok().as_deref() != Some(socket)).then_some(socket);
    let info = Info {
        config_dir: config_dir.clone(),
        hyprland_instance: instance.signature().to_string(),
        trigger_command: runtime::hotkeys::trigger_command(&exe, custom_socket),
    };
    let runtime = Runtime::start(loader, Backends::real()?, info, events.clone())
        .await
        .map_err(anyhow::Error::msg)?;

    tokio::spawn(ipc::serve(listener, events.clone()));
    tokio::spawn(shutdown_on_signal(events));
    tracing::info!(
        "omaestro {} ready: rules in {}, socket {}",
        env!("CARGO_PKG_VERSION"),
        config_dir.display(),
        socket.display()
    );
    match runtime.run(inbox).await {
        Exit::Shutdown => {
            tracing::info!("stopped");
            Ok(())
        }
        // A failure exit, so systemd starts us again in the next session.
        Exit::HyprGone => bail!("Hyprland closed its event socket, the session is gone"),
    }
}

struct RemoveOnDrop(PathBuf);

impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

async fn shutdown_on_signal(events: mpsc::Sender<Event>) {
    let (Ok(mut terminate), Ok(mut interrupt)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        tracing::error!("could not install signal handlers; stop the daemon with SIGKILL");
        return;
    };
    tokio::select! {
        _ = terminate.recv() => {}
        _ = interrupt.recv() => {}
    }
    let _ = events.send(Event::Shutdown).await;
}

/// Logs go to stderr, which is the journal under systemd. `OMAESTRO_LOG`
/// sets the level (error, warn, info, debug, trace).
fn init_logging(foreground: bool) {
    let level = std::env::var("OMAESTRO_LOG")
        .ok()
        .and_then(|level| tracing::Level::from_str(&level).ok())
        .unwrap_or(tracing::Level::INFO);
    let builder = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(level)
        .with_target(false);
    if foreground {
        builder.init();
    } else {
        // The journal has its own timestamps and no use for color codes.
        builder.without_time().with_ansi(false).init();
    }
}

/// Sends a request and turns an error response into an error.
async fn ask(socket: &Path, request: Request) -> Result<Response> {
    let response = ipc::request(socket, &request).await?;
    if response.ok {
        Ok(response)
    } else {
        bail!(
            response
                .error
                .unwrap_or_else(|| "the daemon reported an error".to_string())
        )
    }
}

fn payload<T: serde::de::DeserializeOwned>(response: Response) -> Result<T> {
    serde_json::from_value(response.data.unwrap_or_default())
        .context("the daemon sent data this client does not understand (version mismatch?)")
}

fn print_message(response: Response) -> Result<()> {
    println!("{}", payload::<String>(response)?);
    Ok(())
}

fn print_values(response: Response) -> Result<()> {
    for value in payload::<Vec<String>>(response)? {
        println!("{value}");
    }
    Ok(())
}

fn print_list(response: Response) -> Result<()> {
    let rows = payload::<Vec<TriggerRow>>(response)?;
    if rows.is_empty() {
        println!("no triggers registered");
        return Ok(());
    }
    let id_width = rows.iter().map(|row| row.id.len()).max().unwrap_or(0);
    let kind_width = rows.iter().map(|row| row.kind.len()).max().unwrap_or(0);
    for row in rows {
        println!(
            "{:id_width$}  {:kind_width$}  {}",
            row.id, row.kind, row.origin
        );
    }
    Ok(())
}

fn print_status(response: Response) -> Result<()> {
    let status = payload::<Status>(response)?;
    println!(
        "omaestro {}, pid {}, up {}s",
        status.version, status.pid, status.uptime_secs
    );
    println!("hyprland:  {}", status.hyprland_instance);
    println!("config:    {}", status.config_dir);
    if status.files.is_empty() {
        println!("files:     none");
    } else {
        println!("files:     {}", status.files.join(", "));
    }
    println!("triggers:  {}", status.triggers);
    println!("running:   {} handler(s)", status.running);
    if status.reload_pending {
        println!("reload:    waiting for running handlers");
    }
    if let Some(error) = status.load_error {
        println!("error:     {error}");
    }
    Ok(())
}
