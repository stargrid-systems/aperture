//! PKI generation and certificate loading.
//!
//! On first run, a self-signed CA (ECDSA P-256, 5-year validity) and a server
//! certificate (14-day validity) are generated via rcgen. Both are stored as
//! artifacts in DER format. The server certificate is regenerated periodically
//! by the rotation task.
//!
//! # What lives on disk
//!
//! Four artifacts live under the data directory, all in binary DER form:
//!
//! | Key              | Media type             | Sensitivity |
//! | ---------------- | ---------------------- | ----------- |
//! | `tls/ca-cert`    | `application/pkix-cert`| public      |
//! | `tls/ca-key`     | `application/pkcs8`    | **secret**  |
//! | `tls/server-cert`| `application/pkix-cert`| public      |
//! | `tls/server-key` | `application/pkcs8`    | **secret**  |
//!
//! The CA private key is the single most security-sensitive file the gateway
//! writes. Anyone who can read it can mint certs the gateway will trust.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;

use aperture_artifacts::Artifacts;
use aperture_storage::ArtifactKey;
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

/// CA certificate validity in days (5 years).
const CA_VALIDITY_DAYS: u64 = 365 * 5;

/// Leaf certificate validity in days (short-lived, ACME-style).
const LEAF_VALIDITY_DAYS: u64 = 14;

/// Default CN stamped on the leaf cert on first run.
const LEAF_COMMON_NAME: &str = "Aperture Gateway";

/// Default CN stamped on the self-signed CA on first run.
const CA_COMMON_NAME: &str = "Aperture Gateway CA";

/// Generated PKI material in DER format.
struct Pki {
    ca_cert: CertificateDer<'static>,
    ca_key: PrivatePkcs8KeyDer<'static>,
    server_cert: CertificateDer<'static>,
    server_key: PrivatePkcs8KeyDer<'static>,
}

/// Generates a new CA and leaf certificate signed by that CA.
///
/// Performs ECDSA key generation and certificate signing, so it should run
/// in a blocking context.
fn generate_pki(bind_addr: SocketAddr) -> Result<Pki, TlsError> {
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

    let sans = compute_sans(bind_addr);
    let subject = default_leaf_subject();
    let (server_cert, server_key) = generate_leaf(&issuer, &subject, &sans)?;
    Ok(Pki {
        ca_cert: ca_cert_der,
        ca_key: ca_key_der,
        server_cert,
        server_key,
    })
}

/// Re-issues the leaf cert without changing its identity.
///
/// Reads the existing cert, extracts its subject DN and SANs, then signs a
/// fresh cert (new key, fresh validity) against the CA stored in artifacts.
/// Rotation therefore preserves whatever identity the operator set up:
/// `bind_addr` is not a rotation concern, only first-run generation is.
async fn regenerate_leaf_for_rotation(
    artifacts: &Artifacts,
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
    let ca_cert_der = read_artifact(artifacts, &CA_CERT).await?;
    let ca_key_der = read_artifact(artifacts, &CA_KEY).await?;
    let leaf_der = read_artifact(artifacts, &SERVER_CERT).await?;

    spawn_blocking(move || {
        let ca_key = KeyPair::try_from(ca_key_der.as_slice())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key)?;
        let (subject, sans) = extract_leaf_identity(&leaf_der)?;
        generate_leaf(&issuer, &subject, &sans)
    })
    .await?
}

/// Generates a fresh leaf against the existing CA using default identity.
///
/// Used by `ensure_certificates` when the CA pair is intact but the leaf is
/// missing. The identity (CN + bind-addr-derived SANs) is the same shape the
/// first-run path produces, so a recovered catalog is indistinguishable from
/// a fresh one.
async fn regenerate_leaf_with_default_identity(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
    let ca_cert_der = read_artifact(artifacts, &CA_CERT).await?;
    let ca_key_der = read_artifact(artifacts, &CA_KEY).await?;

    spawn_blocking(move || {
        let ca_key = KeyPair::try_from(ca_key_der.as_slice())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key)?;
        let subject = default_leaf_subject();
        let sans = compute_sans(bind_addr);
        generate_leaf(&issuer, &subject, &sans)
    })
    .await?
}

fn generate_leaf(
    issuer: &Issuer<'_, KeyPair>,
    subject: &DistinguishedName,
    sans: &[SanType],
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
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

/// The subject DN used on first run. Rotation copies the subject from the
/// existing cert instead of using this default.
fn default_leaf_subject() -> DistinguishedName {
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, LEAF_COMMON_NAME);
    dn
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

/// Extracts the subject DN and SANs from an existing leaf cert.
///
/// Used by rotation to re-issue the cert with the same identity. Supports
/// the SAN variants our own PKI produces (`DnsName`, `IpAddress`) plus a
/// few common extras (`RFC822Name`, `URI`). Unsupported variants cause an
/// error so a future change to the SAN set is forced through review rather
/// than silently dropped.
fn extract_leaf_identity(der: &[u8]) -> Result<(DistinguishedName, Vec<SanType>), TlsError> {
    let (_, cert) = x509_parser::parse_x509_certificate(der).map_err(|e| TlsError::CertParse {
        source: anyhow::Error::from(e).context("parsing leaf for rotation"),
    })?;

    let subject = x509_name_to_rcgen(cert.subject())?;

    let sans = match cert.subject_alternative_name() {
        Ok(Some(ext)) => ext
            .value
            .general_names
            .iter()
            .map(general_name_to_rcgen)
            .collect::<Result<Vec<_>, _>>()?,
        Ok(None) => Vec::new(),
        Err(e) => {
            return Err(TlsError::CertParse {
                source: anyhow::Error::from(e).context("parsing subject alternative names"),
            });
        }
    };

    Ok((subject, sans))
}

/// Converts an x509-parser subject DN into an rcgen `DistinguishedName`.
fn x509_name_to_rcgen(name: &X509Name<'_>) -> Result<DistinguishedName, TlsError> {
    let mut dn = DistinguishedName::new();
    for attr in name.iter_attributes() {
        let oid_iter = attr.attr_type().iter();
        let oid_vec: Vec<u64> = match oid_iter {
            Some(it) => it.collect(),
            None => {
                return Err(TlsError::CertParse {
                    source: anyhow::anyhow!(
                        "non-standard OID encoding in subject attribute; cannot rotate"
                    ),
                });
            }
        };
        let dn_type = DnType::from_oid(&oid_vec);
        let value = attr.as_str().map_err(|e| TlsError::CertParse {
            source: anyhow::Error::from(e).context("reading subject attribute value"),
        })?;
        dn.push(dn_type, value.to_owned());
    }
    Ok(dn)
}

/// Converts one x509-parser `GeneralName` into an rcgen `SanType`.
///
/// Returns an error for unsupported variants. Our own PKI only produces
/// `DnsName` and `IpAddress`, but custom certs may include others. An error
/// forces the operator to either drop the unsupported SAN or extend this
/// converter.
fn general_name_to_rcgen(name: &GeneralName<'_>) -> Result<SanType, TlsError> {
    let san = match name {
        GeneralName::DNSName(s) => SanType::DnsName((*s).try_into()?),
        GeneralName::RFC822Name(s) => SanType::Rfc822Name((*s).try_into()?),
        GeneralName::URI(s) => SanType::URI((*s).try_into()?),
        GeneralName::IPAddress(octets) => {
            let ip = ip_addr_from_octets(octets).map_err(|msg| TlsError::CertParse {
                source: anyhow::Error::msg(msg).context("invalid IP SAN"),
            })?;
            SanType::IpAddress(ip)
        }
        other => {
            return Err(TlsError::CertParse {
                source: anyhow::anyhow!(
                    "unsupported SAN variant during rotation: {other:?}; rotation only supports \
                     DNS, IP, RFC822, and URI"
                ),
            });
        }
    };
    Ok(san)
}

/// Maps an x509-parser IP octet slice to `IpAddr`. Mirrors rcgen's private
/// helper so we can preserve IP SANs through rotation.
fn ip_addr_from_octets(octets: &[u8]) -> Result<IpAddr, &'static str> {
    if let Ok(ipv6_octets) = <&[u8; 16]>::try_from(octets) {
        Ok(IpAddr::V6(Ipv6Addr::from(*ipv6_octets)))
    } else if let Ok(ipv4_octets) = <&[u8; 4]>::try_from(octets) {
        Ok(IpAddr::V4(Ipv4Addr::from(*ipv4_octets)))
    } else {
        Err("IP SAN has invalid octet length")
    }
}

/// Ensures TLS certificate artifacts exist, generating them on first run.
///
/// Splits the work in two halves so a partial state is repaired surgically:
///
/// - If either CA artifact is missing, the entire CA pair is regenerated. When
///   that happens any pre-existing leaf is signed by a now-rotated CA, so the
///   leaf is regenerated unconditionally against the new CA.
/// - If the CA pair is intact but either leaf artifact is missing, only the
///   leaf pair is regenerated using the existing CA. This avoids throwing away
///   a perfectly good CA (and an operator's custom CA) just because a single
///   artifact went missing.
///
/// Within each pair, the key is written before its matching cert. A crash
/// between the two writes leaves a state that rustls rejects at load time
/// (stale cert, new key) rather than silently corrupting handshakes.
pub async fn ensure_certificates(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
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
        let pki = spawn_blocking(move || generate_pki(bind_addr)).await??;
        store_key_artifact(artifacts, &CA_KEY, &pki.ca_key).await?;
        store_cert_artifact(artifacts, &CA_CERT, &pki.ca_cert).await?;
        // The CA pair just rotated. Any prior leaf is signed by the old CA,
        // so regenerate the leaf unconditionally instead of trusting a stale
        // signature. On a true first run there is no prior leaf to overwrite.
        store_key_artifact(artifacts, &SERVER_KEY, &pki.server_key).await?;
        store_cert_artifact(artifacts, &SERVER_CERT, &pki.server_cert).await?;
        return Ok(());
    }

    // CA pair intact, only the leaf is missing.
    tracing::info!("regenerating leaf certificate against existing CA");
    let (cert, key) = regenerate_leaf_with_default_identity(artifacts, bind_addr).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Loads the server certificate from artifacts and builds a `ServerConfig`.
pub async fn load_server_config(artifacts: &Artifacts) -> Result<ServerConfig, TlsError> {
    let cert_der = read_artifact(artifacts, &SERVER_CERT).await?;
    let key_der = read_artifact(artifacts, &SERVER_KEY).await?;
    spawn_blocking(move || build_server_config(&cert_der, &key_der)).await?
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

/// Returns true when the server certificate is past half of its own validity.
///
/// The threshold is computed from the cert's `not_before` and `not_after`, not
/// from the hardcoded `LEAF_VALIDITY_DAYS`, so uploaded custom certs keep their
/// own lifetime regardless of the default rotation policy.
async fn needs_rotation(artifacts: &Artifacts) -> Result<bool, TlsError> {
    let der = read_artifact(artifacts, &SERVER_CERT).await?;
    let (_, cert) = x509_parser::parse_x509_certificate(&der).map_err(|e| TlsError::CertParse {
        source: anyhow::Error::from(e),
    })?;
    let validity = cert.validity();
    // `time_to_expiration` returns None when the cert is not currently valid
    // (expired or not-yet-effective). Either way, rotation is wanted.
    let Some(remaining) = validity.time_to_expiration() else {
        return Ok(true);
    };
    // `not_after - not_before` is None only if the cert is malformed
    // (not_after <= not_before). Treat that as needing rotation too.
    let Some(total) = validity.not_after - validity.not_before else {
        return Ok(true);
    };
    Ok(remaining < total / 2)
}

/// Generates a new leaf certificate and stores it as artifacts.
///
/// The new leaf copies the subject DN and SANs from the existing cert, so
/// rotation does not change the cert's identity. Only the key and validity
/// window move forward. See [`extract_leaf_identity`].
///
/// The key is written before the cert. Combined with the reload watcher's
/// debounce, this guarantees that a half-write (new key, stale cert) is
/// detected as a load failure rather than silently corrupting handshakes.
/// The next write (the cert) schedules another reload that succeeds.
async fn rotate_certificate(artifacts: &Artifacts) -> Result<(), TlsError> {
    let (cert, key) = regenerate_leaf_for_rotation(artifacts).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Regenerates the leaf when it is due, reporting whether rotation occurred.
///
/// Live reload of the TLS listener is triggered separately by the artifact
/// change feed (see [`crate::tls::TlsReload`]).
pub(super) async fn rotate_if_due(artifacts: &Artifacts) -> Result<bool, TlsError> {
    if !needs_rotation(artifacts).await? {
        return Ok(false);
    }
    rotate_certificate(artifacts).await?;
    Ok(true)
}

/// Builds a `rustls::ServerConfig` from DER-encoded cert and key.
///
/// Enables TLS 1.3 (preferred) and TLS 1.2 (for legacy client compatibility).
/// TLS 1.1 and earlier are not negotiated.
fn build_server_config(cert_der: &[u8], key_der: &[u8]) -> Result<ServerConfig, TlsError> {
    use rustls::version::{TLS12, TLS13};

    let cert_chain = vec![CertificateDer::from(cert_der.to_vec())];
    let key = PrivateKeyDer::try_from(key_der.to_vec()).map_err(|e| TlsError::CertParse {
        source: anyhow::anyhow!("key parse failed: {e}"),
    })?;

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
    artifacts
        .put(key, Some("application/pkix-cert"), der.as_ref())
        .await?;
    Ok(())
}

async fn store_key_artifact(
    artifacts: &Artifacts,
    key: &ArtifactKey,
    der: &PrivatePkcs8KeyDer<'_>,
) -> Result<(), TlsError> {
    artifacts
        .put(key, Some("application/pkcs8"), der.secret_pkcs8_der())
        .await?;
    Ok(())
}

async fn read_artifact(artifacts: &Artifacts, key: &ArtifactKey) -> Result<Vec<u8>, TlsError> {
    let located = artifacts
        .locate(key)
        .await?
        .ok_or(TlsError::NoCertificate)?;
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

    use aperture_storage::Storage;

    use super::*;
    use crate::tls::{TlsReload, load_shared_config};

    /// Installs the ring crypto provider once per test process.
    fn install_crypto() {
        use std::sync::Once;

        use rustls::crypto::ring;

        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let _ = ring::default_provider().install_default();
        });
    }

    /// A temporary blob store directory removed when dropped.
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

    async fn fresh_store() -> (Artifacts, TempDir) {
        let storage = Storage::open(":memory:").await.unwrap();
        let dir = TempDir::new();
        let artifacts = Artifacts::new(storage, dir.0.clone());
        (artifacts, dir)
    }

    #[tokio::test]
    async fn fresh_cert_does_not_need_rotation() {
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();
        assert!(!needs_rotation(&artifacts).await.unwrap());
    }

    #[tokio::test]
    async fn expired_cert_needs_rotation() {
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        // Generate a cert that is already expired and overwrite the artifact.
        let ca_cert_der = read_artifact(&artifacts, &CA_CERT).await.unwrap();
        let ca_key_der = read_artifact(&artifacts, &CA_KEY).await.unwrap();
        let ca_key = KeyPair::try_from(ca_key_der.as_slice()).unwrap();
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key).unwrap();

        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "expired");
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        // Already expired a day ago.
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

    /// Returns the current digest of `key`, panicking if absent.
    async fn digest_of(artifacts: &Artifacts, key: &ArtifactKey) -> String {
        artifacts
            .artifact(key)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone()
    }

    /// When a leaf artifact is missing but the CA pair is intact,
    /// `ensure_certificates` regenerates only the leaf. The CA and its key
    /// are preserved. This guards against throwing away an operator-uploaded
    /// CA just because one leaf went missing.
    #[tokio::test]
    async fn ensure_certificates_regenerates_only_leaf_when_ca_pair_intact() {
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        let old_ca_cert = digest_of(&artifacts, &CA_CERT).await;
        let old_ca_key = digest_of(&artifacts, &CA_KEY).await;
        let old_server_cert = digest_of(&artifacts, &SERVER_CERT).await;
        let old_server_key = digest_of(&artifacts, &SERVER_KEY).await;

        // Drop only the leaf cert. CA pair stays untouched.
        artifacts
            .evict_version(&SERVER_CERT, &old_server_cert)
            .await
            .unwrap();

        ensure_certificates(&artifacts, addr).await.unwrap();

        // CA pair unchanged.
        assert_eq!(digest_of(&artifacts, &CA_CERT).await, old_ca_cert);
        assert_eq!(digest_of(&artifacts, &CA_KEY).await, old_ca_key);
        // Leaf pair regenerated.
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

    /// When any CA artifact is missing, the entire PKI is regenerated.
    /// The leaf is also re-issued because it was signed by the now-rotated CA.
    #[tokio::test]
    async fn ensure_certificates_regenerates_everything_when_ca_pair_missing() {
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        let old_ca_cert = digest_of(&artifacts, &CA_CERT).await;
        let old_ca_key = digest_of(&artifacts, &CA_KEY).await;
        let old_server_cert = digest_of(&artifacts, &SERVER_CERT).await;
        let old_server_key = digest_of(&artifacts, &SERVER_KEY).await;

        // Drop only the CA cert. The new code path treats this as "CA pair
        // missing" and regenerates everything.
        artifacts
            .evict_version(&CA_CERT, &old_ca_cert)
            .await
            .unwrap();

        ensure_certificates(&artifacts, addr).await.unwrap();

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
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();
        let config = load_server_config(&artifacts).await.unwrap();
        // A successfully built ServerConfig can provide a cert resolver.
        // We just verify no panic and no error.
        let _ = config;
    }

    #[tokio::test]
    async fn load_server_config_fails_on_corrupt_key() {
        install_crypto();
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        // Overwrite the key with garbage.
        artifacts
            .put(&SERVER_KEY, Some("application/pkcs8"), &b"corrupt"[..])
            .await
            .unwrap();

        assert!(load_server_config(&artifacts).await.is_err());
    }

    #[tokio::test]
    async fn reload_certificates_swaps_shared_config() {
        install_crypto();
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

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
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        let rotated = rotate_if_due(&artifacts).await.unwrap();
        assert!(!rotated, "fresh cert should not need rotation");
    }

    /// Rotation preserves the existing cert's identity. This test mints a leaf
    /// with a custom CN and a non-default SAN, runs rotation, and asserts the
    /// rotated cert carries the same identity. Catches a regression that
    /// silently shrinks the SAN set on every rotation.
    #[tokio::test]
    async fn rotation_preserves_subject_and_sans() {
        use x509_parser::extensions::GeneralName as X509GeneralName;

        install_crypto();
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        // Mint a custom leaf signed by the existing CA, with a recognisable
        // CN and an extra SAN the default flow would not produce.
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
        install_crypto();
        use std::time::Duration;

        use tokio::time::{sleep, timeout};
        use tokio_util::sync::CancellationToken;

        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        let config = load_shared_config(&artifacts).await.unwrap();
        let old = config.load_full();

        let reload = TlsReload::new(artifacts.clone(), config.clone());
        let token = CancellationToken::new();
        let handle = tokio::spawn(reload.run(token.clone()));

        // Trigger a cert change via the rotation path.
        rotate_certificate(&artifacts).await.unwrap();

        // The watcher debounces for 500 ms, then reloads. Poll for the swap
        // with a generous outer timeout so the test fails cleanly instead of
        // hanging on a fixed sleep.
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

    /// End-to-end check that the generated PKI actually works for a TLS
    /// handshake. The unit tests above verify the pieces in isolation. This
    /// one wires `ensure_certificates`, `load_shared_config`, `TlsListener`,
    /// and a `TlsConnector` rooted at the generated CA together so a
    /// composition bug does not slip past the test suite.
    #[tokio::test]
    async fn generated_pki_completes_a_real_tls_handshake() {
        install_crypto();
        use std::time::Duration;

        use axum::serve::Listener;
        use rustls::pki_types::ServerName;
        use rustls::{ClientConfig, RootCertStore};
        use tokio::net::{TcpListener, TcpStream};
        use tokio::time::{sleep, timeout};
        use tokio_rustls::TlsConnector;

        use super::super::TlsListener;

        let (artifacts, _dir) = fresh_store().await;
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        ensure_certificates(&artifacts, bind_addr).await.unwrap();

        let shared = load_shared_config(&artifacts).await.unwrap();

        let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bound = tcp.local_addr().unwrap();
        let mut tls_listener = TlsListener::new(tcp, shared);

        let server = tokio::spawn(async move {
            // One accept is enough. The trait drives the handshake.
            let _ = tls_listener.accept().await;
        });

        // Build a client that trusts only the CA we generated. The leaf cert
        // carries `localhost` and the loopback IPs as SANs, so connecting by
        // name should verify cleanly.
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
        // Make sure the server task returns even on failure so its panicked
        // status surfaces.
        let _ = timeout(Duration::from_secs(1), server).await;
        match handshake {
            Ok(Ok(_)) => { /* handshake completed */ }
            Ok(Err(err)) => panic!("TLS handshake failed: {err}"),
            Err(_) => panic!("TLS handshake against generated PKI should complete within timeout"),
        }
    }

    /// Regression test for the key-before-cert write ordering.
    ///
    /// The doc on `rotate_certificate` promises: write key first, then cert.
    /// If the writer is preempted between the two writes, the reload watcher
    /// must detect the mismatch (fresh key, stale cert) as a load failure
    /// rather than silently swapping in a broken config. This test simulates
    /// the half-write by writing only a fresh key and then asking the reload
    /// path to load. The load must fail; the shared config must not change.
    #[tokio::test]
    async fn reload_fails_on_half_written_key() {
        install_crypto();
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        let config = load_shared_config(&artifacts).await.unwrap();
        let before = config.load_full();

        // Write a fresh key but keep the stale cert. This is the exact state
        // the doc warns about: a crash between the two writes of
        // `rotate_certificate` would leave the catalog here.
        let fresh_key = spawn_blocking(|| {
            let key = KeyPair::generate().unwrap();
            PrivatePkcs8KeyDer::from(key.serialize_der())
        })
        .await
        .unwrap();
        store_key_artifact(&artifacts, &SERVER_KEY, &fresh_key)
            .await
            .unwrap();

        // The reload reads both artifacts. Fresh key + stale cert is a
        // mismatch rustls rejects. The shared config must be unchanged.
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
