use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct Filter {
    pub name: String,
    pub value: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Setting {
    pub key: String,
    pub priority: u32,
    pub runtime: String,
    pub filter: HashMap<String, String>,
    pub value: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct SettingKey {
    pub(crate) key: String,
    pub(crate) priority: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub struct RuntimeSettingsResponse {
    pub(crate) settings: Vec<Setting>,
    pub(crate) deleted: Vec<SettingKey>,
    pub(crate) version: String,
}
