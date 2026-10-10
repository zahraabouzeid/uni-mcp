//! Moodle web service responses, reduced to the fields that are read.

use serde::Deserialize;

#[derive(Deserialize)]
pub struct SiteInfo {
    pub userid: u64,
}

#[derive(Deserialize)]
pub struct Course {
    pub id: u64,
    pub fullname: String,
    pub shortname: Option<String>,
}

#[derive(Deserialize)]
pub struct Section {
    pub name: String,
    #[serde(default)]
    pub modules: Vec<Module>,
}

#[derive(Deserialize)]
pub struct Module {
    #[serde(default)]
    pub contents: Vec<Content>,
}

#[derive(Deserialize)]
pub struct Content {
    #[serde(rename = "type")]
    pub kind: String,
    pub filename: String,
    pub filesize: Option<u64>,
    pub fileurl: Option<String>,
    pub mimetype: Option<String>,
}

#[derive(Deserialize)]
pub struct Assignments {
    #[serde(default)]
    pub courses: Vec<AssignmentCourse>,
}

#[derive(Deserialize)]
pub struct AssignmentCourse {
    pub id: u64,
    #[serde(default)]
    pub assignments: Vec<Assignment>,
}

#[derive(Deserialize)]
pub struct Assignment {
    pub id: u64,
    pub name: String,
    #[serde(default)]
    pub duedate: i64,
}

/// Moodle reports errors with status 200 and this body.
#[derive(Deserialize)]
pub struct Exception {
    pub errorcode: String,
    pub message: Option<String>,
}
