use aperture_settings::{ListQuery, SettingDefinition, SettingError, SettingRegistry, Settings};
use aperture_storage::{Order, Storage};
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

macro_rules! stub_def {
    ($name:ident, $key:literal) => {
        struct $name;
        impl SettingDefinition for $name {
            const KEY: &'static str = $key;
            type Value = SystemValue;
            fn default(&self) -> Self::Value {
                SystemValue { hostname: None }
            }
        }
    };
}

stub_def!(DefA, "a");
stub_def!(DefB, "b");
stub_def!(DefC, "c");

fn registry() -> SettingRegistry {
    let mut registry = SettingRegistry::new();
    registry.register(SystemDef);
    registry
}

fn registry_multi() -> SettingRegistry {
    let mut registry = SettingRegistry::new();
    registry.register(DefA);
    registry.register(DefB);
    registry.register(DefC);
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

    let value = settings.get_value("system").await.unwrap();
    assert_eq!(value, serde_json::json!({"hostname": null}));
}

#[tokio::test]
async fn list_returns_registered_keys_with_defaults() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry());

    let page = settings.list(&ListQuery::default()).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].0, "system");
    assert_eq!(page.items[0].1, serde_json::json!({"hostname": null}));
    assert!(page.next_cursor.is_none());
    assert!(page.prev_cursor.is_none());
}

#[tokio::test]
async fn list_paginates_and_orders() {
    let storage = Storage::open(":memory:").await.unwrap();
    let settings = Settings::new(storage.settings().unwrap(), registry_multi());

    let page = settings
        .list(&ListQuery {
            limit: Some(2),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    assert_eq!(page.items[0].0, "a");
    assert_eq!(page.items[1].0, "b");
    assert!(page.next_cursor.is_some());
    assert!(page.prev_cursor.is_none());

    let page2 = settings
        .list(&ListQuery {
            limit: Some(2),
            cursor: page.next_cursor,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page2.items.len(), 1);
    assert_eq!(page2.items[0].0, "c");
    assert!(page2.next_cursor.is_none());
    assert!(page2.prev_cursor.is_some());

    let page_desc = settings
        .list(&ListQuery {
            limit: Some(2),
            order: Some(Order::Desc),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(page_desc.items.len(), 2);
    assert_eq!(page_desc.items[0].0, "c");
    assert_eq!(page_desc.items[1].0, "b");
}
