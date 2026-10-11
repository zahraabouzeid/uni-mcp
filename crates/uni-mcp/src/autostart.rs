//! Lets the server start by itself with the computer, set up for the current
//! user and without administrator rights.

use std::path::{Path, PathBuf};

use serde::Serialize;
use uni_core::{Error, Result};

#[cfg_attr(
    not(any(windows, target_os = "linux", target_os = "macos")),
    allow(dead_code)
)]
const NAME: &str = "unimcp-server";

/// The command that starts this server again.
#[derive(Debug, Clone)]
pub struct Launch {
    program: PathBuf,
    arguments: Vec<String>,
}

impl Launch {
    pub fn of_this_server(port: u16, data: &Path) -> Result<Self> {
        let program = std::env::current_exe()
            .map_err(|e| Error::Config(format!("cannot tell where uniMCP is installed: {e}")))?;
        Ok(Self::new(program, port, data))
    }

    fn new(program: PathBuf, port: u16, data: &Path) -> Self {
        Self {
            program,
            arguments: vec![
                "server".into(),
                "--port".into(),
                port.to_string(),
                "--data".into(),
                data.display().to_string(),
            ],
        }
    }

    fn parts(&self) -> impl Iterator<Item = String> + '_ {
        std::iter::once(self.program.display().to_string()).chain(self.arguments.iter().cloned())
    }
}

/// When the system starts the server. Each system has one of the two.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Moment {
    Boot,
    Login,
}

pub use system::{MOMENT, is_on, turn_off, turn_on};

#[cfg_attr(not(windows), allow(dead_code))]
fn windows_command(launch: &Launch, headless_host: Option<&Path>) -> String {
    let quoted = |part: String| match part.contains(' ') {
        true => format!("\"{part}\""),
        false => part,
    };
    // Without it a console window would stay open for as long as the server runs.
    let host = headless_host.map(|host| format!("\"{}\" --headless", host.display()));
    host.into_iter()
        .chain(launch.parts().map(quoted))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn systemd_unit(launch: &Launch) -> String {
    let quoted = |part: String| {
        let escaped = part
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%");
        format!("\"{escaped}\"")
    };
    let command = launch.parts().map(quoted).collect::<Vec<_>>().join(" ");
    format!(
        "[Unit]\nDescription=uniMCP server\nAfter=network-online.target\n\n\
         [Service]\nExecStart={command}\nRestart=on-failure\n\n\
         [Install]\nWantedBy=default.target\n"
    )
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn launchd_plist(launch: &Launch) -> String {
    let escaped = |part: String| {
        part.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let arguments: String = launch
        .parts()
        .map(|part| format!("    <string>{}</string>\n", escaped(part)))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         \x20 <key>Label</key>\n  <string>{NAME}</string>\n\
         \x20 <key>ProgramArguments</key>\n  <array>\n{arguments}  </array>\n\
         \x20 <key>RunAtLoad</key>\n  <true/>\n\
         \x20 <key>KeepAlive</key>\n  <dict>\n    <key>SuccessfulExit</key>\n    <false/>\n  </dict>\n\
         </dict>\n</plist>\n"
    )
}

#[cfg(windows)]
mod system {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    use super::*;

    pub const MOMENT: Option<Moment> = Some(Moment::Login);
    const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

    fn failed(e: std::io::Error) -> Error {
        Error::Config(format!("cannot change the programs started at login: {e}"))
    }

    pub fn is_on() -> bool {
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(RUN)
            .and_then(|run| run.get_value::<String, _>(NAME))
            .is_ok()
    }

    pub fn turn_on(launch: &Launch) -> Result<Option<String>> {
        let command = windows_command(launch, crate::scheme::headless_console_host().as_deref());
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(RUN)
            .map_err(failed)?;
        run.set_value(NAME, &command).map_err(failed)?;
        Ok(None)
    }

    pub fn turn_off() -> Result<()> {
        let (run, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(RUN)
            .map_err(failed)?;
        match run.delete_value(NAME) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(failed(e)),
            _ => Ok(()),
        }
    }
}

#[cfg(target_os = "linux")]
mod system {
    use std::fs;
    use std::process::Command;

    use super::*;

    pub const MOMENT: Option<Moment> = Some(Moment::Boot);

    fn unit_name() -> String {
        format!("{NAME}.service")
    }

    fn unit_file() -> Option<PathBuf> {
        Some(
            dirs::config_dir()?
                .join("systemd")
                .join("user")
                .join(unit_name()),
        )
    }

    fn systemctl(arguments: &[&str]) -> bool {
        Command::new("systemctl")
            .arg("--user")
            .args(arguments)
            .output()
            .is_ok_and(|output| output.status.success())
    }

    pub fn is_on() -> bool {
        unit_file().is_some_and(|file| file.is_file())
    }

    /// Does not start the service now: the server asking for this is running.
    pub fn turn_on(launch: &Launch) -> Result<Option<String>> {
        let file = unit_file().ok_or_else(|| Error::Config("no home folder".into()))?;
        let cannot_write =
            |e: std::io::Error| Error::Config(format!("cannot write {}: {e}", file.display()));
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder).map_err(cannot_write)?;
        }
        fs::write(&file, systemd_unit(launch)).map_err(cannot_write)?;
        if !(systemctl(&["daemon-reload"]) && systemctl(&["enable", &unit_name()])) {
            let _ = fs::remove_file(&file);
            return Err(Error::Config(
                "this system does not run services for your user (no systemd)".into(),
            ));
        }
        let stays_after_logout = Command::new("loginctl")
            .arg("enable-linger")
            .output()
            .is_ok_and(|output| output.status.success());
        Ok((!stays_after_logout).then(|| {
            "It starts when you log in. To start it at boot, run once: \
             sudo loginctl enable-linger $USER"
                .to_owned()
        }))
    }

    pub fn turn_off() -> Result<()> {
        let Some(file) = unit_file() else {
            return Ok(());
        };
        systemctl(&["disable", &unit_name()]);
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Config(format!(
                "cannot remove {}: {e}",
                file.display()
            ))),
            _ => {
                systemctl(&["daemon-reload"]);
                Ok(())
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod system {
    use std::fs;

    use super::*;

    pub const MOMENT: Option<Moment> = Some(Moment::Login);

    fn agent_file() -> Option<PathBuf> {
        Some(
            dirs::home_dir()?
                .join("Library")
                .join("LaunchAgents")
                .join(format!("{NAME}.plist")),
        )
    }

    pub fn is_on() -> bool {
        agent_file().is_some_and(|file| file.is_file())
    }

    /// The system picks the file up at the next login.
    pub fn turn_on(launch: &Launch) -> Result<Option<String>> {
        let file = agent_file().ok_or_else(|| Error::Config("no home folder".into()))?;
        let cannot_write =
            |e: std::io::Error| Error::Config(format!("cannot write {}: {e}", file.display()));
        if let Some(folder) = file.parent() {
            fs::create_dir_all(folder).map_err(cannot_write)?;
        }
        fs::write(&file, launchd_plist(launch)).map_err(cannot_write)?;
        Ok(None)
    }

    pub fn turn_off() -> Result<()> {
        let Some(file) = agent_file() else {
            return Ok(());
        };
        match fs::remove_file(&file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::Config(format!(
                "cannot remove {}: {e}",
                file.display()
            ))),
            _ => Ok(()),
        }
    }
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
mod system {
    use super::*;

    pub const MOMENT: Option<Moment> = None;

    pub fn is_on() -> bool {
        false
    }

    pub fn turn_on(_launch: &Launch) -> Result<Option<String>> {
        Err(Error::Config(
            "starting with the computer is not supported on this system".into(),
        ))
    }

    pub fn turn_off() -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub mod testing {
    use super::*;

    pub fn launch() -> Launch {
        Launch::new(
            PathBuf::from("/opt/uni mcp/unimcp"),
            8787,
            Path::new("/srv/data"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::testing::launch;
    use super::*;

    #[test]
    fn windows_starts_it_without_a_window_and_quotes_paths_with_spaces() {
        let host = Path::new(r"C:\Windows\System32\conhost.exe");
        assert_eq!(
            windows_command(&launch(), Some(host)),
            "\"C:\\Windows\\System32\\conhost.exe\" --headless \"/opt/uni mcp/unimcp\" server --port 8787 --data /srv/data"
        );
        assert!(windows_command(&launch(), None).starts_with("\"/opt/uni mcp/unimcp\" server"));
    }

    #[test]
    fn the_systemd_unit_restarts_after_a_crash_and_starts_with_the_user_session() {
        let unit = systemd_unit(&launch());
        assert!(unit.contains(
            "ExecStart=\"/opt/uni mcp/unimcp\" \"server\" \"--port\" \"8787\" \"--data\" \"/srv/data\"\n"
        ));
        assert!(unit.contains("Restart=on-failure\n"));
        assert!(unit.ends_with("[Install]\nWantedBy=default.target\n"));
    }

    #[test]
    fn a_percent_sign_in_a_path_is_not_read_as_a_systemd_placeholder() {
        let odd = Launch::new(PathBuf::from("/opt/100%/unimcp"), 1, Path::new("/d"));
        assert!(systemd_unit(&odd).contains("\"/opt/100%%/unimcp\""));
    }

    #[test]
    fn the_launchd_agent_lists_every_argument_and_comes_back_after_a_crash() {
        let odd = Launch::new(PathBuf::from("/Apps/a&b/unimcp"), 8787, Path::new("/d"));
        let plist = launchd_plist(&odd);
        assert!(plist.contains("<string>/Apps/a&amp;b/unimcp</string>"));
        assert!(plist.contains("<string>--port</string>\n    <string>8787</string>"));
        assert!(plist.contains("<key>RunAtLoad</key>\n  <true/>"));
        assert!(plist.contains("<key>SuccessfulExit</key>\n    <false/>"));
        assert!(plist.contains("<string>unimcp-server</string>"));
    }
}
