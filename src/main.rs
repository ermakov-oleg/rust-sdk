#![warn(rust_2018_idioms)]






use cian_settings::{test};





type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
//
// #[derive(Debug, Deserialize)]
// struct PGConnectionString {
//     user: String,
//     password: String,
// }
//
//
// fn read_settings<T>() -> Result<T>
//     where
//         T: serde::de::DeserializeOwned,
//
// {
//     // let contents = fs::read_to_string("./settings.json")
//     let contents = fs::read_to_string("./settings-simple.json")
//         .expect("Something went wrong reading the file");
//
//     // println!("With text:\n{}", contents);
//
//     let result = serde_json::from_str(contents.as_str())?;
//
//     Ok(result)
//
// }

#[tokio::main]
async fn main() -> Result<()> {
    test();

    // println!("With text:\n{:?}", data);

    return Ok(());

    // let settings = Settings::new("http://master.runtime-settings.dev3.cian.ru/".into()).await;
    //
    // let ctx = Context {
    //     application: "test-rust".into(),
    //     server: "test-server".into(),
    //     environment: HashMap::new(),
    //     host: None,
    //     url: None,
    //     url_path: None,
    //     email: None,
    //     ip: None,
    //     context: Default::default(),
    // };
    //
    // let key = "postgres_connection/qa_tests_manager";
    // // let key = "CALLTRACKING_CORE_TIMEOUT";
    //
    // // let val: Option<u32> = settings.get(key, &ctx);
    // let val: Option<PGConnectionString> = settings.get(key, &ctx);
    //
    // println!("Settings {}:{:#?}", key, val);
    //
    // delay_for(Duration::from_secs(60)).await;
    // Ok(())
}
