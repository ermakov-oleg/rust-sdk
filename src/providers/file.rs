use std::collections::HashMap;
use std::fs;
use std::path::Path;

use log::error;
use serde::Deserialize;

use crate::entities::Setting;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct FileProvider {
    path: String,
}

impl FileProvider {
    pub fn new(path: String) -> FileProvider {
        FileProvider { path }
    }

    pub fn read_settings(&self) -> Result<Vec<Setting>> {
        let contents = fs::read_to_string(&self.path)?;
        let result: Vec<Setting> = serde_json::from_str(contents.as_str())?;
        Ok(result)
    }
}


