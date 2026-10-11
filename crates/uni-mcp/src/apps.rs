//! Adds this program to the MCP clients on this computer, when the student
//! asks for it on the dashboard.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use toml_edit::{Array, DocumentMut, Item, Table, value};
use uni_core::{Error, Result};

use crate::launch;

/// Also the name in `web/components/setup-view.tsx`.
const SERVER: &str = "uniMCP";
const ARGUMENT: &str = "serve";
const CLAUDE_CODE: &str = "claude";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum App {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    GeminiCli,
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct AppState {
    pub id: App,
    pub installed: bool,
    pub added: bool,
    /// Claude Desktop writes its settings back when it quits, so it has to be
    /// closed before they are changed.
    pub running: bool,
}

/// Where the clients keep their settings.
struct Places {
    claude_desktop: PathBuf,
    claude_code: PathBuf,
    codex: PathBuf,
    gemini_cli: PathBuf,
}

impl Places {
    fn of_this_user() -> Option<Self> {
        let home = dirs::home_dir()?;
        Some(Self {
            claude_desktop: dirs::config_dir()?
                .join("Claude")
                .join("claude_desktop_config.json"),
            claude_code: home.join(".claude.json"),
            codex: home.join(".codex").join("config.toml"),
            gemini_cli: home.join(".gemini").join("settings.json"),
        })
    }

    fn state(&self, id: App, command: &str) -> AppState {
        let folder_exists = |file: &Path| file.parent().is_some_and(Path::is_dir);
        let read = |file: &Path| fs::read_to_string(file).unwrap_or_default();
        let (installed, added) = match id {
            App::ClaudeDesktop => (
                folder_exists(&self.claude_desktop),
                json_has(&read(&self.claude_desktop), command),
            ),
            App::ClaudeCode => (
                launch::find(CLAUDE_CODE).is_some(),
                json_has(&read(&self.claude_code), command),
            ),
            App::Codex => (
                folder_exists(&self.codex),
                toml_has(&read(&self.codex), command),
            ),
            App::GeminiCli => (
                folder_exists(&self.gemini_cli),
                json_has(&read(&self.gemini_cli), command),
            ),
        };
        AppState {
            id,
            installed,
            added,
            running: id == App::ClaudeDesktop && installed && claude_desktop_is_running(),
        }
    }
}

pub fn states(command: &str) -> Vec<AppState> {
    let Some(places) = Places::of_this_user() else {
        return Vec::new();
    };
    [
        App::ClaudeDesktop,
        App::ClaudeCode,
        App::Codex,
        App::GeminiCli,
    ]
    .into_iter()
    .map(|id| places.state(id, command))
    .collect()
}

pub fn add(app: App, command: &str) -> Result<()> {
    let places = Places::of_this_user()
        .ok_or_else(|| Error::Config("this system has no home folder".into()))?;
    match app {
        App::ClaudeDesktop => {
            if claude_desktop_is_running() {
                return Err(Error::InvalidInput(
                    "quit Claude Desktop first, also from the system tray, then add again".into(),
                ));
            }
            rewrite(&places.claude_desktop, |settings| {
                json_with(settings, command)
            })
        }
        App::Codex => rewrite(&places.codex, |settings| toml_with(settings, command)),
        App::GeminiCli => rewrite(&places.gemini_cli, |settings| json_with(settings, command)),
        App::ClaudeCode => {
            let program = launch::find(CLAUDE_CODE)
                .ok_or_else(|| Error::Config("the `claude` command was not found".into()))?;
            // Fails when there is nothing to remove, which is fine.
            let _ = run(&program, &["mcp", "remove", "--scope", "user", SERVER]);
            run(
                &program,
                &[
                    "mcp", "add", "--scope", "user", SERVER, "--", command, ARGUMENT,
                ],
            )
        }
    }
}

fn rewrite(file: &Path, change: impl FnOnce(&str) -> Result<String>) -> Result<()> {
    let cannot = |what: &str, e: std::io::Error| {
        Error::Config(format!("cannot {what} {}: {e}", file.display()))
    };
    let settings = match fs::read_to_string(file) {
        Ok(settings) => settings,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(cannot("read", e)),
    };
    fs::write(file, change(&settings)?).map_err(|e| cannot("write", e))
}

fn run(program: &Path, arguments: &[&str]) -> Result<()> {
    let mut command = Command::new(program);
    command.args(arguments).stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let failed = |why: String| Error::Config(format!("`{CLAUDE_CODE}` failed: {why}"));
    let output = command.output().map_err(|e| failed(e.to_string()))?;
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(failed(
        message.lines().next().unwrap_or("no message").to_owned(),
    ))
}

fn entry(command: &str) -> Value {
    json!({ "command": command, "args": [ARGUMENT] })
}

fn json_has(settings: &str, command: &str) -> bool {
    serde_json::from_str::<Value>(settings)
        .is_ok_and(|settings| settings["mcpServers"][SERVER] == entry(command))
}

fn json_with(settings: &str, command: &str) -> Result<String> {
    let unreadable = || Error::Config("the settings file of this app is not valid JSON".into());
    let mut settings: Value = match settings.trim() {
        "" => json!({}),
        settings => serde_json::from_str(settings).map_err(|_| unreadable())?,
    };
    let servers = settings
        .as_object_mut()
        .ok_or_else(unreadable)?
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    servers
        .as_object_mut()
        .ok_or_else(unreadable)?
        .insert(SERVER.to_owned(), entry(command));
    let mut text = serde_json::to_string_pretty(&settings).map_err(|_| unreadable())?;
    text.push('\n');
    Ok(text)
}

fn toml_has(settings: &str, command: &str) -> bool {
    let Ok(settings) = settings.parse::<DocumentMut>() else {
        return false;
    };
    let server = settings
        .get("mcp_servers")
        .and_then(|servers| servers.get(SERVER));
    let arguments = server
        .and_then(|server| server.get("args"))
        .and_then(Item::as_array)
        .map(|arguments| arguments.iter().filter_map(|a| a.as_str()).collect());
    server
        .and_then(|server| server.get("command"))
        .and_then(Item::as_str)
        == Some(command)
        && arguments == Some(vec![ARGUMENT])
}

fn toml_with(settings: &str, command: &str) -> Result<String> {
    let unreadable = || Error::Config("the settings file of this app is not valid TOML".into());
    let mut settings: DocumentMut = settings.parse().map_err(|_| unreadable())?;
    let servers = settings.entry("mcp_servers").or_insert_with(|| {
        let mut servers = Table::new();
        servers.set_implicit(true);
        Item::Table(servers)
    });
    let mut server = Table::new();
    server["command"] = value(command);
    server["args"] = value(Array::from_iter([ARGUMENT]));
    servers
        .as_table_like_mut()
        .ok_or_else(unreadable)?
        .insert(SERVER, Item::Table(server));
    Ok(settings.to_string())
}

fn claude_desktop_is_running() -> bool {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always),
    );
    system
        .processes()
        .values()
        .filter_map(|process| process.exe())
        .any(is_claude_desktop)
}

/// Claude Code's program has the same name. Only the desktop app is an
/// Electron app, with its `app.asar` in a resources folder next to it.
fn is_claude_desktop(program: &Path) -> bool {
    let named_claude = program
        .file_stem()
        .is_some_and(|name| name.eq_ignore_ascii_case("claude"));
    let has_resources = |folder: Option<&Path>, resources: &str| {
        folder.is_some_and(|folder| folder.join(resources).join("app.asar").is_file())
    };
    let folder = program.parent();
    named_claude
        && (has_resources(folder, "resources")
            || has_resources(folder.and_then(Path::parent), "Resources"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("unimcp-apps-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("folder");
        folder
    }

    #[test]
    fn json_settings_gain_the_server_and_keep_everything_else() {
        let before = r#"{
            "preferences": { "theme": "dark" },
            "mcpServers": { "other": { "command": "other", "args": [] } }
        }"#;
        assert!(!json_has(before, "unimcp"));

        let after = json_with(before, "unimcp").expect("settings");
        assert!(json_has(&after, "unimcp"));
        let settings: Value = serde_json::from_str(&after).expect("json");
        assert_eq!(settings["preferences"]["theme"], "dark");
        assert_eq!(settings["mcpServers"]["other"]["command"], "other");
    }

    #[test]
    fn a_different_command_does_not_count_as_added_and_is_replaced() {
        let old = json_with("", r"C:\Downloads\unimcp.exe").expect("settings");
        assert!(!json_has(&old, "unimcp"));
        assert!(json_has(
            &json_with(&old, "unimcp").expect("settings"),
            "unimcp"
        ));
    }

    #[test]
    fn broken_json_settings_are_left_alone() {
        assert!(json_with("{ not json", "unimcp").is_err());
        assert!(json_with("[]", "unimcp").is_err());
        assert!(json_with(r#"{ "mcpServers": 3 }"#, "unimcp").is_err());
    }

    #[test]
    fn toml_settings_gain_the_server_and_keep_comments_and_other_servers() {
        let before = "# my settings\nmodel = \"x\"\n\n[mcp_servers.other]\ncommand = 'other'\n";
        assert!(!toml_has(before, "unimcp"));

        let after = toml_with(before, "unimcp").expect("settings");
        assert!(toml_has(&after, "unimcp"));
        assert!(after.starts_with("# my settings\nmodel = \"x\"\n"));
        assert!(after.contains("[mcp_servers.other]\ncommand = 'other'\n"));
        assert!(after.contains("[mcp_servers.uniMCP]\n"));
    }

    #[test]
    fn empty_toml_settings_work_and_broken_ones_are_left_alone() {
        let after = toml_with("", "unimcp").expect("settings");
        assert_eq!(
            after,
            "[mcp_servers.uniMCP]\ncommand = \"unimcp\"\nargs = [\"serve\"]\n"
        );
        assert!(toml_with("[broken", "unimcp").is_err());
    }

    #[test]
    fn an_app_whose_settings_folder_is_missing_is_not_installed() {
        let home = folder("home");
        let places = Places {
            claude_desktop: home.join("Claude").join("claude_desktop_config.json"),
            claude_code: home.join(".claude.json"),
            codex: home.join(".codex").join("config.toml"),
            gemini_cli: home.join(".gemini").join("settings.json"),
        };
        let codex = places.state(App::Codex, "unimcp");
        assert!(!codex.installed && !codex.added && !codex.running);

        fs::create_dir_all(home.join(".codex")).expect("folder");
        fs::write(&places.codex, toml_with("", "unimcp").expect("settings")).expect("settings");
        let codex = places.state(App::Codex, "unimcp");
        assert!(codex.installed && codex.added);

        fs::create_dir_all(home.join(".gemini")).expect("folder");
        let gemini = places.state(App::GeminiCli, "unimcp");
        assert!(gemini.installed && !gemini.added);
        fs::write(
            &places.gemini_cli,
            json_with("", "unimcp").expect("settings"),
        )
        .expect("settings");
        assert!(places.state(App::GeminiCli, "unimcp").added);

        fs::remove_dir_all(home).expect("clean up");
    }

    #[test]
    fn only_the_desktop_app_counts_as_claude_desktop() {
        let desktop = folder("desktop");
        fs::create_dir_all(desktop.join("resources")).expect("folder");
        fs::write(desktop.join("resources").join("app.asar"), "").expect("archive");
        assert!(is_claude_desktop(&desktop.join("Claude.exe")));
        assert!(!is_claude_desktop(&desktop.join("Update.exe")));

        let code = folder("code");
        assert!(!is_claude_desktop(&code.join("claude.exe")));

        fs::remove_dir_all(desktop).expect("clean up");
        fs::remove_dir_all(code).expect("clean up");
    }
}
