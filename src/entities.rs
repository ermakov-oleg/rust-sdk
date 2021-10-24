use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
pub struct Filter {
    pub name: String,
    pub value: String,
}

#[derive(Deserialize, Debug)]
pub struct Setting {
    pub key: String,
    pub priority: u32,
    pub runtime: String,
    pub filter: HashMap<String, String>,
    pub value: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SettingKey {
    key: String,
    priority: u32,
}

#[derive(Deserialize, Debug)]
pub struct RuntimeSettingsResponse {
    pub settings: Vec<Setting>,
    deleted: Vec<SettingKey>,
    pub(crate) version: String,
}
