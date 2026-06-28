use std::{env, fs, process};

use aperture_artifacts::BlobStore;
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[tokio::test]
async fn put_is_content_addressed_and_idempotent() {
    let root = env::temp_dir().join(format!("aperture-blobstore-test-{}", process::id()));
    let _ = fs::remove_dir_all(&root);
    let store = BlobStore::new(root.clone());

    let data = b"hello aperture";
    let (digest, size) = store.put(&data[..]).await.unwrap();

    assert_eq!(size, data.len() as u64);
    assert_eq!(digest.hex().len(), 64);
    assert!(digest.to_string().starts_with("sha256:"));
    assert!(store.contains(&digest).await);

    let mut file = File::open(store.path(&digest)).await.unwrap();
    let mut stored = Vec::new();
    file.read_to_end(&mut stored).await.unwrap();
    assert_eq!(stored, data);

    // Same content produces the same digest.
    let (again, _) = store.put(&data[..]).await.unwrap();
    assert_eq!(digest, again);

    let _ = fs::remove_dir_all(&root);
}
