use std::borrow::BorrowMut;
use std::collections::HashMap;
// use std::hash::Hash;
// use std::iter::Iterator;
use std::sync::RwLock;
//
// use serde::de::DeserializeOwned;
//
// use crate::context::Context;
use crate::filters::SettingsService;
use crate::providers::{DiffSettings, MicroserviceRuntimeSettingsProvider};
// // use crate::providers::RuntimeSettingsProvider;
// // use crate::MicroserviceRuntimeSettingsProvider;
//
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

use async_trait::async_trait;
use std::env::var;
use lazy_static::lazy_static;
use crate::entities::SettingKey;

lazy_static! {
    // static ref RUNTIME_SETTINGS_BASE_URL: String = var("RUNTIME_SETTINGS_BASE_URL").unwrap_or("https://runtime-settings.micro.cian.tech".to_string());
    static ref RUNTIME_SETTINGS_BASE_URL: String = var("RUNTIME_SETTINGS_BASE_URL").unwrap_or("http://master.runtime-settings.dev3.cian.ru".to_string());
}


pub struct RuntimeSettings {
    settings: RwLock<HashMap<String, Vec<SettingsService>>>,
    mcs_settings_provider: Option<Box<dyn DiffSettings>>,
    version: RwLock<String>,
}

// #[async_trait]
impl RuntimeSettings {
    pub fn new() -> Self {
        let mcs_settings_provider = MicroserviceRuntimeSettingsProvider::new(RUNTIME_SETTINGS_BASE_URL.to_string());
        Self {
            settings: RwLock::new(HashMap::new()),
            version: RwLock::from("0".to_string()),
            mcs_settings_provider: Some(Box::new(mcs_settings_provider)),
        }
    }
    pub async fn refresh(&mut self) -> Result<()> {
        println!("Refresh settings ...");
        if let Some(mcs_provider) = &self.mcs_settings_provider {
            let version = {
                let version_guard = &self.version.read().unwrap();
                (*version_guard).clone()
            };
            let new_settings = match mcs_provider.get_settings(&version).await {
                Ok(r) => r,
                Err(err) => {
                    eprintln!("Error: Could not update settings {}", err);
                    return Err(err);
                }
            };
            println!("New Settings {:?}", &new_settings);


            let new_settings_services = new_settings.settings.into_iter().map(|s| SettingsService::new(s)).collect();
            {
                let mut settings_guard = self.settings.write().unwrap();
                let mut ss = settings_guard.borrow_mut();
                merge_settings(ss, new_settings_services);
            }

            {
                let mut version_guard = self.version.write().unwrap();
                *version_guard = new_settings.version;

            }
        }



        // let mut settings_guard = self.settings.write().unwrap();
        // *settings_guard = new_settings;
        // println!("Settings refreshed");
        Ok(())
    }
}

fn delete_settings(settings: &mut HashMap<String, Vec<SettingsService>>, deleted: Vec<SettingKey>) {
    for del in deleted {
        let SettingKey {key, priority} = del;
        settings.entry(key).and_modify(
            |i|
                {
                    if let Ok(idx) = i.binary_search_by(|s| s.setting.priority.cmp(&priority)) {
                        i.remove(idx);
                    }
                }
        );
    }
}

fn merge_settings(settings: &mut HashMap<String, Vec<SettingsService>>, new_settings: Vec<SettingsService>) {
    for new in new_settings {
        let mut entry = settings.entry(new.setting.key.clone()).or_insert_with(|| vec![]);
        let mut insert_pos = entry.len();
        let mut swap = false;
        for (pos, ss) in entry.iter_mut().enumerate() {
            if ss.setting.priority > new.setting.priority {
                insert_pos = pos;
                break
            }
            if ss.setting.priority == new.setting.priority {
                insert_pos = pos;
                swap = true;
                break
            }
        }

        if swap {
            entry[insert_pos] = new;
        } else {
            entry.insert(insert_pos, new);
        }
    };
}

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
#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use serde::Deserialize;
    use async_trait::async_trait;
    use crate::entities::{Filter, Setting};
    use super::*;

    type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

    #[derive(Deserialize, Debug, PartialEq)]
    struct SomeData {
        key: String,
    }

    fn make_ss(key: &str, priority: u32, value: Option<String>) -> SettingsService {
        SettingsService::new(Setting {
            key: key.into(),
            priority,
            value,
            runtime: "rust".into(),
            filter: HashMap::new(),
        })
    }

    #[test]
    fn test_merge_settings_empty_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::new();
        let new_settings = vec![
            make_ss("foo", 10, None),
            make_ss("bar", 0, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([
                ("foo".to_string(), vec![make_ss("foo", 10, None)]),
                ("bar".to_string(), vec![make_ss("bar", 0, None)]),
            ])
        );
    }

    #[test]
    fn test_merge_settings_insert_settings_with_same_name() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::new();
        let new_settings = vec![
            make_ss("foo", 10, None),
            make_ss("foo", 0, None),
            make_ss("foo", 30, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([
                ("foo".to_string(), vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, None),
                    make_ss("foo", 30, None),
                ]),
            ])
        );
    }

    #[test]
    fn test_merge_settings_extends_existed_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::from([
            ("foo".to_string(), vec![make_ss("foo", 10, None)]),
        ]);
        let new_settings = vec![
            make_ss("foo", 0, None),
            make_ss("foo", 30, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([
                ("foo".to_string(), vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, None),
                    make_ss("foo", 30, None),
                ]),
            ])
        );
    }

    #[test]
    fn test_merge_settings_swap_existed_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::from([
            ("foo".to_string(), vec![make_ss("foo", 10, None)]),
        ]);
        let new_settings = vec![
            make_ss("foo", 0, None),
            make_ss("foo", 10, Some("new_value".to_string())),
            make_ss("foo", 30, None),
        ];
        merge_settings(&mut settings, new_settings);

        assert_eq!(
            settings,
            HashMap::from([
                ("foo".to_string(), vec![
                    make_ss("foo", 0, None),
                    make_ss("foo", 10, Some("new_value".to_string())),
                    make_ss("foo", 30, None),
                ]),
            ])
        );
    }

    #[test]
    fn test_delete_settings() {
        let mut settings: HashMap<String, Vec<SettingsService>> = HashMap::from([
            ("foo".to_string(), vec![make_ss("foo", 10, None)]),
        ]);
        delete_settings(
            &mut settings,
            vec![
                SettingKey{
                    key: "foo".to_string(),
                    priority: 10
                },
                SettingKey{
                    key: "bar".to_string(),
                    priority: 0
                }
            ]
        );

        assert_eq!(settings, HashMap::from([("foo".to_string(), vec![])]));
    }
}
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
