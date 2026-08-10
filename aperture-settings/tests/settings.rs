use aperture_settings::{SettingDefinition, SettingError, SettingRegistry, Settings};
use aperture_storage::Storage;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema, PartialEq)]
struct SystemValue {
    hostname: Option<String>,
}

struct SystemDef;

impl SettingDefinition for SystemDef {
    const KEY: &'static str = "system";
    type Value = SystemValue;

    fn default(&self) -> Self::Value {
        SystemValue { hostname: None }
    }
}

fn registry() -> SettingRegistry {
    let mut registry = SettingRegistry::new();
    registry.register(SystemDef);
    registry
}

#[tokio::test]
async fn get_value_returns_default_when_nothing_stored() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let value = settings.get_value("system").await.unwrap();
    assert_eq!(value, serde_json::json!({"hostname": null}));
}

#[tokio::test]
async fn get_typed_returns_default_when_nothing_stored() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let value: SystemValue = settings.get::<SystemDef>().await.unwrap();
    assert_eq!(value, SystemValue { hostname: None });
}

#[tokio::test]
async fn set_then_get_round_trips() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    settings
        .set_value(
            "system",
            serde_json::json!({"hostname": "aperture"}),
            aperture_storage::ActorId::SYSTEM,
        )
        .await
        .unwrap();

    let value = settings.get_value("system").await.unwrap();
    assert_eq!(value, serde_json::json!({"hostname": "aperture"}));
}

#[tokio::test]
async fn get_value_rejects_unregistered_key() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let err = settings.get_value("unknown").await.unwrap_err();
    assert!(matches!(err, SettingError::NotRegistered(_)));
}

#[tokio::test]
async fn set_value_rejects_unregistered_key() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let err = settings
        .set_value(
            "unknown",
            serde_json::json!({}),
            aperture_storage::ActorId::SYSTEM,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SettingError::NotRegistered(_)));
}

#[tokio::test]
async fn set_value_rejects_value_that_does_not_deserialize() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let err = settings
        .set_value(
            "system",
            serde_json::json!("not an object"),
            aperture_storage::ActorId::SYSTEM,
        )
        .await
        .unwrap_err();
    assert!(matches!(err, SettingError::Decode(_)));

    // Nothing was written, so the default is still returned.
    let value = settings.get_value("system").await.unwrap();
    assert_eq!(value, serde_json::json!({"hostname": null}));
}

#[tokio::test]
async fn list_returns_registered_keys_with_defaults() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let entries = settings.list().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, "system");
    assert_eq!(entries[0].1, serde_json::json!({"hostname": null}));
}
