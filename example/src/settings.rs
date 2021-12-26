use std::collections::HashMap;
use std::error::Error;

use lazy_static::lazy_static;
use tokio::task;
use tokio::time::{sleep, Duration};

use runtime_settings::{Context, RuntimeSettings};

use crate::consts::APPLICATION_NAME;

lazy_static! {
    pub static ref RUNTIME_SETTINGS: RuntimeSettings = RuntimeSettings::new();
}

pub async fn setup() {
    RUNTIME_SETTINGS.init().await;
    RUNTIME_SETTINGS.refresh().await.unwrap();

    task::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            let _ = RUNTIME_SETTINGS
                .refresh()
                .await
                .or_else::<Box<dyn Error>, _>(|e| {
                    tracing::error!("Error when update RS {}", e);
                    Ok(())
                });
        }
    });
}

pub fn get_context() -> Context {
    Context {
        application: APPLICATION_NAME.to_string(),
        server: "test-server".into(),
        environment: HashMap::from([("TEST".to_string(), "ermakov".to_string())]),
        host: None,
        url: None,
        url_path: None,
        email: None,
        ip: None,
        context: Default::default(),
    }
}
