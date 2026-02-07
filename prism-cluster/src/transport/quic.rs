//! QUIC transport implementation using Quinn
//!
//! Provides secure, multiplexed connections between cluster nodes.

use crate::config::{ClusterConfig, ClusterTlsConfig};
use crate::error::{ClusterError, Result};
use quinn::{ClientConfig, Endpoint, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{debug, info};

/// QUIC transport wrapper for cluster communication
pub struct QuicTransport {
    endpoint: Endpoint,
    config: ClusterConfig,
}

impl QuicTransport {
    /// Create a new server transport
    pub async fn new_server(config: ClusterConfig) -> Result<Self> {
        let endpoint = make_server_endpoint(&config).await?;
        Ok(Self { endpoint, config })
    }

    /// Create a new client transport
    pub async fn new_client(config: ClusterConfig) -> Result<Self> {
        let endpoint = make_client_endpoint(&config).await?;
        Ok(Self { endpoint, config })
    }

    /// Get the local address this transport is bound to
    pub fn local_addr(&self) -> Result<SocketAddr> {
        self.endpoint.local_addr().map_err(ClusterError::from)
    }

    /// Get reference to the underlying endpoint
    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// Get the cluster config
    pub fn config(&self) -> &ClusterConfig {
        &self.config
    }
}

/// Create a QUIC server endpoint
pub async fn make_server_endpoint(config: &ClusterConfig) -> Result<Endpoint> {
    let bind_addr = config.parse_bind_addr().map_err(|e| {
        ClusterError::Config(format!("Invalid bind address '{}': {}", config.bind_addr, e))
    })?;

    let server_config = build_server_config(&config.tls)?;

    let endpoint = Endpoint::server(server_config, bind_addr).map_err(|e| {
        ClusterError::Transport(format!("Failed to create server endpoint: {}", e))
    })?;

    info!("Cluster server listening on {}", bind_addr);
    Ok(endpoint)
}

/// Create a QUIC client endpoint
pub async fn make_client_endpoint(config: &ClusterConfig) -> Result<Endpoint> {
    // Bind to any available port for client
    let bind_addr: SocketAddr = "0.0.0.0:0".parse().unwrap();

    let client_config = build_client_config(&config.tls)?;

    let mut endpoint = Endpoint::client(bind_addr)
        .map_err(|e| ClusterError::Transport(format!("Failed to create client endpoint: {}", e)))?;

    endpoint.set_default_client_config(client_config);

    debug!("Created cluster client endpoint");
    Ok(endpoint)
}

/// Build rustls ServerConfig for QUIC
fn build_server_config(tls_config: &ClusterTlsConfig) -> Result<ServerConfig> {
    let (certs, key) = load_certs_and_key(tls_config)?;

    let crypto = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| ClusterError::Tls(format!("Failed to build server TLS config: {}", e)))?;

    let server_config = ServerConfig::with_crypto(Arc::new(
        quinn::crypto::rustls::QuicServerConfig::try_from(crypto)
            .map_err(|e| ClusterError::Tls(format!("Failed to create QUIC server config: {}", e)))?,
    ));

    Ok(server_config)
}

/// Build rustls ClientConfig for QUIC
fn build_client_config(tls_config: &ClusterTlsConfig) -> Result<ClientConfig> {
    let crypto = if tls_config.skip_verify {
        // WARNING: This is insecure and should only be used for development
        tracing::warn!("Cluster TLS verification disabled - INSECURE");

        rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth()
    } else {
        // Build proper certificate verification
        let mut roots = rustls::RootCertStore::empty();

        if let Some(ref ca_path) = tls_config.ca_cert_path {
            let ca_file = File::open(ca_path).map_err(|e| {
                ClusterError::Tls(format!("Failed to open CA cert file {:?}: {}", ca_path, e))
            })?;
            let mut ca_reader = BufReader::new(ca_file);
            let ca_certs = rustls_pemfile::certs(&mut ca_reader)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| ClusterError::Tls(format!("Failed to parse CA certs: {}", e)))?;

            for cert in ca_certs {
                roots
                    .add(cert)
                    .map_err(|e| ClusterError::Tls(format!("Failed to add CA cert: {}", e)))?;
            }
        } else {
            // Use system root certificates
            let native_certs = rustls_native_certs::load_native_certs();
            for cert in native_certs.certs {
                let _ = roots.add(cert);
            }
        }

        rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth()
    };

    let client_config = ClientConfig::new(Arc::new(
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
            .map_err(|e| ClusterError::Tls(format!("Failed to create QUIC client config: {}", e)))?,
    ));

    Ok(client_config)
}

/// Load certificates and private key from PEM files
fn load_certs_and_key(
    tls_config: &ClusterTlsConfig,
) -> Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    // Load certificates
    let cert_file = File::open(&tls_config.cert_path).map_err(|e| {
        ClusterError::Tls(format!(
            "Failed to open cert file {:?}: {}",
            tls_config.cert_path, e
        ))
    })?;
    let mut cert_reader = BufReader::new(cert_file);
    let certs = rustls_pemfile::certs(&mut cert_reader)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| ClusterError::Tls(format!("Failed to parse certificates: {}", e)))?;

    if certs.is_empty() {
        return Err(ClusterError::Tls(format!(
            "No certificates found in {:?}",
            tls_config.cert_path
        )));
    }

    // Load private key
    let key_file = File::open(&tls_config.key_path).map_err(|e| {
        ClusterError::Tls(format!(
            "Failed to open key file {:?}: {}",
            tls_config.key_path, e
        ))
    })?;
    let mut key_reader = BufReader::new(key_file);

    let key = rustls_pemfile::private_key(&mut key_reader)
        .map_err(|e| ClusterError::Tls(format!("Failed to parse private key: {}", e)))?
        .ok_or_else(|| {
            ClusterError::Tls(format!("No private key found in {:?}", tls_config.key_path))
        })?;

    Ok((certs, key))
}

/// Certificate verifier that skips all verification (INSECURE)
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> std::result::Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::RSA_PKCS1_SHA256,
            rustls::SignatureScheme::RSA_PKCS1_SHA384,
            rustls::SignatureScheme::RSA_PKCS1_SHA512,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            rustls::SignatureScheme::RSA_PSS_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA384,
            rustls::SignatureScheme::RSA_PSS_SHA512,
            rustls::SignatureScheme::ED25519,
        ]
    }
}
