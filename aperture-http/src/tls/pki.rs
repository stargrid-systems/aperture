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
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::fs;
use tokio::io::AsyncReadExt;
use tokio::task::spawn_blocking;

use super::error::TlsError;
use super::{CA_CERT, CA_KEY, SERVER_CERT, SERVER_KEY, SharedConfig};

/// CA certificate validity in days (5 years).
const CA_VALIDITY_DAYS: u64 = 365 * 5;

/// Leaf certificate validity in days (short-lived, ACME-style).
const LEAF_VALIDITY_DAYS: u64 = 14;

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
        .push(rcgen::DnType::CommonName, "Aperture Gateway CA");
    set_validity(&mut ca_params, CA_VALIDITY_DAYS);

    let ca_key = KeyPair::generate()?;
    let ca_key_der = PrivatePkcs8KeyDer::from(ca_key.serialize_der());
    let ca_cert = ca_params.self_signed(&ca_key)?;
    let ca_cert_der = ca_cert.der().clone();
    let issuer = Issuer::new(ca_params, ca_key);

    let sans = compute_sans(bind_addr);
    let (server_cert, server_key) = generate_leaf(&issuer, &sans)?;
    Ok(Pki {
        ca_cert: ca_cert_der,
        ca_key: ca_key_der,
        server_cert,
        server_key,
    })
}

/// Generates a new leaf certificate signed by the CA stored in artifacts.
///
/// Reads the CA material asynchronously, then performs key generation and
/// certificate signing in a blocking task.
async fn regenerate_leaf(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
    let ca_cert_der = read_artifact(artifacts, &CA_CERT).await?;
    let ca_key_der = read_artifact(artifacts, &CA_KEY).await?;

    spawn_blocking(move || {
        let ca_key = KeyPair::try_from(ca_key_der.as_slice())?;
        let issuer = Issuer::from_ca_cert_der(&CertificateDer::from(ca_cert_der), ca_key)?;
        let sans = compute_sans(bind_addr);
        generate_leaf(&issuer, &sans)
    })
    .await?
}

fn generate_leaf(
    issuer: &Issuer<'_, KeyPair>,
    sans: &[SanType],
) -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), TlsError> {
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
    Ok((
        cert.der().clone(),
        PrivatePkcs8KeyDer::from(key.serialize_der()),
    ))
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
///
/// All four artifacts must be present together. If any is missing (e.g. from a
/// partially-interrupted first run), the entire PKI is regenerated so the
/// rotation code can rely on a complete set. Keys are written before their
/// matching certs so a crash between writes leaves a state that rustls rejects
/// at load time rather than silently corrupting handshakes.
pub async fn ensure_certificates(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<(), TlsError> {
    if artifacts.locate(&CA_CERT).await?.is_some()
        && artifacts.locate(&CA_KEY).await?.is_some()
        && artifacts.locate(&SERVER_CERT).await?.is_some()
        && artifacts.locate(&SERVER_KEY).await?.is_some()
    {
        return Ok(());
    }
    tracing::info!("generating initial PKI");
    let pki = spawn_blocking(move || generate_pki(bind_addr)).await??;
    store_key_artifact(artifacts, &CA_KEY, &pki.ca_key).await?;
    store_cert_artifact(artifacts, &CA_CERT, &pki.ca_cert).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &pki.server_key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &pki.server_cert).await?;
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
    let (_, cert) = x509_parser::parse_x509_certificate(&der)
        .map_err(|e| TlsError::CertParse(format!("x509 parse failed: {e}")))?;
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
/// The key is written before the cert so a crash between writes leaves a state
/// that rustls rejects (stale cert, new key) rather than silently corrupting
/// handshakes (new cert, stale key).
async fn rotate_certificate(artifacts: &Artifacts, bind_addr: SocketAddr) -> Result<(), TlsError> {
    let (cert, key) = regenerate_leaf(artifacts, bind_addr).await?;
    store_key_artifact(artifacts, &SERVER_KEY, &key).await?;
    store_cert_artifact(artifacts, &SERVER_CERT, &cert).await?;
    Ok(())
}

/// Regenerates the leaf when it is due, reporting whether rotation occurred.
///
/// Live reload of the TLS listener is triggered separately by the artifact
/// change feed (see [`crate::tls::TlsReload`]).
pub(super) async fn rotate_if_due(
    artifacts: &Artifacts,
    bind_addr: SocketAddr,
) -> Result<bool, TlsError> {
    if !needs_rotation(artifacts).await? {
        return Ok(false);
    }
    rotate_certificate(artifacts, bind_addr).await?;
    Ok(true)
}

/// Builds a `rustls::ServerConfig` from DER-encoded cert and key.
///
/// Enables TLS 1.3 (preferred) and TLS 1.2 (for legacy client compatibility).
/// TLS 1.1 and earlier are not negotiated.
fn build_server_config(cert_der: &[u8], key_der: &[u8]) -> Result<ServerConfig, TlsError> {
    use rustls::version::{TLS12, TLS13};

    let cert_chain = vec![CertificateDer::from(cert_der.to_vec())];
    let key = PrivateKeyDer::try_from(key_der.to_vec())
        .map_err(|e| TlsError::CertParse(format!("key parse failed: {e}")))?;

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

    #[tokio::test]
    async fn ensure_certificates_regenerates_when_any_artifact_missing() {
        let (artifacts, _dir) = fresh_store().await;
        let addr: SocketAddr = "[::1]:8443".parse().unwrap();
        ensure_certificates(&artifacts, addr).await.unwrap();

        // Sanity: complete set is in place.
        assert!(artifacts.locate(&SERVER_CERT).await.unwrap().is_some());

        // Capture the four initial digests so we can confirm regeneration.
        let old_ca_cert = artifacts
            .artifact(&CA_CERT)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let old_ca_key = artifacts
            .artifact(&CA_KEY)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let old_server_cert = artifacts
            .artifact(&SERVER_CERT)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let old_server_key = artifacts
            .artifact(&SERVER_KEY)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();

        // Drop the server cert. ensure_certificates should regenerate the PKI
        // (and a new server-cert/key pair along with it).
        artifacts
            .evict_version(&SERVER_CERT, &old_server_cert)
            .await
            .unwrap();

        ensure_certificates(&artifacts, addr).await.unwrap();

        let new_ca_cert = artifacts
            .artifact(&CA_CERT)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let new_ca_key = artifacts
            .artifact(&CA_KEY)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let new_server_cert = artifacts
            .artifact(&SERVER_CERT)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();
        let new_server_key = artifacts
            .artifact(&SERVER_KEY)
            .await
            .unwrap()
            .unwrap()
            .latest
            .digest
            .clone();

        // All four artifacts should be present with new digests. The docstring
        // promises the entire PKI is regenerated, so verifying every digest
        // changed guards against a future regression that only writes one.
        assert!(artifacts.locate(&SERVER_CERT).await.unwrap().is_some());
        assert!(artifacts.locate(&SERVER_KEY).await.unwrap().is_some());
        assert!(artifacts.locate(&CA_CERT).await.unwrap().is_some());
        assert!(artifacts.locate(&CA_KEY).await.unwrap().is_some());
        assert_ne!(
            new_ca_cert, old_ca_cert,
            "CA cert should have been regenerated"
        );
        assert_ne!(
            new_ca_key, old_ca_key,
            "CA key should have been regenerated"
        );
        assert_ne!(
            new_server_cert, old_server_cert,
            "server cert should have been regenerated"
        );
        assert_ne!(
            new_server_key, old_server_key,
            "server key should have been regenerated"
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

        rotate_certificate(&artifacts, addr).await.unwrap();
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

        let rotated = rotate_if_due(&artifacts, addr).await.unwrap();
        assert!(!rotated, "fresh cert should not need rotation");
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
        rotate_certificate(&artifacts, addr).await.unwrap();

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
}
