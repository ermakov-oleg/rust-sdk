#![allow(non_upper_case_globals)]
use std::error::Error;

use lazy_static::lazy_static;
use tokio::task;
use tokio::time::{sleep, Duration};

use crate::rs::RuntimeSettings;

lazy_static! {
    pub static ref settings: RuntimeSettings = RuntimeSettings::new();
}

pub async fn setup() {
    settings.init().await;
    settings.refresh().await.unwrap();

    task::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            let _ = settings.refresh().await.or_else::<Box<dyn Error>, _>(|e| {
                tracing::error!("Error when update RS {}", e);
                Ok(())
            });
        }
    });
}
