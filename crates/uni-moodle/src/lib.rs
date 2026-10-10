//! Moodle connector on the official web service API, with the token the
//! Moodle mobile app gets. Read-only.

mod client;
pub mod login;
mod wire;

pub use client::MoodleClient;

pub const SERVICE: &str = "moodle";
