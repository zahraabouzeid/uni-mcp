//! Field names here are what the model sees, so keep them plain and stable.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Course {
    pub id: String,
    pub name: String,
    pub short_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Assignment {
    pub id: String,
    pub course_id: String,
    pub name: String,
    /// Unix timestamp in seconds, UTC.
    pub due_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CourseFile {
    pub section: String,
    pub name: String,
    pub size: Option<u64>,
    pub mime_type: Option<String>,
    /// Opaque reference to pass back to `Courses::read_file`.
    pub file_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub title: String,
    /// Local time at the university with its UTC offset, e.g. `2026-10-21T12:30:00+02:00`.
    pub start: String,
    pub end: Option<String>,
    pub weekday: String,
    pub location: Option<String>,
    pub note: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Relative to the root of the file store, `/`-separated.
    pub path: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub modified: Option<String>,
}
