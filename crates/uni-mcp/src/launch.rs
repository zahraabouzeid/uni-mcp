//! How an MCP client should start this program: by its name where the system
//! finds it that way, as after the installer, otherwise by its full path.

use std::path::{Path, PathBuf};

pub fn command() -> Option<String> {
    let program = std::env::current_exe().ok()?;
    Some(command_for(&program, search_path()))
}

fn command_for(program: &Path, folders: impl IntoIterator<Item = PathBuf>) -> String {
    let same_file = |other: PathBuf| match (program.canonicalize(), other.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    let name = program.file_name().zip(program.file_stem());
    match name {
        Some((file, stem))
            if folders
                .into_iter()
                .any(|folder| same_file(folder.join(file))) =>
        {
            stem.to_string_lossy().into_owned()
        }
        _ => program.display().to_string(),
    }
}

/// The file a command name stands for, as a terminal would find it.
pub fn find(name: &str) -> Option<PathBuf> {
    find_in(name, search_path())
}

fn find_in(name: &str, folders: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    let endings: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "bat"]
    } else {
        &[""]
    };
    folders
        .into_iter()
        .flat_map(|folder| {
            endings
                .iter()
                .map(move |ending| folder.join(name).with_extension(ending))
        })
        .find(|file| file.is_file())
}

fn search_path() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default();
    folders.extend(stored_user_path());
    folders
}

/// A program started before the installer ran still has the old search path;
/// the one new programs get is in the registry.
#[cfg(windows)]
fn stored_user_path() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey("Environment")
        .and_then(|environment| environment.get_value::<String, _>("Path"))
        .map(|path| std::env::split_paths(&path).collect())
        .unwrap_or_default()
}

#[cfg(not(windows))]
fn stored_user_path() -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn folder(name: &str) -> PathBuf {
        let folder =
            std::env::temp_dir().join(format!("unimcp-launch-{name}-{}", std::process::id()));
        fs::create_dir_all(&folder).expect("folder");
        folder
    }

    #[test]
    fn a_program_on_the_search_path_is_started_by_its_name() {
        let installed = folder("installed");
        let program = installed.join("unimcp.exe");
        fs::write(&program, "").expect("program");

        let elsewhere = folder("elsewhere");
        assert_eq!(
            command_for(&program, [elsewhere.clone(), installed.clone()]),
            "unimcp"
        );

        fs::remove_dir_all(installed).expect("clean up");
        fs::remove_dir_all(elsewhere).expect("clean up");
    }

    #[test]
    fn a_command_is_found_in_the_first_folder_that_has_it() {
        let empty = folder("empty");
        let tools = folder("tools");
        let file = if cfg!(windows) {
            "claude.cmd"
        } else {
            "claude"
        };
        fs::write(tools.join(file), "").expect("command");

        let found = find_in("claude", [empty.clone(), tools.clone()]);
        assert_eq!(found, Some(tools.join(file)));
        assert_eq!(find_in("codex", [empty.clone(), tools.clone()]), None);

        fs::remove_dir_all(empty).expect("clean up");
        fs::remove_dir_all(tools).expect("clean up");
    }

    #[test]
    fn any_other_program_is_started_by_its_path() {
        let downloads = folder("downloads");
        let program = downloads.join("unimcp.exe");
        fs::write(&program, "").expect("program");

        // Another copy with the same name is on the search path, but not this one.
        let installed = folder("other-copy");
        fs::write(installed.join("unimcp.exe"), "old").expect("other copy");

        assert_eq!(
            command_for(&program, [installed.clone()]),
            program.display().to_string()
        );
        assert_eq!(command_for(&program, []), program.display().to_string());

        fs::remove_dir_all(downloads).expect("clean up");
        fs::remove_dir_all(installed).expect("clean up");
    }
}
