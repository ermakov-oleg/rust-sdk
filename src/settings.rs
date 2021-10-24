// use crate::{Context, RuntimeSettings};
// use serde::de::DeserializeOwned;
// use std::borrow::Borrow;
// use std::hash::Hash;
// use std::sync::Arc;
// use std::time::Duration;
// use tokio::io;
// use tokio::task;
// use tokio::time::delay_for;
//
// pub struct Settings {
//     runtime_settings: Arc<RuntimeSettings>,
// }
//
// impl Settings {
//     pub async fn new(rs_base_url: String) -> Self {
//         Self {
//             runtime_settings: Self::init_runtime_settings(rs_base_url).await,
//         }
//     }
//
//     async fn init_runtime_settings(base_url: String) -> Arc<RuntimeSettings> {
//
//         let runtime_settings = RuntimeSettings::new(base_url.as_str());
//
//         // runtime_settings.refresh().await.unwrap(); // todo: fallback
//
//         let settings = Arc::new(runtime_settings);
//
//         let settings_p = Arc::clone(&settings);
//
//         task::spawn(async move {
//             loop {
//                 delay_for(Duration::from_secs(10)).await;
//                 println!("Update RS started");
//                 // let _ = settings_p.refresh().await.or_else::<io::Error, _>(|e| {
//                 //     println!("Error when update RS {}", e);
//                 //     Ok(())
//                 // });
//             }
//         });
//
//         settings
//     }
//
//     pub fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
//     where
//         String: Borrow<K>,
//         K: Hash + Eq,
//         V: DeserializeOwned,
//     {
//         // todo: add layers
//         self.runtime_settings.get(key, ctx)
//     }
// }
