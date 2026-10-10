//! Registers `unimcp://` links, so the browser can hand the Moodle sign-in
//! redirect to `unimcp login-callback`.

use std::io;

pub const SCHEME: &str = "unimcp";

/// Whether [`register`] works on this system.
pub const SUPPORTED: bool = cfg!(windows);

/// Points the scheme at this executable, for the current user only.
#[cfg(windows)]
pub fn register(profile_id: &str) -> io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let exe = std::env::current_exe()?;
    let mut handler = format!(
        "\"{}\" --profile \"{profile_id}\" login-callback \"%1\"",
        exe.display()
    );
    // A console program started by the browser would pop up a window.
    if let Some(conhost) = headless_console_host() {
        handler = format!("\"{}\" --headless {handler}", conhost.display());
    }

    let classes = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = classes.create_subkey(format!(r"Software\Classes\{SCHEME}"))?;
    key.set_value("", &format!("URL:{SCHEME}"))?;
    key.set_value("URL Protocol", &"")?;
    let (command, _) = key.create_subkey(r"shell\open\command")?;
    command.set_value("", &handler)
}

#[cfg(windows)]
fn headless_console_host() -> Option<std::path::PathBuf> {
    let conhost = std::path::Path::new(&std::env::var_os("SystemRoot")?)
        .join("System32")
        .join("conhost.exe");
    conhost.exists().then_some(conhost)
}

#[cfg(not(windows))]
pub fn register(_profile_id: &str) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}
