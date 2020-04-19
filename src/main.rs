#![warn(rust_2018_idioms)]

use std::borrow::Borrow;
use std::cmp::Reverse;
use std::hash::Hash;
use std::iter::Iterator;

use async_trait::async_trait;
use bytes::buf::BufExt;
use hyper::{body::HttpBody as _, Client};
use regex::Regex;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::from_str;
use std::collections::HashMap;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[derive(Deserialize, Debug)]
struct Filter {
    name: String,
    value: String,
}

#[derive(Deserialize, Debug)]
struct Setting {
    key: String,
    priority: u32,
    runtime: String,
    filters: Option<Vec<Filter>>,
    value: Option<String>,
}

#[derive(Deserialize, Debug)]
struct SettingKey {
    key: String,
    priority: u32,
}

#[derive(Deserialize, Debug)]
struct RuntimeSettingsResponse {
    settings: Vec<Box<Setting>>,
    deleted: Vec<Box<SettingKey>>,
    version: String,
}

struct Context {
    application: String,
    server: String,
    environment: HashMap<String, String>,
    host: Option<String>,
    url: Option<String>,
    url_path: Option<String>,
    email: Option<String>,
    ip: Option<String>,
    context: HashMap<String, String>,
}

trait RSFilter {
    fn check(&self, ctx: &Context) -> bool {
        false
    }
}

enum RSFilterImpl {
    Filter(Box<dyn RSFilter>),
}

struct PatternRSFilter {
    ctx_attr: String,
    pattern: Regex,
}

// todo: HashMap filter for envs
// todo: PatternHashMap filter for context

struct DummyRSFilter {}

impl RSFilter for DummyRSFilter {}

impl PatternRSFilter {
    fn new(ctx_attr: String, pattern: String) -> Self {
        PatternRSFilter {
            ctx_attr,
            pattern: Regex::new(&format!("^{}$", pattern)).unwrap(),
        }
    }
}

impl RSFilter for PatternRSFilter {
    fn check(&self, ctx: &Context) -> bool {
        match self.ctx_attr.as_str() {
            "application" => self.pattern.is_match(ctx.application.as_str()),
            "server" => self.pattern.is_match(ctx.server.as_str()),
            "host" => ctx
                .host
                .as_ref()
                .map_or(false, |v| self.pattern.is_match(v.as_str())),
            "url" => ctx
                .url
                .as_ref()
                .map_or(false, |v| self.pattern.is_match(v.as_str())),
            "url_path" => ctx
                .url_path
                .as_ref()
                .map_or(false, |v| self.pattern.is_match(v.as_str())),
            "email" => ctx
                .email
                .as_ref()
                .map_or(false, |v| self.pattern.is_match(v.as_str())),
            "ip" => ctx
                .ip
                .as_ref()
                .map_or(false, |v| self.pattern.is_match(v.as_str())),
            _ => {
                eprintln!("Invalid ctx_attr: {}", &self.ctx_attr);
                false
            }
        }
    }
}

fn make_rs_filter(f: &Filter) -> RSFilterImpl {
    fn _mk_pattern_flt(ctx_attr: &str, pattern: String) -> RSFilterImpl {
        RSFilterImpl::Filter(Box::new(PatternRSFilter::new(
            ctx_attr.into(),
            pattern.into(),
        )))
    }

    match f.name.as_ref() {
        "application" => _mk_pattern_flt("application", f.value.clone()),
        "server" => _mk_pattern_flt("server", f.value.clone()),
        "url" => _mk_pattern_flt("url", f.value.clone()),
        "url_path" => _mk_pattern_flt("url_path", f.value.clone()),
        "email" => _mk_pattern_flt("email", f.value.clone()),
        "ip" => _mk_pattern_flt("ip", f.value.clone()),
        _ => RSFilterImpl::Filter(Box::new(DummyRSFilter {})),
    }
}

struct SettingsService {
    setting: Box<Setting>,
    filters: Vec<RSFilterImpl>,
}

impl SettingsService {
    fn new(setting: Box<Setting>) -> Self {
        let filters = match &setting.filters {
            Some(f) => f.into_iter().map(make_rs_filter).collect(),
            None => vec![],
        };

        SettingsService { setting, filters }
    }

    fn is_suitable(&self, ctx: &Context) -> bool {
        (&self.filters).into_iter().all(|filter| {
            let RSFilterImpl::Filter(flt) = filter;
            flt.check(ctx)
        })
    }
}

fn prepare_settings(settings: Vec<Box<Setting>>) -> HashMap<String, Vec<Box<SettingsService>>> {
    let mut settings_dict = HashMap::new();
    for s in settings {
        let key = s.key.clone();
        let ss = SettingsService::new(s);

        settings_dict
            .entry(key.into())
            .or_insert_with(Vec::new)
            .push(Box::new(ss));
    }
    settings_dict
        .values_mut()
        .for_each(|data| data.sort_by_key(|ss| Reverse(ss.setting.priority)));
    settings_dict
}

struct RuntimeSettings {
    settings: HashMap<String, Vec<Box<SettingsService>>>,
    settings_provider: Box<dyn RuntimeSettingsProvider>,
}

#[async_trait]
trait RuntimeSettingsProvider {
    async fn get_settings(&self) -> Result<HashMap<String, Vec<Box<SettingsService>>>>;
}

struct MicroserviceRuntimeSettingsProvider {
    base_url: String,
}

impl MicroserviceRuntimeSettingsProvider {
    fn new(base_url: String) -> Self {
        MicroserviceRuntimeSettingsProvider { base_url }
    }
}

#[async_trait]
impl RuntimeSettingsProvider for MicroserviceRuntimeSettingsProvider {
    async fn get_settings(&self) -> Result<HashMap<String, Vec<Box<SettingsService>>>> {
        let url = format!(
            "{}/v2/get-runtime-settings/?runtime=python&version=0",
            self.base_url
        )
        .parse()
        .unwrap();
        println!("Get runtime settings");
        let rs_response: RuntimeSettingsResponse = fetch_json(url).await?;

        println!("Settings: {:#?}", rs_response);

        let settings = prepare_settings(rs_response.settings);

        Ok(settings)
    }
}

impl RuntimeSettings {
    fn new<T: RuntimeSettingsProvider + 'static>(settings_provider: T) -> Self {
        RuntimeSettings {
            settings: HashMap::new(),
            settings_provider: Box::new(settings_provider),
        }
    }

    async fn refresh(&mut self) -> Result<()> {
        let new_settings = match self.settings_provider.get_settings().await {
            Ok(r) => r,
            Err(err) => {
                eprintln!("Error: Could not update settings {}", err);
                return Ok(());
            }
        };
        self.settings = new_settings;
        println!("Settings refreshed");
        Ok(())
    }

    fn get<K: ?Sized, V>(&self, key: &K, ctx: &Context) -> Option<V>
    where
        String: Borrow<K>,
        K: Hash + Eq,
        V: DeserializeOwned,
    {
        let value = match self.settings.get(key) {
            Some(vss) => vss
                .into_iter()
                .find(|f| f.is_suitable(ctx))
                .and_then(|val| val.setting.value.clone()),
            None => None,
        };

        value.map_or(None, |v| {
            serde_json::from_str(&v)
                .map_err(|err| {
                    eprintln!("Error when deserialize value {}", err);
                })
                .ok()
        })
    }
}

// A simple type alias so as to DRY.

#[derive(Debug, Deserialize)]
struct PGConnectionString {
    user: String,
    password: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let settings_provider = MicroserviceRuntimeSettingsProvider::new(
        "http://master.runtime-settings.dev3.cian.ru".into(),
    );
    let mut runtime_settings = RuntimeSettings::new(settings_provider);

    runtime_settings.refresh().await?;

    let ctx = Context {
        application: "test-rust".into(),
        server: "test-server".into(),
        environment: HashMap::new(),
        host: None,
        url: None,
        url_path: None,
        email: None,
        ip: None,
        context: Default::default(),
    };

    let key = "postgres_connection/qa_tests_manager";
    let key = "isNewPublishTerms.Enabled";

    // let val: Option<PGConnectionString> = runtime_settings.get(key, &ctx);
    let val: Option<String> = runtime_settings.get(key, &ctx);

    println!("Settings {}:{:#?}", key, val);

    Ok(())
}

async fn fetch_json<T>(url: hyper::Uri) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let client = Client::new();

    // Fetch the url...
    let res = client.get(url).await?;

    // asynchronously aggregate the chunks of the body
    let body = hyper::body::aggregate(res).await?;

    // try to parse as json with serde_json
    let result = serde_json::from_reader(body.reader())?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    fn _make_ctx() -> Context {
        Context {
            application: "test-rust".to_string(),
            server: "test-server".to_string(),
            environment: Default::default(),
            host: None,
            url: None,
            url_path: None,
            email: None,
            ip: None,
            context: Default::default(),
        }
    }

    #[test]
    fn test_settings_service_is_suitable_without_filters() {
        // arrange
        let ss = SettingsService::new(Box::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: None,
            value: Some("foo".to_string()),
        }));

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_with_unknown_filter() {
        // arrange
        let ss = SettingsService::new(Box::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "unknown_filter".to_string(),
                value: "test".to_string(),
            }]),
            value: Some("foo".to_string()),
        }));

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_filter_for_another_application() {
        // arrange
        let ss = SettingsService::new(Box::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "application".to_string(),
                value: "python-mcs-test".to_string(),
            }]),
            value: Some("foo".to_string()),
        }));

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_application_filter() {
        // arrange
        let ss = SettingsService::new(Box::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "application".to_string(),
                value: "test-rust".to_string(),
            }]),
            value: Some("foo".to_string()),
        }));

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_url_filter() {
        // arrange
        let ss = SettingsService::new(Box::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "url".to_string(),
                value: "some-url".to_string(),
            }]),
            value: Some("foo".to_string()),
        }));

        let mut ctx = _make_ctx();
        ctx.url = Some("some-url".to_string());

        // act && assert
        assert!(ss.is_suitable(&ctx));
    }

    #[test]
    fn test_prepare_settings_expected_settings_sort_order() {
        // arrange
        let raw_settings = vec![
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 100,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 0,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
            Box::new(Setting {
                key: "foo".to_string(),
                priority: 110,
                runtime: "rust".to_string(),
                filters: None,
                value: None,
            }),
        ];

        // act
        let settings = prepare_settings(raw_settings);

        // assert
        assert_eq!(
            settings["foo"]
                .iter()
                .map(|s| s.setting.priority)
                .collect::<Vec<u32>>(),
            [110, 100, 0]
        );
    }
}
