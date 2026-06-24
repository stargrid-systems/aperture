use std::{env, fs};

use aperture_artifacts::{Artifacts, MediaType};
use aperture_storage::{ArtifactKind, ArtifactStatus, Storage};

#[tokio::test]
#[ignore = "network: pulls the spectra image from ghcr.io"]
async fn pulls_spectra_from_ghcr() {
    let storage = Storage::open(":memory:").await.unwrap();
    let root = env::temp_dir().join("aperture-oci-it");
    let _ = fs::remove_dir_all(&root);

    let artifacts = Artifacts::new(storage, root.clone());
    let media_type = MediaType::from("application/vnd.spectra.tar+gzip");
    let located = artifacts
        .ensure_oci(
            "spectra",
            ArtifactKind::Oci,
            "ghcr.io/stargrid-systems/spectra:0.2.0",
            &media_type,
        )
        .await
        .unwrap();

    assert!(located.path.is_file());
    assert!(located.digest.to_string().starts_with("sha256:"));

    let repo = artifacts.storage().artifacts();
    let artifact = repo.get("spectra").await.unwrap().unwrap();
    assert_eq!(artifact.status, ArtifactStatus::Present);
    assert!(artifact.size_bytes.unwrap_or(0) > 0);
    assert_eq!(repo.downloads_for("spectra").await.unwrap().len(), 1);

    // A second ensure finds the blob present and does not download again.
    artifacts
        .ensure_oci(
            "spectra",
            ArtifactKind::Oci,
            "ghcr.io/stargrid-systems/spectra:0.2.0",
            &media_type,
        )
        .await
        .unwrap();
    assert_eq!(repo.downloads_for("spectra").await.unwrap().len(), 1);
    assert!(artifacts.active_downloads().is_empty());

    // A sync with the blob present removes nothing.
    let report = artifacts.sync().await.unwrap();
    assert_eq!(report.removed_blobs, 0);
    assert_eq!(report.removed_entries, 0);

    let _ = fs::remove_dir_all(&root);
}
