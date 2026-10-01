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
mod geometry;
mod ipc;
mod luajson;
mod plugins;
mod runtime;
mod service;
mod skill;
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
    List {
        /// One JSON array, for scripts and the panel
        #[arg(long)]
        json: bool,
    },
    /// Switch a trigger back on
    Enable { id: String },
    /// Switch a trigger off (its hotkey is unbound, its timer stopped);
    /// the choice outlives reloads and restarts
    Disable { id: String },
    /// Let rules take chords Hyprland already has (on), give them back (off),
    /// or, alone, say which it is
    Override {
        #[arg(value_parser = ["on", "off"])]
        state: Option<String>,
    },
    /// Run a Lua chunk inside the daemon and print what it returns
    Eval { chunk: String },
    /// A line-by-line Lua console into the daemon (Ctrl+D to leave)
    Repl,
    /// Show what the daemon is doing
    Status {
        /// As JSON, for scripts and the panel
        #[arg(long)]
        json: bool,
    },
    /// Start the daemon through its systemd user unit
    Start,
    /// Stop the daemon: the unit's, or one started by hand
    Stop,
    /// Stop it and start it again (after `cargo install`, say)
    Restart,
    /// Open or close the rules panel of the Omarchy plugin
    Panel,
    /// Check the session and the tools the daemon needs
    Doctor {
        /// Remove binds left behind by a daemon that did not exit cleanly
        #[arg(long)]
        clear: bool,
    },
    /// The omaestro skill for AI coding agents: install it, or print it
    Skill {
        #[command(subcommand)]
        action: SkillCommand,
    },
    /// Plugins: Lua modules in ~/.config/omaestro/lib, loaded with om.use(name)
    Plugin {
        /// Directory with init.lua, rules.d/ and lib/ [default: ~/.config/omaestro]
        #[arg(long, env = "OMAESTRO_CONFIG_DIR", value_name = "DIR", global = true)]
        config_dir: Option<PathBuf>,
        #[command(subcommand)]
        action: PluginCommand,
    },
}

#[derive(Subcommand)]
enum SkillCommand {
    /// Write SKILL.md where Claude Code looks (~/.claude/skills/omaestro), or into DIR
    Install {
        #[arg(long, value_name = "DIR")]
        dir: Option<PathBuf>,
    },
    /// Print it, for any other agent or a read
    Show,
}

#[derive(Subcommand)]
enum PluginCommand {
    /// Install a plugin into lib/ and write the rule that loads it: a
    /// built-in one by name (see `available`), or a repository by git URL
    /// or user/repo on GitHub
    Add {
        #[arg(value_name = "NAME|URL")]
        what: String,
        /// A tag or branch to pin [default: the default branch]
        #[arg(long, value_name = "REF")]
        r#ref: Option<String>,
        /// Only install; do not write rules.d/NAME.lua
        #[arg(long)]
        no_rule: bool,
    },
    /// The plugins that ship with om, and which are installed
    Available,
    /// The installed plugins: name, version, Lua path, source
    List {
        /// As JSON
        #[arg(long)]
        json: bool,
    },
    /// Start a plugin of your own: lib/NAME with init.lua and README.md,
    /// git init, the rule that loads it, opened in $EDITOR
    New {
        name: String,
        /// Only create the files
        #[arg(long)]
        no_edit: bool,
        /// Do not write rules.d/NAME.lua
        #[arg(long)]
        no_rule: bool,
    },
    /// git pull the plugins that came from a repository (all, or one)
    Update { name: Option<String> },
    /// Delete a plugin from lib/
    Remove {
        name: String,
        /// Even with uncommitted or unpushed work in it
        #[arg(long)]
        force: bool,
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
        Command::List { json } => print_list(ask(&socket, Request::List).await?, json)?,
        Command::Enable { id } => print_message(ask(&socket, Request::Enable { id }).await?)?,
        Command::Disable { id } => print_message(ask(&socket, Request::Disable { id }).await?)?,
        Command::Override { state } => match state {
            Some(state) => {
                let on = state == "on";
                print_message(ask(&socket, Request::Override { on }).await?)?
            }
            None => {
                let status = payload::<Status>(ask(&socket, Request::Status).await?)?;
                println!("{}", override_line(status.override_binds));
            }
        },
        Command::Eval { chunk } => print_values(ask(&socket, Request::Eval { chunk }).await?)?,
        Command::Repl => repl(&socket).await?,
        Command::Status { json } => return status(&socket, json).await,
        Command::Start => service::start(&socket).await?,
        Command::Stop => service::stop(&socket).await?,
        Command::Restart => service::restart(&socket).await?,
        Command::Panel => service::panel().await?,
        Command::Doctor { clear } => {
            if !doctor::run(&socket, clear).await {
                return Ok(ExitCode::FAILURE);
            }
        }
        Command::Skill { action } => match action {
            SkillCommand::Install { dir } => {
                let path = skill::install(dir.as_deref())?;
                println!("installed the omaestro skill at {}", path.display());
            }
            SkillCommand::Show => skill::show(),
        },
        Command::Plugin { config_dir, action } => {
            let config_dir = match config_dir {
                Some(dir) => dir,
                None => config::default_config_dir()?,
            };
            let lib = config_dir.join("lib");
            match action {
                PluginCommand::Add {
                    what,
                    r#ref,
                    no_rule,
                } => plugins::add(&config_dir, &what, r#ref.as_deref(), !no_rule).await?,
                PluginCommand::Available => plugins::available(&config_dir)?,
                PluginCommand::List { json } => plugins::list(&lib, json).await?,
                PluginCommand::New {
                    name,
                    no_edit,
                    no_rule,
                } => plugins::new(&config_dir, &name, !no_edit, !no_rule).await?,
                PluginCommand::Update { name } => plugins::update(&lib, name.as_deref()).await?,
                PluginCommand::Remove { name, force } => {
                    plugins::remove(&config_dir, &name, force).await?
                }
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
    let state_dir = config::default_state_dir()?;
    fs::create_dir_all(&state_dir).with_context(|| format!("creating {}", state_dir.display()))?;
    let info = Info {
        config_dir: config_dir.clone(),
        hyprland_instance: instance.signature().to_string(),
        trigger_command: runtime::hotkeys::trigger_command(&exe, custom_socket),
        state_dir,
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

/// Reads Lua lines from stdin and runs each in the daemon, like `om eval`
/// in a loop. Errors are printed, not fatal.
async fn repl(socket: &Path) -> Result<()> {
    use std::io::{BufRead, Write};
    ask(socket, Request::Status).await?;
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    loop {
        print!("om> ");
        stdout.flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            println!();
            return Ok(());
        }
        let chunk = line.trim().to_string();
        if chunk.is_empty() {
            continue;
        }
        match ipc::request(socket, &Request::Eval { chunk }).await {
            Ok(response) if response.ok => {
                for value in
                    serde_json::from_value::<Vec<String>>(response.data.unwrap_or_default())
                        .unwrap_or_default()
                {
                    println!("{value}");
                }
            }
            Ok(response) => println!("error: {}", response.error.unwrap_or_default()),
            Err(err) => println!("error: {err:#}"),
        }
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

/// What `om status` and a bare `om override` say about the setting.
fn override_line(on: bool) -> &'static str {
    if on {
        "on (rules take chords Hyprland already has)"
    } else {
        "off (Hyprland's own binds win)"
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

fn print_list(response: Response, json: bool) -> Result<()> {
    let rows = payload::<Vec<TriggerRow>>(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    if rows.is_empty() {
        println!("no triggers registered");
        return Ok(());
    }
    let id_width = rows.iter().map(|row| row.id.len()).max().unwrap_or(0);
    let kind_width = rows.iter().map(|row| row.kind.len()).max().unwrap_or(0);
    for row in rows {
        let note = if !row.enabled {
            "  (disabled)".to_string()
        } else if let Some(problem) = &row.problem {
            format!("  refused: {problem}")
        } else if let Some(what) = &row.overrides {
            format!("  overrides: {what}")
        } else {
            String::new()
        };
        println!(
            "{:id_width$}  {:kind_width$}  {}{note}",
            row.id, row.kind, row.origin
        );
    }
    Ok(())
}

/// `om status`: what the daemon says and who keeps it running; or, when
/// nothing answers, what would. A failure exit then, for scripts.
async fn status(socket: &Path, json: bool) -> Result<ExitCode> {
    let response = match ipc::request(socket, &Request::Status).await {
        Ok(response) if response.ok => response,
        Ok(response) => bail!(
            response
                .error
                .unwrap_or_else(|| "the daemon reported an error".to_string())
        ),
        Err(err) => {
            if json {
                return Err(err);
            }
            println!("daemon:    not running (no answer on {})", socket.display());
            println!("unit:      {}", service::unit_hint().await);
            return Ok(ExitCode::FAILURE);
        }
    };
    let status = payload::<Status>(response)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(ExitCode::SUCCESS);
    }
    let supervisor = service::supervisor_of(status.pid).await;
    println!(
        "omaestro {}, pid {}, up {}s",
        status.version, status.pid, status.uptime_secs
    );
    println!("under:     {}", supervisor.describe());
    println!("hyprland:  {}", status.hyprland_instance);
    println!("config:    {}", status.config_dir);
    if status.files.is_empty() {
        println!("files:     none");
    } else {
        println!("files:     {}", status.files.join(", "));
    }
    if status.disabled > 0 {
        println!(
            "triggers:  {} ({} disabled)",
            status.triggers, status.disabled
        );
    } else {
        println!("triggers:  {}", status.triggers);
    }
    println!("override:  {}", override_line(status.override_binds));
    println!("running:   {} handler(s)", status.running);
    if status.reload_pending {
        println!("reload:    waiting for running handlers");
    }
    if let Some(error) = status.load_error {
        println!("error:     {error}");
    }
    Ok(ExitCode::SUCCESS)
}
