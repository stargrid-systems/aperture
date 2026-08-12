//! PKI generation and certificate loading.
//!
//! Artifacts (all DER): `tls_ca-cert` (pkix-cert), `tls_ca-key` (pkcs8,
//! secret), `tls_server-cert` (pkix-cert), `tls_server-key` (pkcs8, secret).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::{Arc, LazyLock};

use anyhow::Context as _;
use aperture_artifacts::Artifacts;
use aperture_storage::{ArtifactKey, MediaType};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    Issuer, KeyPair, KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::task::spawn_blocking;
use x509_parser::extensions::GeneralName;
use x509_parser::x509::X509Name;

use super::error::TlsError;
use super::{CA_CERT, CA_KEY, SERVER_CERT, SERVER_KEY, SharedConfig};

static PKIX_CERT: LazyLock<MediaType> =
    LazyLock::new(|| "application/pkix-cert".parse().expect("valid media type"));
static PKCS8: LazyLock<MediaType> =
    LazyLock::new(|| "application/pkcs8".parse().expect("valid media type"));
const CA_VALIDITY_DAYS: u32 = 365 * 5;

const LEAF_VALIDITY_DAYS: u32 = 14;
const LEAF_COMMON_NAME: &str = "Aperture Gateway";
const CA_COMMON_NAME: &str = "Aperture Gateway CA";

struct Pki {
    ca_cert: CertificateDer<'static>,
    ca_key: PrivatePkcs8KeyDer<'static>,
    server_cert: CertificateDer<'static>,
    server_key: PrivatePkcs8KeyDer<'static>,
}

/// Generates a CA and leaf cert signed by it. Blocking.
fn generate_pki(bind_addr: SocketAddr, hostname: Option<&str>) -> anyhow::Result<Pki> {
    let mut ca_params = CertificateParams::default();
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca_params
        .distinguished_name
        .push(DnType::CommonName, CA_COMMON_NAME);
    set_validity(&mut ca_params, CA_VALIDITY_DAYS);

    let ca_key = KeyPair::generate()?;
    let ca_key_der = PrivatePkcs8KeyDer::from(ca_key.serialize_der());
    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_cert_der = ca_cert.der().clone();
    let issuer = Issuer::new(ca_params, ca_key);

    let sans = compute_sans(bind_addr, hostname);
    let subject = default_leaf_subject();
    let (server_cert, server_key) = generate_leaf(&issuer, &subject, &sans)?;
    Ok(Pki {
        ca_cert: ca_cert_der,
        ca_key: ca_key_der,
        server_cert,
        server_key,
    })
}

/// Re-issues the leaf cert preserving its subject and SANs.
async fn regenerate_leaf_for_rotation(
    artifacts: &Artifacts,
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
    let ca_cert_der = read_artifact(artifacts, &CA_CERT).await?;
    let ca_key_der = read_artifact(artifacts, &CA_KEY).await?;
    let leaf_der = read_artifact(artifacts, &SERVER_CERT).await?;

    Ok(spawn_blocking(move || {
        let ca_key = KeyPair::try_from(ca_key_der.as_slice())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key)?;
        let (subject, sans) = extract_leaf_identity(&leaf_der)?;
        generate_leaf(&issuer, &subject, &sans)
    })
    .await??)
}

/// Generates a leaf against the existing CA using default identity.
async fn regenerate_leaf_with_default_identity(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
    hostname: Option<&str>,
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
    let ca_cert_der = read_artifact(artifacts, &CA_CERT).await?;
    let ca_key_der = read_artifact(artifacts, &CA_KEY).await?;
    let hostname = hostname.map(str::to_owned);

    Ok(spawn_blocking(move || {
        let ca_key = KeyPair::try_from(ca_key_der.as_slice())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key)?;
        let subject = default_leaf_subject();
        let sans = compute_sans(bind_addr, hostname.as_deref());
        generate_leaf(&issuer, &subject, &sans)
    })
    .await??)
}

fn generate_leaf(
    issuer: &Issuer<'_, KeyPair>,
    subject: &DistinguishedName,
    sans: &[SanType],
) -> anyhow::Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>)> {
    let mut params = CertificateParams::default();
    params.subject_alt_names = sans.to_vec();
    params.distinguished_name = subject.clone();
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    set_validity(&mut params, LEAF_VALIDITY_DAYS);

    let key = KeyPair::generate()?;
    let cert = params.signed_by(&key, issuer)?;
    Ok((
        cert.der().clone(),
        PrivatePkcs8KeyDer::from(key.serialize_der()),
    ))
}

fn default_leaf_subject() -> DistinguishedName {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, LEAF_COMMON_NAME);
    dn
}

fn set_validity(params: &mut CertificateParams, days: u32) {
    let now = time::OffsetDateTime::now_utc();
    params.not_before = now - time::Duration::days(1);
    params.not_after = now + time::Duration::days(days.into());
}

/// Computes SANs for a leaf cert. Localhost is always included. A non-loopback
/// bind IP is appended if set. When `hostname` is set, `<hostname>.local` is
/// added for mDNS reachability.
fn compute_sans(bind_addr: SocketAddr, hostname: Option<&str>) -> Vec<SanType> {
    let mut sans = vec![
        SanType::DnsName("localhost".try_into().expect("valid DNS name")),
        SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        SanType::IpAddress(IpAddr::V6(Ipv6Addr::LOCALHOST)),
    ];

    if let Some(host) = hostname {
        let mdns = format!("{host}.local");
        if let Ok(dns) = mdns.try_into() {
            sans.push(SanType::DnsName(dns));
        }
    }

    let bind_ip = bind_addr.ip();
    let already = sans
        .iter()
        .any(|s| matches!(s, SanType::IpAddress(a) if *a == bind_ip));
    if !already && !bind_ip.is_unspecified() {
        sans.push(SanType::IpAddress(bind_ip));
    }
    sans
}

/// Extracts subject DN and SANs from an existing leaf cert.
fn extract_leaf_identity(der: &[u8]) -> anyhow::Result<(DistinguishedName, Vec<SanType>)> {
    let (_, cert) =
        x509_parser::parse_x509_certificate(der).context("parsing leaf for rotation")?;

    let subject = x509_name_to_rcgen(cert.subject())?;

    let sans = match cert.subject_alternative_name().context("parsing SANs")? {
        Some(ext) => ext
            .value
            .general_names
            .iter()
            .map(general_name_to_rcgen)
            .collect::<Result<Vec<_>, _>>()?,
        None => Vec::new(),
    };

    Ok((subject, sans))
}

fn x509_name_to_rcgen(name: &X509Name<'_>) -> anyhow::Result<DistinguishedName> {
    let mut dn = DistinguishedName::new();
    for attr in name.iter_attributes() {
        let oid_vec: Vec<u64> = attr
            .attr_type()
            .iter()
            .ok_or_else(|| anyhow::anyhow!("non-standard OID encoding in subject attribute"))?
            .collect();
        let dn_type = DnType::from_oid(&oid_vec);
        let value = attr.as_str().context("reading subject attribute value")?;
        dn.push(dn_type, value.to_owned());
    }
    Ok(dn)
}

fn general_name_to_rcgen(name: &GeneralName<'_>) -> anyhow::Result<SanType> {
    let san = match name {
        GeneralName::DNSName(s) => SanType::DnsName((*s).try_into()?),
        GeneralName::RFC822Name(s) => SanType::Rfc822Name((*s).try_into()?),
        GeneralName::URI(s) => SanType::URI((*s).try_into()?),
        GeneralName::IPAddress(octets) => SanType::IpAddress(ip_addr_from_octets(octets)?),
        other => anyhow::bail!(
            "unsupported SAN variant during rotation: {other:?}; only DNS, IP, RFC822, and URI \
             are supported"
        ),
    };
    Ok(san)
}

fn ip_addr_from_octets(octets: &[u8]) -> anyhow::Result<IpAddr> {
    if let Ok(o) = <&[u8; 16]>::try_from(octets) {
        Ok(IpAddr::V6(Ipv6Addr::from(*o)))
    } else if let Ok(o) = <&[u8; 4]>::try_from(octets) {
        Ok(IpAddr::V4(Ipv4Addr::from(*o)))
    } else {
        anyhow::bail!("IP SAN has invalid octet length")
    }
}

/// Ensures TLS artifacts exist, generating them on first run.
///
/// If the CA pair is intact but the leaf is missing, only the leaf is
/// regenerated. Keys are written before certs so a half-write leaves a state
/// rustls rejects rather than silently corrupting handshakes.
pub async fn ensure_certificates(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
    hostname: Option<&str>,
) -> Result<(), TlsError> {
    let ca_present =
        artifacts.locate(&CA_CERT).await?.is_some() && artifacts.locate(&CA_KEY).await?.is_some();
    let leaf_present = artifacts.locate(&SERVER_CERT).await?.is_some()
        && artifacts.locate(&SERVER_KEY).await?.is_some();

    if ca_present && leaf_present {
        return Ok(());
    }

    if !ca_present {
        tracing::info!("generating initial PKI");
        let hostname = hostname.map(str::to_owned);
        let pki = spawn_blocking(move || generate_pki(bind_addr, hostname.as_deref())).await??;
        store_key_artifact(artifacts, &CA_KEY, &pki.ca_key).await?;
        store_cert_artifact(artifacts, &CA_CERT, &pki.ca_cert).await?;
        store_key_artifact(artifacts, &SERVER_KEY, &pki.server_key).await?;
        store_cert_artifact(artifacts, &SERVER_CERT, &pki.server_cert).await?;
        return Ok(());
    }

    tracing::info!("regenerating leaf certificate against existing CA");
    let (cert, key) = regenerate_leaf_with_default_identity(artifacts, bind_addr, hostname).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Regenerates the leaf certificate with a new identity (SANs).
///
/// Used when the advertised hostname changes. The existing CA is reused; only
/// the leaf is re-issued. Live reload is triggered by the artifact change feed
/// (see [`crate::tls::TlsReload`]).
///
/// # Errors
///
/// Returns `TlsError` if the CA artifacts cannot be read, the leaf cannot be
/// generated, or the new artifacts cannot be stored.
pub async fn regenerate_leaf_for_identity(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
    hostname: Option<&str>,
) -> Result<(), TlsError> {
    let (cert, key) = regenerate_leaf_with_default_identity(artifacts, bind_addr, hostname).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Loads the server cert and builds a `ServerConfig`.
pub async fn load_server_config(artifacts: &Artifacts) -> Result<ServerConfig, TlsError> {
    let cert_der = read_artifact(artifacts, &SERVER_CERT).await?;
    let key_der = read_artifact(artifacts, &SERVER_KEY).await?;
    Ok(spawn_blocking(move || build_server_config(&cert_der, &key_der)).await??)
}

/// Reloads certs from artifacts and swaps the shared config.
pub async fn reload_certificates(
    artifacts: &Artifacts,
    config: &SharedConfig,
) -> Result<(), TlsError> {
    let new_config = load_server_config(artifacts).await?;
    config.store(Arc::new(new_config));
    tracing::info!("TLS certificates reloaded");
    Ok(())
}

/// Returns true when the server cert is past half its validity. Computed from
/// the cert's own timestamps, not the default lifetime.
async fn needs_rotation(artifacts: &Artifacts) -> Result<bool, TlsError> {
    let der = read_artifact(artifacts, &SERVER_CERT).await?;
    let (_, cert) =
        x509_parser::parse_x509_certificate(&der).context("parsing cert for rotation check")?;
    let validity = cert.validity();
    let Some(remaining) = validity.time_to_expiration() else {
        return Ok(true);
    };
    let Some(total) = validity.not_after - validity.not_before else {
        return Ok(true);
    };
    Ok(remaining < total / 2)
}

/// Generates and stores a new leaf, preserving identity. Key written before
/// cert so a half-write is detected as a load failure by the reload watcher.
async fn rotate_certificate(artifacts: &Artifacts) -> Result<(), TlsError> {
    let (cert, key) = regenerate_leaf_for_rotation(artifacts).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Rotates the leaf if due. Live reload is triggered separately by the change
/// feed (see [`crate::tls::TlsReload`]).
pub(super) async fn rotate_if_due(artifacts: &Artifacts) -> Result<bool, TlsError> {
    if !needs_rotation(artifacts).await? {
        return Ok(false);
    }
    rotate_certificate(artifacts).await?;
    Ok(true)
}

fn build_server_config(cert_der: &[u8], key_der: &[u8]) -> anyhow::Result<ServerConfig> {
    use rustls::version::{TLS12, TLS13};

    let cert_chain = vec![CertificateDer::from(cert_der.to_vec())];
    let key = PrivateKeyDer::try_from(key_der.to_vec())
        .map_err(|e| anyhow::anyhow!("key parse failed: {e}"))?;

    Ok(
        ServerConfig::builder_with_protocol_versions(&[&TLS13, &TLS12])
            .with_no_client_auth()
            .with_single_cert(cert_chain, key)?,
    )
}

async fn store_cert_artifact(
    artifacts: &Artifacts,
    key: &ArtifactKey,
    der: &CertificateDer<'_>,
) -> Result<(), TlsError> {
    artifacts.put(key, Some(&PKIX_CERT), der.as_ref()).await?;
    Ok(())
}

async fn store_key_artifact(
    artifacts: &Artifacts,
    key: &ArtifactKey,
    der: &PrivatePkcs8KeyDer<'_>,
) -> Result<(), TlsError> {
    artifacts
        .put(key, Some(&PKCS8), der.secret_pkcs8_der())
        .await?;
    Ok(())
}

async fn read_artifact(artifacts: &Artifacts, key: &ArtifactKey) -> Result<Vec<u8>, TlsError> {
    let located = artifacts.locate(key).await?.ok_or(TlsError::NoArtifact)?;
    let mut file = fs::File::open(&located.path).await?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).await?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::PathBuf;
    use std::{env, fs, process};

    use aperture_storage::{Digest, Storage};

    use super::*;
    use crate::tls::{TlsReload, load_shared_config};

    fn install_crypto() {
        use std::sync::Once;

        use rustls::crypto::ring;

        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = ring::default_provider().install_default();
        });
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let dir = env::temp_dir().join(format!(
                "aperture-tls-tests-{}-{}",
                process::id(),
                uuid::Uuid::new_v4()
            ));
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    async fn fresh_store() -> (Artifacts, aperture_events::EventBus, TempDir) {
        let storage = Storage::open(":memory:").await.unwrap();
        let dir = TempDir::new();
        let event_bus = aperture_events::EventBus::new(storage.events().unwrap());
        let artifacts = Artifacts::new(storage, dir.0.clone(), event_bus.clone());
        (artifacts, event_bus, dir)
    }

    #[tokio::test]
    async fn fresh_cert_does_not_need_rotation() {
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();
        assert!(!needs_rotation(&artifacts).await.unwrap());
    }

    #[tokio::test]
    async fn expired_cert_needs_rotation() {
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let ca_cert_der = read_artifact(&artifacts, &CA_CERT).await.unwrap();
        let ca_key_der = read_artifact(&artifacts, &CA_KEY).await.unwrap();
        let ca_key = KeyPair::try_from(ca_key_der.as_slice()).unwrap();
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key).unwrap();

        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "expired");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let now = time::OffsetDateTime::now_utc();
        params.not_before = now - time::Duration::days(10);
        params.not_after = now - time::Duration::days(1);
        let key = KeyPair::generate().unwrap();
        let expired = params.signed_by(&key, &issuer).unwrap();
        store_cert_artifact(&artifacts, &SERVER_CERT, expired.der())
            .await
            .unwrap();

        assert!(needs_rotation(&artifacts).await.unwrap());
    }

    async fn digest_of(artifacts: &Artifacts, key: &ArtifactKey) -> Digest {
        artifacts
            .artifact(key)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
    }

    #[tokio::test]
    async fn ensure_certificates_regenerates_only_leaf_when_ca_pair_intact() {
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let old_ca_cert = digest_of(&artifacts, &CA_CERT).await;
        let old_ca_key = digest_of(&artifacts, &CA_KEY).await;
        let old_server_cert = digest_of(&artifacts, &SERVER_CERT).await;
        let old_server_key = digest_of(&artifacts, &SERVER_KEY).await;

        artifacts
            .evict_version(&SERVER_CERT, &old_server_cert)
            .await
            .unwrap();

        ensure_certificates(&artifacts, addr, None).await.unwrap();

        assert_eq!(digest_of(&artifacts, &CA_CERT).await, old_ca_cert);
        assert_eq!(digest_of(&artifacts, &CA_KEY).await, old_ca_key);
        assert_ne!(
            digest_of(&artifacts, &SERVER_CERT).await,
            old_server_cert,
            "leaf cert should have been re-issued"
        );
        assert_ne!(
            digest_of(&artifacts, &SERVER_KEY).await,
            old_server_key,
            "leaf key should have been re-issued"
        );
    }

    #[tokio::test]
    async fn ensure_certificates_regenerates_everything_when_ca_pair_missing() {
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let old_ca_cert = digest_of(&artifacts, &CA_CERT).await;
        let old_ca_key = digest_of(&artifacts, &CA_KEY).await;
        let old_server_cert = digest_of(&artifacts, &SERVER_CERT).await;
        let old_server_key = digest_of(&artifacts, &SERVER_KEY).await;

        artifacts
            .evict_version(&CA_CERT, &old_ca_cert)
            .await
            .unwrap();

        ensure_certificates(&artifacts, addr, None).await.unwrap();

        assert_ne!(
            digest_of(&artifacts, &CA_CERT).await,
            old_ca_cert,
            "CA cert should have been regenerated"
        );
        assert_ne!(
            digest_of(&artifacts, &CA_KEY).await,
            old_ca_key,
            "CA key should have been regenerated"
        );
        assert_ne!(
            digest_of(&artifacts, &SERVER_CERT).await,
            old_server_cert,
            "leaf cert should have been re-issued against the new CA"
        );
        assert_ne!(
            digest_of(&artifacts, &SERVER_KEY).await,
            old_server_key,
            "leaf key should have been re-issued against the new CA"
        );
    }

    #[tokio::test]
    async fn load_server_config_succeeds_after_ensure() {
        install_crypto();
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();
        let config = load_server_config(&artifacts).await.unwrap();
        let _ = config;
    }

    #[tokio::test]
    async fn load_server_config_fails_on_corrupt_key() {
        install_crypto();
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        artifacts
            .put(&SERVER_KEY, Some(&PKCS8), &b"corrupt"[..])
            .await
            .unwrap();

        assert!(load_server_config(&artifacts).await.is_err());
    }

    #[tokio::test]
    async fn reload_certificates_swaps_shared_config() {
        install_crypto();
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let config = load_shared_config(&artifacts).await.unwrap();
        let old = config.load_full();

        rotate_certificate(&artifacts).await.unwrap();
        reload_certificates(&artifacts, &config).await.unwrap();

        let new = config.load_full();
        assert!(
            !Arc::ptr_eq(&old, &new),
            "shared config was not swapped after reload"
        );
    }

    #[tokio::test]
    async fn rotate_if_due_returns_false_for_fresh_cert() {
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let rotated = rotate_if_due(&artifacts).await.unwrap();
        assert!(!rotated, "fresh cert should not need rotation");
    }

    /// Rotation preserves CN and SANs.
    #[tokio::test]
    async fn rotation_preserves_subject_and_sans() {
        use x509_parser::extensions::GeneralName as X509GeneralName;

        install_crypto();
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let ca_cert_der = read_artifact(&artifacts, &CA_CERT).await.unwrap();
        let ca_key_der = read_artifact(&artifacts, &CA_KEY).await.unwrap();
        let (custom_cert, custom_key) = spawn_blocking(move || {
            let ca_key = KeyPair::try_from(ca_key_der.as_slice()).unwrap();
            let issuer =
                Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key).unwrap();
            let mut params = CertificateParams::default();
            let mut subject = DistinguishedName::new();
            subject.push(DnType::CommonName, "rotation-test.example");
            params.distinguished_name = subject;
            params.subject_alt_names = vec![
                SanType::DnsName("rotation-test.example".try_into().unwrap()),
                SanType::IpAddress(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1))),
            ];
            params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
            params.use_authority_key_identifier_extension = true;
            set_validity(&mut params, LEAF_VALIDITY_DAYS);
            let key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&key, &issuer).unwrap();
            (
                cert.der().clone(),
                PrivatePkcs8KeyDer::from(key.serialize_der()),
            )
        })
        .await
        .unwrap();
        store_key_artifact(&artifacts, &SERVER_KEY, &custom_key)
            .await
            .unwrap();
        store_cert_artifact(&artifacts, &SERVER_CERT, &custom_cert)
            .await
            .unwrap();

        rotate_certificate(&artifacts).await.unwrap();

        let rotated_der = read_artifact(&artifacts, &SERVER_CERT).await.unwrap();
        let (_, rotated) = x509_parser::parse_x509_certificate(&rotated_der).unwrap();
        let cn = rotated
            .subject()
            .iter_common_name()
            .next()
            .and_then(|attr| attr.as_str().ok())
            .unwrap_or("<missing>");
        assert_eq!(cn, "rotation-test.example", "CN must survive rotation");

        let sans = rotated.subject_alternative_name().unwrap().unwrap();
        let dns: Vec<_> = sans
            .value
            .general_names
            .iter()
            .filter_map(|n| match n {
                X509GeneralName::DNSName(s) => Some(*s),
                _ => None,
            })
            .collect();
        assert!(
            dns.contains(&"rotation-test.example"),
            "custom DNS SAN must survive rotation, got {dns:?}"
        );
    }

    #[tokio::test]
    async fn reload_watcher_swaps_config_on_cert_change() {
        use std::time::Duration;

        use tokio::time::{sleep, timeout};
        use tokio_util::sync::CancellationToken;

        install_crypto();

        let (artifacts, bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let config = load_shared_config(&artifacts).await.unwrap();
        let old = config.load_full();

        let reload = TlsReload::new(artifacts.clone(), config.clone(), &bus);
        let token = CancellationToken::new();
        let handle = tokio::spawn(reload.run(token.clone()));

        rotate_certificate(&artifacts).await.unwrap();

        let deadline = Duration::from_secs(5);
        let observed = timeout(deadline, async {
            loop {
                let new = config.load_full();
                if !Arc::ptr_eq(&old, &new) {
                    return;
                }
                sleep(Duration::from_millis(50)).await;
            }
        })
        .await;
        token.cancel();
        handle.await.unwrap();
        assert!(
            observed.is_ok(),
            "watcher did not reload config within {deadline:?} after cert change"
        );
    }

    /// End-to-end TLS handshake with generated PKI.
    #[tokio::test]
    async fn generated_pki_completes_a_real_tls_handshake() {
        use std::time::Duration;

        use axum::serve::Listener;
        use rustls::pki_types::ServerName;
        use rustls::{ClientConfig, RootCertStore};
        use tokio::net::{TcpListener, TcpStream};
        use tokio::time::{sleep, timeout};
        use tokio_rustls::TlsConnector;

        use super::super::TlsListener;

        install_crypto();

        let (artifacts, _bus, _dir) = fresh_store().await;
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        ensure_certificates(&artifacts, bind_addr, None).await.unwrap();

        let shared = load_shared_config(&artifacts).await.unwrap();

        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = tcp.local_addr().unwrap();
        let mut tls_listener = TlsListener::new(tcp, shared);

        let server = tokio::spawn(async move {
            let _ = tls_listener.accept().await;
        });

        let ca_der = read_artifact(&artifacts, &CA_CERT).await.unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(ca_der.as_slice().into()).unwrap();
        let client_config = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = TlsConnector::from(Arc::new(client_config));

        let stream = timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(s) = TcpStream::connect(bound).await {
                    return s;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("connect within timeout");

        let server_name = ServerName::try_from("localhost").unwrap();
        let handshake = timeout(
            Duration::from_secs(2),
            connector.connect(server_name, stream),
        )
        .await;
        let _ = timeout(Duration::from_secs(1), server).await;
        match handshake {
            Ok(Ok(_)) => { /* handshake completed */ }
            Ok(Err(err)) => panic!("TLS handshake failed: {err}"),
            Err(err) => {
                panic!("TLS handshake against generated PKI should complete within timeout: {err}")
            }
        }
    }

    /// Half-written key (new key, stale cert) must fail reload and not swap
    /// the shared config.
    #[tokio::test]
    async fn reload_fails_on_half_written_key() {
        install_crypto();
        let (artifacts, _bus, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr, None).await.unwrap();

        let config = load_shared_config(&artifacts).await.unwrap();
        let before = config.load_full();

        let fresh_key = spawn_blocking(|| {
            let key = KeyPair::generate().unwrap();
            PrivatePkcs8KeyDer::from(key.serialize_der())
        })
        .await
        .unwrap();
        store_key_artifact(&artifacts, &SERVER_KEY, &fresh_key)
            .await
            .unwrap();

        let result = reload_certificates(&artifacts, &config).await;
        assert!(
            result.is_err(),
            "reload must fail when key and cert are out of sync"
        );
        let after = config.load_full();
        assert!(
            Arc::ptr_eq(&before, &after),
            "shared config must not be swapped after a failed reload"
        );
    }
}
