//! `unimcp`: the dashboard and commands for the student, and the MCP server
//! their AI assistant talks to.

mod accounts;
mod apps;
mod autostart;
mod config;
mod connectors;
mod dashboard;
mod hosting;
mod launch;
mod login;
mod prompts;
mod scheme;
mod server;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use uni_core::profile::Profile;
use uni_core::secrets::{KeyringStore, SecretStore};

use crate::connectors::Connectors;

#[derive(Parser)]
#[command(name = "unimcp", version, about)]
struct Cli {
    /// University to use instead of the one chosen on the dashboard.
    #[arg(long, env = "UNIMCP_PROFILE", global = true)]
    profile: Option<String>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the dashboard in this window: choose your university, log in, try the tools.
    Dashboard {
        /// Only print the address instead of opening the browser.
        #[arg(long)]
        no_browser: bool,
        /// For working on the page: serve only its API, for `npm run dev` in `web`.
        #[arg(long)]
        dev: bool,
    },
    /// Run the MCP server on stdio. This is the command an MCP client starts.
    Serve,
    /// Run the MCP server over HTTP, reachable from this computer only.
    Server {
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Folder for the accounts of the server. Defaults to one in your user folder.
        #[arg(long)]
        data: Option<PathBuf>,
        /// Start the server with this computer from now on, or stop doing so.
        #[arg(long, value_name = "on|off")]
        autostart: Option<Switch>,
    },
    /// Log in to a service, e.g. `unimcp login moodle`. `unimcp status` lists them.
    Login { service: String },
    /// Receives the sign-in link from the browser; not meant to be run by hand.
    #[command(hide = true)]
    LoginCallback { url: String },
    /// Forget the stored login for a service.
    Logout { service: String },
    /// Show which services are configured and logged in.
    Status,
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Switch {
    On,
    Off,
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            // In serve mode stdout belongs to the MCP protocol.
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let secrets: Arc<dyn SecretStore> = Arc::new(KeyringStore);
    // Without a command: the dashboard for a person, the server for a program.
    let by_a_person = cli.command.is_none() && std::io::stdin().is_terminal();
    if by_a_person && dashboard::DETACHABLE {
        return dashboard::open_detached();
    }
    let command = match cli.command {
        Some(command) => command,
        None if by_a_person => Command::Dashboard {
            no_browser: false,
            dev: false,
        },
        None => Command::Serve,
    };
    if let Command::Dashboard { no_browser, dev } = command {
        let mode = if dev {
            dashboard::Mode::Dev
        } else {
            dashboard::Mode::Page {
                open_browser: !no_browser,
            }
        };
        return dashboard::run(secrets, mode).await;
    }

    let profile_id = match cli.profile {
        Some(id) => id,
        None => config::load().map_err(|e| e.to_string())?.profile.ok_or(
            "no university selected yet; run `unimcp` to open the dashboard and select one",
        )?,
    };
    let profile = Profile::builtin(&profile_id).map_err(|e| e.to_string())?;
    let connectors = Connectors::new(profile.clone(), secrets.clone());
    // A broken config must not keep the MCP client from its tools.
    let custom_instructions = || {
        config::load()
            .map(|config| config.custom_instructions)
            .unwrap_or_default()
    };

    match command {
        Command::Dashboard { .. } => Ok(()),
        Command::Serve => server::serve(connectors, custom_instructions()).await,
        Command::Server {
            port,
            data,
            autostart,
        } => {
            let folder = data
                .or_else(|| {
                    Some(
                        dirs::data_dir()?
                            .join(env!("CARGO_PKG_NAME"))
                            .join("server"),
                    )
                })
                .ok_or("this system has no folder for program data; pass --data")?;
            match autostart {
                Some(Switch::On) => {
                    let launch = autostart::Launch::of_this_server(port, &folder)
                        .map_err(|e| e.to_string())?;
                    let note = autostart::turn_on(&launch).map_err(|e| e.to_string())?;
                    println!("The server now starts with this computer.");
                    if let Some(note) = note {
                        println!("{note}");
                    }
                    return Ok(());
                }
                Some(Switch::Off) => {
                    autostart::turn_off().map_err(|e| e.to_string())?;
                    println!("The server no longer starts with this computer.");
                    return Ok(());
                }
                None => {}
            }
            let accounts = accounts::Accounts::open(&folder).map_err(|e| e.to_string())?;
            let connectors = move || Connectors::new(profile.clone(), secrets.clone());
            server::serve_http(port, connectors, custom_instructions(), accounts, &folder).await
        }
        Command::Login { service } => login::login(&connectors, &service)
            .await
            .map_err(|e| e.to_string()),
        Command::LoginCallback { url } => {
            login::callback(&connectors, &url).map_err(|e| e.to_string())
        }
        Command::Logout { service } => {
            login::logout(&connectors, &service).map_err(|e| e.to_string())?;
            println!("Removed the stored login for {service}.");
            Ok(())
        }
        Command::Status => {
            for line in connectors.status() {
                println!("{line}");
            }
            Ok(())
        }
    }
}
