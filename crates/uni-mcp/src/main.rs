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
mod oauth;
mod prompts;
mod scheme;
mod server;
mod store;
mod vault;

use std::io::IsTerminal;
use std::net::IpAddr;
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
    /// Run the MCP server over HTTP with accounts, for a home server or the cloud.
    Server {
        /// Address to listen on. The default only accepts this computer; in a
        /// container it is `0.0.0.0`, with HTTPS in front.
        #[arg(long, default_value = "127.0.0.1")]
        bind: IpAddr,
        /// Port for members: their dashboards and their AI apps.
        #[arg(long, default_value_t = 8787)]
        port: u16,
        /// Port for the admin pages. Never make it reachable from outside.
        #[arg(long, default_value_t = 8788)]
        admin_port: u16,
        /// The address members reach the server at, e.g. `https://uni.example.org`.
        #[arg(long, env = "UNIMCP_PUBLIC_URL")]
        public_url: Option<String>,
        /// The address of the admin pages from outside, if they are reachable there.
        #[arg(long, env = "UNIMCP_ADMIN_URL")]
        admin_url: Option<String>,
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
        bind,
        port,
        admin_port,
        public_url,
        admin_url,
        data,
        autostart,
    } = command
    {
        let listen = server::Listen {
            bind,
            port,
            admin_port,
            public_url: public_url.map(|url| url.trim_end_matches('/').to_owned()),
            admin_url: admin_url.map(|url| url.trim_end_matches('/').to_owned()),
        };
        return run_server(listen, data, autostart).await;
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

/// The server mode serves accounts only: each member chooses a university
/// on the dashboard, and AI apps connect with a member's permission.
async fn run_server(
    listen: server::Listen,
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
            if accounts.waits_for_its_admin(now).map_err(text)? {
                let invite = accounts.invite(now).map_err(text)?;
                println!("Create an admin account:");
                let address = listen.admin_address(listen.admin_port);
                println!("{}", server::admin_link(&address, &invite));
            }
            let launch = autostart::Launch::of_this_server(listen.port, listen.admin_port, &folder)
                .map_err(text)?;
            let note = autostart::turn_on(&launch, true).map_err(text)?;
            let when = match autostart::MOMENT {
                Some("boot") => "whenever this computer starts",
                _ => "whenever you log in to this computer",
            };
            println!("The server is running in the background and starts {when}.");
            println!("Dashboard: http://127.0.0.1:{}/server", listen.port);
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

    drop(accounts);
    server::serve_http(listen, database, &folder).await
}
