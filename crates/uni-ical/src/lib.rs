//! Calendar connector for a student's personal iCalendar feed, as published
//! by campus systems such as CAMPUSonline. Read-only: it only ever sends GET.

mod client;
mod parse;

pub use client::IcalClient;

pub const SERVICE: &str = "calendar";
