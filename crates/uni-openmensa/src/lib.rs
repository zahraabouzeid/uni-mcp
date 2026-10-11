//! Canteen menu connector for an OpenMensa API. Read-only and without a
//! login: it only ever sends GET.

mod client;

pub use client::OpenMensaClient;

pub const SERVICE: &str = "canteens";
