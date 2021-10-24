use std::borrow::Borrow;
use std::collections::HashMap;
use std::fs;


use serde::Deserialize;


//
// pub fn get_settings<'a>() -> RuntimeSettings<'a> {
//     let raw: RuntimeSettingsFilesRaw = read_settings();
//     raw.into()
// }

#[derive(Deserialize, Debug, Clone)]
pub struct Setting {
    key: String,
    priority: u32,
    runtime: String,
    filter: HashMap<String, String>,
    value: String,
}

#[derive(Deserialize, Debug)]
pub struct RuntimeSettingsFilesRaw {
    settings: Vec<Setting>,
}

fn read_settings<T>() -> T
where
    T: serde::de::DeserializeOwned,
{
    // let contents = fs::read_to_string("./settings.json")
    let contents = fs::read_to_string("./settings-simple.json").expect("File error");
    println!("{}", contents);
    let result = serde_json::from_str(contents.as_str()).expect("Fail parse");

    result
}
