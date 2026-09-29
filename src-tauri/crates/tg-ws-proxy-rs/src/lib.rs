pub mod check;
pub mod config;
pub mod crypto;
pub mod default_domains;
pub mod faketls;
pub mod limits;
pub mod outbound;
pub mod pool;
pub mod proxy;
pub mod runtime;
pub mod server;
pub mod splitter;
pub mod stats;
pub mod ws_client;

/// Version of the embedded engine, kept in one place for library consumers.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
