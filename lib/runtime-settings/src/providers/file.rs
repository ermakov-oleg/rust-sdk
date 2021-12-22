use std::fs;

use crate::entities::Setting;
use crate::providers::Result;

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
