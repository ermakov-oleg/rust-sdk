use serde::Deserialize;

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
    pub filters: Option<Vec<Filter>>,
    pub value: Option<String>,
}
