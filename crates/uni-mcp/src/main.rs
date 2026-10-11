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
mod store;
mod vault;

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

    if let Command::Server {
        port,
        data,
        autostart,
    } = command
    {
        return run_server(cli.profile, secrets, port, data, autostart).await;
    }

    let profile_id = match cli.profile {
        Some(id) => id,
        None => config::load().map_err(|e| e.to_string())?.profile.ok_or(
            "no university selected yet; run `unimcp` to open the dashboard and select one",
        )?,
    };
    let profile = Profile::builtin(&profile_id).map_err(|e| e.to_string())?;
    let connectors = Connectors::new(profile, secrets);
    // A broken config must not keep the MCP client from its tools.
    let custom_instructions = || {
        config::load()
            .map(|config| config.custom_instructions)
            .unwrap_or_default()
    };

    match command {
        Command::Dashboard { .. } | Command::Server { .. } => Ok(()),
        Command::Serve => server::serve(connectors, custom_instructions()).await,
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

/// The server mode works on a computer where no university was ever chosen:
/// then it serves the accounts and their dashboard only.
async fn run_server(
    profile: Option<String>,
    secrets: Arc<dyn SecretStore>,
    port: u16,
    data: Option<PathBuf>,
    autostart: Option<Switch>,
) -> Result<(), String> {
    let text = |e: uni_core::Error| e.to_string();
    let folder = data
        .or_else(|| {
            Some(
                dirs::data_dir()?
                    .join(env!("CARGO_PKG_NAME"))
                    .join("server"),
            )
        })
        .ok_or("this system has no folder for program data; pass --data")?;
    let database = store::Database::open(&folder).map_err(text)?;
    let accounts = accounts::Accounts::new(database.clone());

    match autostart {
        Some(Switch::On) => {
            // The background server has no terminal to show the link in.
            let now = accounts::now();
            if accounts.waits_for_its_owner(now).map_err(text)? {
                let invite = accounts.invite(now).map_err(text)?;
                println!("Create an admin account:");
                println!("{}", server::owner_link(port, &invite));
            }
            let launch = autostart::Launch::of_this_server(port, &folder).map_err(text)?;
            let note = autostart::turn_on(&launch, true).map_err(text)?;
            let when = match autostart::MOMENT {
                Some(autostart::Moment::Boot) => "whenever this computer starts",
                _ => "whenever you log in to this computer",
            };
            println!("The server is running in the background and starts {when}.");
            println!("Admin dashboard: http://127.0.0.1:{port}/server");
            if let Some(note) = note {
                println!("{note}");
            }
            return Ok(());
        }
        Some(Switch::Off) => {
            autostart::turn_off().map_err(text)?;
            println!("The server no longer starts by itself. A running one keeps running.");
            return Ok(());
        }
        None => {}
    }

    let chosen = profile.or_else(|| config::load().ok()?.profile);
    let connectors = match chosen {
        Some(id) => {
            let profile = Profile::builtin(&id).map_err(text)?;
            Some(move || Connectors::new(profile.clone(), secrets.clone()))
        }
        None => None,
    };
    let custom_instructions = config::load()
        .map(|config| config.custom_instructions)
        .unwrap_or_default();
    drop(accounts);
    server::serve_http(port, connectors, custom_instructions, database, &folder).await
}
