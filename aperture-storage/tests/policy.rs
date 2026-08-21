use std::slice::from_ref;

use aperture_storage::{PolicyType, Storage};

#[tokio::test]
async fn insert_batch_skips_duplicate_rules() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.policy().unwrap();
    let rule = (
        PolicyType::Policy,
        vec!["admin".to_owned(), "*".to_owned(), "*".to_owned()],
    );

    repo.insert_batch(from_ref(&rule)).await.unwrap();
    repo.insert_batch(&[rule.clone(), rule]).await.unwrap();

    assert_eq!(repo.count().await.unwrap(), 1);
}

#[tokio::test]
async fn insert_skips_duplicate_rules() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.policy().unwrap();
    let rule = ["admin".to_owned(), "*".to_owned(), "*".to_owned()];

    repo.insert(PolicyType::Policy, &rule).await.unwrap();
    // A duplicate single-row add is a no-op rather than a constraint error.
    repo.insert(PolicyType::Policy, &rule).await.unwrap();

    assert_eq!(repo.count().await.unwrap(), 1);
}

#[tokio::test]
async fn insert_skips_duplicates_with_null_tails() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.policy().unwrap();
    // A grouping rule has two values, so v2 through v5 are NULL.
    let rule = ["actor:1".to_owned(), "viewer".to_owned()];

    repo.insert(PolicyType::Grouping, &rule).await.unwrap();
    repo.insert(PolicyType::Grouping, &rule).await.unwrap();
    repo.insert_batch(&[(
        PolicyType::Grouping,
        vec!["actor:1".to_owned(), "viewer".to_owned()],
    )])
    .await
    .unwrap();

    assert_eq!(repo.count().await.unwrap(), 1);
}

#[tokio::test]
async fn replace_all_swaps_the_full_rule_set() {
    let storage = Storage::open(":memory:").await.unwrap();
    let repo = storage.policy().unwrap();
    repo.insert(
        PolicyType::Policy,
        &["admin".to_owned(), "*".to_owned(), "*".to_owned()],
    )
    .await
    .unwrap();

    repo.replace_all(&[
        (
            PolicyType::Policy,
            vec!["viewer".to_owned(), "event".to_owned(), "read".to_owned()],
        ),
        (
            PolicyType::Grouping,
            vec!["actor:1".to_owned(), "viewer".to_owned()],
        ),
    ])
    .await
    .unwrap();

    let rules = repo.load_all().await.unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(
        rules[0],
        aperture_storage::PolicyRule {
            ptype: PolicyType::Policy,
            values: vec![
                Some("viewer".to_owned()),
                Some("event".to_owned()),
                Some("read".to_owned()),
                None,
                None,
                None
            ]
        }
    );
    assert_eq!(
        rules[1],
        aperture_storage::PolicyRule {
            ptype: PolicyType::Grouping,
            values: vec![
                Some("actor:1".to_owned()),
                Some("viewer".to_owned()),
                None,
                None,
                None,
                None
            ]
        }
    );
}
