//! `unimcp`: the dashboard and commands for the student, and the MCP server
//! their AI client talks to.

mod config;
mod connectors;
mod dashboard;
mod login;
mod scheme;
mod server;

use std::io::IsTerminal;
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
    /// Open the dashboard: choose your university, log in, try the tools.
    Dashboard {
        /// Only print the address instead of opening the browser.
        #[arg(long)]
        no_browser: bool,
        /// For working on the page: serve only its API, for `npm run dev` in `web`.
        #[arg(long)]
        dev: bool,
    },
    /// Run the MCP server on stdio (this is what an AI client starts).
    Serve,
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
    let command = cli.command.unwrap_or_else(|| {
        if std::io::stdin().is_terminal() {
            Command::Dashboard {
                no_browser: false,
                dev: false,
            }
        } else {
            Command::Serve
        }
    });
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
        None => config::load()
            .map_err(|e| e.to_string())?
            .profile
            .ok_or("no university chosen yet; run `unimcp` in a terminal to set it up")?,
    };
    let profile = Profile::builtin(&profile_id).map_err(|e| e.to_string())?;
    let connectors = Connectors::new(profile, secrets);

    match command {
        Command::Dashboard { .. } => Ok(()),
        Command::Serve => server::serve(connectors).await,
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
