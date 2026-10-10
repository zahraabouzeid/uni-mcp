//! WebDAV connector for Nextcloud and ownCloud file stores. Read-only: it only
//! ever sends PROPFIND and GET.

mod client;
mod multistatus;

pub use client::WebdavClient;
