//! PKI generation and certificate loading.
//!
//! On first run, a self-signed CA (ECDSA P-256, 5-year validity) and a server
//! certificate (14-day validity) are generated via rcgen. Both are stored as
//! artifacts. The server certificate is regenerated periodically by the
//! rotation task.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use aperture_artifacts::Artifacts;
use jiff::Timestamp;
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::CertificateDer;
use tokio::fs;
use tokio::io::AsyncReadExt;

use super::SharedConfig;
use crate::tls::error::TlsError;

/// CA certificate validity in days (5 years).
pub const CA_VALIDITY_DAYS: u64 = 365 * 5;

/// Leaf certificate validity in days (short-lived, ACME-style).
pub const LEAF_VALIDITY_DAYS: u64 = 14;

/// Artifact keys for TLS certificates.
pub const CA_CERT_KEY: &str = "tls/ca-cert";
pub const CA_KEY_KEY: &str = "tls/ca-key";
pub const SERVER_CERT_KEY: &str = "tls/server-cert";
pub const SERVER_KEY_KEY: &str = "tls/server-key";

/// Generated PKI material, PEM-encoded.
pub struct Pki {
    pub ca_cert_pem: String,
    pub ca_key_pem: String,
    pub server_cert_pem: String,
    pub server_key_pem: String,
}

/// Generates a new CA and server certificate signed by that CA.
pub fn generate_pki(bind_addr: SocketAddr) -> Result<Pki, TlsError> {
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Aperture Gateway CA");
    set_validity(&mut ca_params, CA_VALIDITY_DAYS);

    let ca_key = KeyPair::generate()?;
    let ca_key_pem = ca_key.serialize_pem();
    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_cert_pem = ca_cert.pem();
    let issuer = Issuer::new(ca_params, ca_key);

    let sans = compute_sans(bind_addr);
    let (server_cert_pem, server_key_pem) = generate_leaf(&issuer, &sans)?;
    Ok(Pki {
        ca_cert_pem,
        ca_key_pem,
        server_cert_pem,
        server_key_pem,
    })
}

/// Generates a new leaf certificate signed by the CA stored in artifacts.
pub async fn regenerate_leaf(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(String, String), TlsError> {
    let ca_cert_pem = read_artifact(artifacts, CA_CERT_KEY).await?;
    let ca_key_pem = read_artifact(artifacts, CA_KEY_KEY).await?;

    let ca_key = KeyPair::from_pem(&ca_key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(&ca_cert_pem, ca_key)?;

    let sans = compute_sans(bind_addr);
    generate_leaf(&issuer, &sans)
}

fn generate_leaf(
    issuer: &Issuer<'_, KeyPair>,
    sans: &[SanType],
) -> Result<(String, String), TlsError> {
    let mut params = CertificateParams::default();
    params.subject_alt_names = sans.to_vec();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "Aperture Gateway");
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    set_validity(&mut params, LEAF_VALIDITY_DAYS);

    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, issuer)?;
    Ok((cert.pem(), key.serialize_pem()))
}

fn set_validity(params: &mut CertificateParams, days: u64) {
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(days as i64);
}

fn compute_sans(bind_addr: SocketAddr) -> Vec<SanType> {
    let mut sans = vec![
        SanType::DnsName("localhost".try_into().expect("valid DNS name")),
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ];
    let bind_ip = bind_addr.ip();
    let already = sans
        .iter()
        .any(|s| matches!(s, SanType::IpAddress(a) if *a == bind_ip));
    if !already && !bind_ip.is_unspecified() {
        sans.push(SanType::IpAddress(bind_ip));
    }
    sans
}

/// Ensures TLS certificate artifacts exist, generating them on first run.
pub async fn ensure_certificates(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(), TlsError> {
    if artifacts.locate(SERVER_CERT_KEY).await?.is_some() {
        return Ok(());
    }
    tracing::info!("generating initial PKI");
    let pki = generate_pki(bind_addr)?;
    store_artifact(artifacts, CA_CERT_KEY, &pki.ca_cert_pem).await?;
    store_artifact(artifacts, CA_KEY_KEY, &pki.ca_key_pem).await?;
    store_artifact(artifacts, SERVER_CERT_KEY, &pki.server_cert_pem).await?;
    store_artifact(artifacts, SERVER_KEY_KEY, &pki.server_key_pem).await?;
    Ok(())
}

/// Loads the server certificate from artifacts and builds a `ServerConfig`.
pub async fn load_server_config(artifacts: &Artifacts) -> Result<ServerConfig, TlsError> {
    let cert_pem = read_artifact(artifacts, SERVER_CERT_KEY).await?;
    let key_pem = read_artifact(artifacts, SERVER_KEY_KEY).await?;
    build_server_config(&cert_pem, &key_pem)
}

/// Reloads certificates from artifacts and swaps the shared config.
pub async fn reload_certificates(
    artifacts: &Artifacts,
    config: &SharedConfig,
) -> Result<(), TlsError> {
    let new_config = load_server_config(artifacts).await?;
    config.store(Arc::new(new_config));
    tracing::info!("TLS certificates reloaded");
    Ok(())
}

/// Returns true when the server certificate is past half its lifetime.
pub async fn needs_rotation(artifacts: &Artifacts) -> Result<bool, TlsError> {
    let Some(key) = artifacts.artifact(SERVER_CERT_KEY).await? else {
        return Ok(false);
    };
    let now = Timestamp::now();
    let age_ms = (now.as_millisecond() - key.latest.downloaded_at.as_millisecond()).max(0);
    let half_life_ms = (LEAF_VALIDITY_DAYS as i64) * 24 * 60 * 60 * 1000 / 2;
    Ok(age_ms >= half_life_ms)
}

/// Generates a new leaf certificate and stores it as artifacts.
pub async fn rotate_certificate(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(), TlsError> {
    let (cert_pem, key_pem) = regenerate_leaf(artifacts, bind_addr).await?;
    store_artifact(artifacts, SERVER_CERT_KEY, &cert_pem).await?;
    store_artifact(artifacts, SERVER_KEY_KEY, &key_pem).await?;
    Ok(())
}

/// Builds a `rustls::ServerConfig` from PEM-encoded cert and key.
pub fn build_server_config(cert_pem: &str, key_pem: &str) -> Result<ServerConfig, TlsError> {
    let cert_chain: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut cert_pem.as_bytes()).collect::<Result<Vec<_>, _>>()?;

    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())?
        .ok_or_else(|| TlsError::PemParse("no private key found in PEM".into()))?;

    Ok(ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?)
}

async fn store_artifact(artifacts: &Artifacts, key: &str, pem: &str) -> Result<(), TlsError> {
    artifacts
        .put(key, Some("application/x-pem-file"), pem.as_bytes())
        .await?;
    Ok(())
}

async fn read_artifact(artifacts: &Artifacts, key: &str) -> Result<String, TlsError> {
    let located = artifacts
        .locate(key)
        .await?
        .ok_or(TlsError::NoCertificate)?;
    let mut file = fs::File::open(&located.path).await?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await?;
    String::from_utf8(buf).map_err(|e| TlsError::PemParse(e.to_string()))
}
