//! Transport layer for cluster communication
//!
//! Provides QUIC-based transport using Quinn with TLS encryption.

mod quic;

pub use quic::{make_client_endpoint, make_server_endpoint, QuicTransport};
