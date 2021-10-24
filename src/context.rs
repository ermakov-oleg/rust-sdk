use std::collections::HashMap;

pub struct Context {
    pub application: String,
    pub server: String,
    pub host: Option<String>,
    pub url: Option<String>,
    pub url_path: Option<String>,
    pub email: Option<String>,
    pub ip: Option<String>,
    pub environment: HashMap<String, String>,
    pub context: HashMap<String, String>,
}
