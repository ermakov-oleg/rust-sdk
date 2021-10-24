// use std::borrow::Borrow;
// use std::collections::HashMap;
// use std::hash::Hash;
// use std::iter::Iterator;
// use std::sync::RwLock;
//
// use serde::de::DeserializeOwned;
//
// use crate::context::Context;
// use crate::filters::SettingsService;
// // use crate::providers::RuntimeSettingsProvider;
// // use crate::MicroserviceRuntimeSettingsProvider;
//
// type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
//
// pub struct RuntimeSettings {
//     settings: RwLock<HashMap<String, Vec<SettingsService>>>,
//     // mcs_settings_provider: MicroserviceRuntimeSettingsProvider<'static>,
//     version: RwLock<String>,
// }
//
// impl RuntimeSettings {
//     pub fn new(base_url: &str) -> Self {
//         // let mcs_settings_provider = MicroserviceRuntimeSettingsProvider::new(base_url);
//         Self {
//             settings: RwLock::new(HashMap::new()),
//             version: RwLock::from("0".to_string()),
//             // mcs_settings_provider,
//         }
//     }
//     //
//     // pub async fn refresh(&mut self) -> Result<()> {
//     //     // let new_settings = match self.mcs_settings_provider.get_settings(&self.version).await {
//     //         Ok(r) => r,
//     //         Err(err) => {
//     //             eprintln!("Error: Could not update settings {}", err);
//     //             return Err(err);
//     //         }
//     //     };
//     //
//     //     let mut settings_guard = self.settings.write().unwrap();
//     //     *settings_guard = new_settings;
//     //     println!("Settings refreshed");
//     //     Ok(())
//     // }
//     //
//     // pub async fn refresh_with_settings_provider(
//     //     &self,
//     //     settings_provider: &dyn RuntimeSettingsProvider,
//     // ) -> Result<()> {
//     //     let new_settings = match settings_provider.get_settings().await {
//     //         Ok(r) => r,
//     //         Err(err) => {
//     //             eprintln!("Error: Could not update settings {}", err);
//     //             return Ok(());
//     //         }
//     //     };
//     //     let mut settings_guard = self.settings.write().unwrap();
//     //     *settings_guard = new_settings;
//     //     println!("Settings refreshed");
//     //     Ok(())
//     // }
//
//     pub fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
//         where
//             String: Borrow<K>,
//             K: Hash + Eq,
//             V: DeserializeOwned,
//     {
//         let settings_guard = self.settings.read().unwrap();
//         let value = match settings_guard.get(key) {
//             Some(vss) => vss
//                 .iter()
//                 .find(|f| f.is_suitable(ctx))
//                 .and_then(|val| val.setting.value.clone()),
//             None => None,
//         };
//
//         value.and_then(|v| {
//             serde_json::from_str(&v)
//                 .map_err(|err| {
//                     eprintln!("Error when deserialize value {}", err);
//                 })
//                 .ok()
//         })
//     }
// }
//
// #[cfg(test)]
// mod tests {
//     use std::sync::Mutex;
//     use serde::Deserialize;
//     use async_trait::async_trait;
//     use crate::entities::{Filter, Setting};
//     use super::*;
//
//     type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
//
//     #[derive(Deserialize, Debug, PartialEq)]
//     struct SomeData {
//         key: String,
//     }
//
//     fn _make_ctx() -> Context {
//         Context {
//             application: "test-rust".to_string(),
//             server: "test-server".to_string(),
//             environment: Default::default(),
//             host: None,
//             url: None,
//             url_path: None,
//             email: None,
//             ip: None,
//             context: Default::default(),
//         }
//     }
//
//     struct TestSettingsProvider {
//         settings: Mutex<HashMap<String, Vec<SettingsService>>>,
//     }
//
//     #[async_trait]
//     impl RuntimeSettingsProvider for TestSettingsProvider {
//         async fn get_settings(&self) -> Result<HashMap<String, Vec<SettingsService>>> {
//             let mut guard = self.settings.lock().unwrap();
//             let settings = guard.drain();
//             Ok(settings.collect())
//         }
//     }
//
//     #[tokio::test]
//     async fn test_runtime_settings_refresh() {
//         let mut settings = HashMap::new();
//         settings.insert(
//             "TEST_KEY".to_string(),
//             vec![SettingsService::new(Setting {
//                 key: "TEST_KEY".to_string(),
//                 priority: 0,
//                 runtime: "rust".to_string(),
//                 filters: None,
//                 value: Some("{\"key\": \"value\"}".into()),
//             })],
//         );
//         let settings_provider = TestSettingsProvider {
//             settings: Mutex::new(settings),
//         };
//         let runtime_settings = RuntimeSettings::new(settings_provider);
//
//         runtime_settings.refresh().await.unwrap();
//
//         let key = "TEST_KEY";
//
//         // act
//         let val: Option<SomeData> = runtime_settings.get(key, &_make_ctx());
//
//         // assert
//         assert_eq!(
//             val,
//             Some(SomeData {
//                 key: "value".to_string()
//             })
//         );
//     }
//
//     #[tokio::test]
//     async fn test_runtime_settings_get_skip_not_suitable_settings() {
//         let mut settings = HashMap::new();
//         settings.insert(
//             "TEST_KEY".to_string(),
//             vec![
//                 SettingsService::new(Setting {
//                     key: "TEST_KEY".to_string(),
//                     priority: 10,
//                     runtime: "rust".to_string(),
//                     filters: Some(vec![Filter {
//                         name: "application".to_string(),
//                         value: "foo".to_string(),
//                     }]),
//                     value: Some("{\"key\": \"wrong-value\"}".into()),
//                 }),
//                 SettingsService::new(Setting {
//                     key: "TEST_KEY".to_string(),
//                     priority: 0,
//                     runtime: "rust".to_string(),
//                     filters: None,
//                     value: Some("{\"key\": \"right-value\"}".into()),
//                 })
//             ],
//         );
//         let settings_provider = TestSettingsProvider {
//             settings: Mutex::new(settings),
//         };
//         let runtime_settings = RuntimeSettings::new(settings_provider);
//
//         runtime_settings.refresh().await.unwrap();
//
//         let key = "TEST_KEY";
//
//         // act
//         let val: Option<SomeData> = runtime_settings.get(key, &_make_ctx());
//
//         // assert
//         assert_eq!(
//             val,
//             Some(SomeData {
//                 key: "right-value".to_string()
//             })
//         );
//     }
// }
