//! Prismsearch - Rust client for Prism search engine.

pub mod client;
pub mod error;
pub mod models;
pub mod query;

pub use client::Client;
pub use error::Error;
pub use models::*;
pub use query::Query;
