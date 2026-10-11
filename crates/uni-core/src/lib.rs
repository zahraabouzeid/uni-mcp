//! Shared foundation of uniMCP: capability traits, data types, university
//! profiles and secret storage. Knows nothing about any one service or MCP.

mod archive;
pub mod capability;
pub mod document;
pub mod error;
pub mod model;
mod office;
pub mod profile;
pub mod secrets;

pub use capability::{Calendar, Canteens, Courses, Files};
pub use error::{Error, Result};
