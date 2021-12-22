use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use tokio::task;
use tokio::time::{sleep, Duration};

use runtime_settings::{Context, RuntimeSettings};

use crate::consts::APPLICATION_NAME;

pub async fn setup() -> Arc<RuntimeSettings> {
    let runtime_settings = RuntimeSettings::new();
    runtime_settings.init().await;
    runtime_settings.refresh().await.unwrap();

    let settings = Arc::new(runtime_settings);
    let settings_p = Arc::clone(&settings);

    task::spawn(async move {
        loop {
            sleep(Duration::from_secs(10)).await;
            let _ = settings_p
                .refresh()
                .await
                .or_else::<Box<dyn Error>, _>(|e| {
                    tracing::error!("Error when update RS {}", e);
                    Ok(())
                });
        }
    });

    settings
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
