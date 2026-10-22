//! The `icarus` command. With no arguments it opens the native app, which hosts a session that
//! the browser GUI, agents (`icarus mcp --connect`), OSC surfaces and scripts can all join.
//! `gui` does the same with only the browser GUI, `serve` runs headless with any adapters,
//! `mcp` lets an agent drive its own session or join a running one, and `demo` runs offline.

mod demo;
mod gui;
mod mcp;
mod osc;
mod socket;
mod ws;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use icarus_client::{Local, Remote};
use icarus_model::Project;
use icarus_session::Session;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub use icarus_client::Shared;

#[derive(Parser)]
#[command(
    version,
    about = "Icarus: an open, agent-native DAW",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    /// With no command, `icarus` opens the native app with these options.
    #[command(flatten)]
    app: AppArgs,
}

#[derive(Args, Clone)]
struct AppArgs {
    /// Join a running session through its JSON socket instead of hosting one, e.g. 127.0.0.1:7878.
    #[arg(long)]
    connect: Option<String>,
    /// Project file to open.
    #[arg(long)]
    project: Option<PathBuf>,
    /// Name your changes are attributed to.
    #[arg(long, default_value = "human")]
    name: String,
    /// Address to also serve the browser GUI on (same session).
    #[arg(long, default_value = "127.0.0.1:7880")]
    web: String,
    /// JSON socket that agents (`icarus mcp --connect`) and scripts join.
    #[arg(long, default_value = "127.0.0.1:7878")]
    socket: String,
    /// Also listen for OSC on this UDP port.
    #[arg(long)]
    osc: Option<u16>,
    /// Don't serve the browser GUI.
    #[arg(long)]
    no_web: bool,
    /// Run without audio output.
    #[arg(long)]
    no_live: bool,
}

#[derive(Subcommand)]
enum Cmd {
    /// Open the native app (the default when no command is given).
    App(AppArgs),
    /// Build a patch, sweep a filter with a recorded gesture, render it and probe it.
    Demo {
        /// Where to write the rendered audio.
        #[arg(long, default_value = "icarus-demo.wav")]
        out: PathBuf,
    },
    /// Open the stock GUI in your browser, with live audio and a socket agents can join.
    Gui {
        /// Project file to open.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Address for the GUI and its WebSocket.
        #[arg(long, default_value = "127.0.0.1:7880")]
        addr: String,
        /// JSON socket for scripts and `icarus mcp --connect`.
        #[arg(long, default_value = "127.0.0.1:7878")]
        socket: String,
        /// Also listen for OSC on this UDP port.
        #[arg(long)]
        osc: Option<u16>,
        /// Serve the GUI from this directory instead of the built-in copy, to edit it live.
        #[arg(long)]
        gui_dir: Option<PathBuf>,
        /// Run without audio output.
        #[arg(long)]
        no_live: bool,
        /// Don't open a browser.
        #[arg(long)]
        no_open: bool,
    },
    /// Run a session and expose it to clients.
    Serve {
        /// Project file to open.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Speak MCP on stdin/stdout, for agents such as Claude Code.
        #[arg(long)]
        mcp: bool,
        #[command(flatten)]
        adapters: Adapters,
        /// Play the session live through PipeWire.
        #[arg(long)]
        live: bool,
    },
    /// Speak MCP on stdin/stdout. With --connect, join a running session (e.g. the GUI's).
    Mcp {
        /// JSON socket of a running session, e.g. 127.0.0.1:7878.
        #[arg(long)]
        connect: Option<String>,
        /// Project file for a session of its own (ignored with --connect).
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

#[derive(Args, Default)]
struct Adapters {
    /// Listen for OSC on this UDP port (TouchOSC, VR, Max/Pd, ...).
    #[arg(long)]
    osc: Option<u16>,
    /// Accept line-delimited JSON requests on this TCP address, e.g. 127.0.0.1:7878.
    #[arg(long)]
    socket: Option<String>,
    /// Serve the GUI and WebSocket clients (browsers, WebXR, game engines) on this address.
    #[arg(long)]
    ws: Option<String>,
    /// Serve the GUI from this directory instead of the built-in copy.
    #[arg(long)]
    gui_dir: Option<PathBuf>,
}

impl Adapters {
    fn spawn(self, shared: &Shared) -> anyhow::Result<Vec<std::thread::JoinHandle<()>>> {
        let mut threads = Vec::new();
        if let Some(port) = self.osc {
            threads.push(osc::spawn(shared.clone(), port)?);
        }
        if let Some(addr) = self.socket {
            threads.push(socket::spawn(shared.clone(), &addr)?);
        }
        if let Some(addr) = self.ws {
            threads.push(ws::spawn(shared.clone(), &addr, self.gui_dir)?);
        }
        Ok(threads)
    }
}

fn open_session(project: Option<PathBuf>, live: bool) -> anyhow::Result<Shared> {
    let project = match project {
        Some(path) => serde_json::from_str(
            &std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
        )?,
        None => Project::default(),
    };
    let mut session = Session::new(project)?;
    if live {
        attach_live(&mut session)?;
    }
    Ok(Arc::new(Mutex::new(session)))
}

/// Starts an adapter, warning instead of failing: the app is still useful if a port is taken.
fn try_spawn(what: &str, spawned: anyhow::Result<std::thread::JoinHandle<()>>) -> bool {
    match spawned {
        Ok(_) => true,
        Err(e) => {
            eprintln!("icarus: {what} not started: {e}");
            false
        }
    }
}

#[cfg(feature = "app")]
fn run_app(args: AppArgs) -> anyhow::Result<()> {
    let client_name = format!("app:{}", args.name);
    if let Some(addr) = &args.connect {
        let (connection, events) = Remote::connect(addr)
            .with_context(|| format!("connecting to the session at {addr}; is Icarus running?"))?;
        let options = icarus_app::Options {
            client_name,
            web_url: None,
        };
        return icarus_app::run(Box::new(connection), events, options);
    }

    let shared = open_session(args.project, !args.no_live)?;
    if try_spawn("agent socket", socket::spawn(shared.clone(), &args.socket)) {
        eprintln!(
            "icarus: let an agent join with:  claude mcp add icarus -- icarus mcp --connect {}",
            args.socket
        );
    }
    let web_url = (!args.no_web && try_spawn("browser GUI", ws::spawn(shared.clone(), &args.web, None)))
        .then(|| format!("http://{}", args.web));
    if let Some(port) = args.osc {
        try_spawn("OSC", osc::spawn(shared.clone(), port));
    }
    let (connection, events) = Local::connect(shared);
    icarus_app::run(
        Box::new(connection),
        events,
        icarus_app::Options { client_name, web_url },
    )
}

#[cfg(not(feature = "app"))]
fn run_app(_args: AppArgs) -> anyhow::Result<()> {
    anyhow::bail!("this build has no native app; rebuild with the `app` feature, or use `icarus gui`")
}

fn mcp_local(shared: Shared) -> mcp::Backend {
    let (connection, events) = Local::connect(shared);
    mcp::Backend {
        connection: Box::new(connection),
        events,
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command.unwrap_or(Cmd::App(cli.app)) {
        Cmd::App(args) => run_app(args),
        Cmd::Demo { out } => demo::run(&out),
        Cmd::Gui {
            project,
            addr,
            socket,
            osc,
            gui_dir,
            no_live,
            no_open,
        } => {
            let shared = open_session(project, !no_live)?;
            let threads = Adapters {
                osc,
                socket: Some(socket.clone()),
                ws: Some(addr.clone()),
                gui_dir,
            }
            .spawn(&shared)?;
            eprintln!("icarus: let an agent join with:  claude mcp add icarus -- icarus mcp --connect {socket}");
            if !no_open {
                let _ = std::process::Command::new("xdg-open")
                    .arg(format!("http://{addr}"))
                    .spawn();
            }
            for t in threads {
                let _ = t.join();
            }
            Ok(())
        }
        Cmd::Serve {
            project,
            mcp,
            adapters,
            live,
        } => {
            let shared = open_session(project, live)?;
            let threads = adapters.spawn(&shared)?;
            if mcp {
                return mcp::run(mcp_local(shared));
            }
            anyhow::ensure!(
                !threads.is_empty(),
                "nothing to serve: pass --mcp, --osc, --socket or --ws"
            );
            for t in threads {
                let _ = t.join();
            }
            Ok(())
        }
        Cmd::Mcp { connect, project } => match connect {
            Some(addr) => {
                let (connection, events) = Remote::connect(&addr)
                    .with_context(|| format!("connecting to the session at {addr}; is Icarus running?"))?;
                mcp::run(mcp::Backend {
                    connection: Box::new(connection),
                    events,
                })
            }
            None => mcp::run(mcp_local(open_session(project, false)?)),
        },
    }
}

#[cfg(feature = "live")]
fn attach_live(session: &mut Session) -> anyhow::Result<()> {
    let live = icarus_pipewire::LiveOutput::start(session.project())?;
    session.set_sink(Box::new(live));
    eprintln!("icarus: live audio through PipeWire (press play to start)");
    Ok(())
}

#[cfg(not(feature = "live"))]
fn attach_live(_session: &mut Session) -> anyhow::Result<()> {
    anyhow::bail!("this build has no live audio; rebuild with the `live` feature")
}
