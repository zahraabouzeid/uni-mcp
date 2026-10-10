//! Shared foundation of uniMCP: capability traits, data types, university
//! profiles and secret storage. Knows nothing about any one service or MCP.

pub mod capability;
pub mod error;
pub mod model;
pub mod profile;
pub mod secrets;

pub use capability::{Calendar, Courses, Files};
pub use error::{Error, Result};
