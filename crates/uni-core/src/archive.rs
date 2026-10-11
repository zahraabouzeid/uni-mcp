use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::document::MAX_FILE_BYTES;
use crate::{Error, Result};

fn unreadable(why: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("could not read this zip archive: {why}"))
}

pub fn list(bytes: &[u8]) -> Result<String> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(unreadable)?;
    let mut lines = Vec::new();
    for index in 0..archive.len() {
        let file = archive.by_index(index).map_err(unreadable)?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().map_err(unreadable)?;
        lines.push(format!("{name}\t{} bytes", file.size()));
    }
    Ok(format!(
        "Zip archive with {} files. Read one of them by passing its name as `entry`.\n\n{}",
        lines.len(),
        lines.join("\n")
    ))
}

pub fn entry(bytes: &[u8], name: &str) -> Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(unreadable)?;
    let file = archive
        .by_name(name)
        .map_err(|_| Error::InvalidInput(format!("the archive has no file called `{name}`")))?;
    // The size in the archive is only a claim, so the limit is enforced while unpacking.
    let mut content = Vec::new();
    file.take(MAX_FILE_BYTES as u64 + 1)
        .read_to_end(&mut content)
        .map_err(unreadable)?;
    crate::document::check_size(content.len() as u64)?;
    Ok(content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::office::testing::archive;

    #[test]
    fn lists_the_files_with_their_sizes() {
        let zip = archive(&[("sheet1/task.txt", "Solve it."), ("notes.md", "# Notes")]);
        let listing = list(&zip).expect("list");
        assert!(listing.starts_with("Zip archive with 2 files."));
        assert!(listing.contains("sheet1/task.txt\t9 bytes"));
        assert!(listing.contains("notes.md\t7 bytes"));
    }

    #[test]
    fn hands_out_one_file() {
        let zip = archive(&[("sheet1/task.txt", "Solve it.")]);
        assert_eq!(entry(&zip, "sheet1/task.txt").expect("entry"), b"Solve it.");
        let error = entry(&zip, "missing.txt").expect_err("missing");
        assert!(error.to_string().contains("missing.txt"));
    }

    #[test]
    fn a_damaged_archive_is_an_error() {
        assert!(list(b"PK\x03\x04 not really a zip").is_err());
        assert!(entry(b"PK\x03\x04 not really a zip", "a").is_err());
    }
}
