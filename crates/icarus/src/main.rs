//! The `icarus` command: a demo, and `serve`, which runs one session that any number of
//! agents, OSC surfaces, VR rigs and scripts can drive at the same time.

mod demo;
mod mcp;
mod osc;
mod socket;

use anyhow::Context;
use clap::{Parser, Subcommand};
use icarus_model::Project;
use icarus_session::Session;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// One session shared by every adapter.
pub type Shared = Arc<Mutex<Session>>;

#[derive(Parser)]
#[command(version, about = "Icarus: an open, agent-native DAW")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Build a patch, sweep a filter with a recorded gesture, render it and probe it.
    Demo {
        /// Where to write the rendered audio.
        #[arg(long, default_value = "icarus-demo.wav")]
        out: PathBuf,
    },
    /// Run a session and expose it to clients.
    Serve {
        /// Project file to open.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Speak MCP on stdin/stdout, for agents such as Claude Code.
        #[arg(long)]
        mcp: bool,
        /// Listen for OSC on this UDP port (TouchOSC, VR, Max/Pd, ...).
        #[arg(long)]
        osc: Option<u16>,
        /// Accept line-delimited JSON requests on this TCP address, e.g. 127.0.0.1:7878.
        #[arg(long)]
        socket: Option<String>,
        /// Play the session live through PipeWire.
        #[arg(long)]
        live: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Cmd::Demo { out } => demo::run(&out),
        Cmd::Serve {
            project,
            mcp,
            osc,
            socket,
            live,
        } => {
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
            let shared: Shared = Arc::new(Mutex::new(session));

            let mut threads = Vec::new();
            if let Some(port) = osc {
                threads.push(osc::spawn(shared.clone(), port)?);
            }
            if let Some(addr) = socket {
                threads.push(socket::spawn(shared.clone(), &addr)?);
            }
            if mcp {
                return mcp::run(shared);
            }
            anyhow::ensure!(!threads.is_empty(), "nothing to serve: pass --mcp, --osc or --socket");
            for t in threads {
                let _ = t.join();
            }
            Ok(())
        }
    }
}

#[cfg(feature = "live")]
fn attach_live(session: &mut Session) -> anyhow::Result<()> {
    let live = icarus_pipewire::LiveOutput::start(session.project())?;
    session.set_sink(Box::new(live));
    eprintln!("icarus: playing live through PipeWire");
    Ok(())
}

#[cfg(not(feature = "live"))]
fn attach_live(_session: &mut Session) -> anyhow::Result<()> {
    anyhow::bail!("this build has no live audio; rebuild with the `live` feature")
}
