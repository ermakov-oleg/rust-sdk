use std::collections::HashMap;

use runtime_settings::Context;

use crate::consts::APPLICATION_NAME;

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
