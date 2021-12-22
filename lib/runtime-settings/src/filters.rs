use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use regex::Regex;

use crate::context::Context;
use crate::entities::Setting;

pub trait RSFilter: Send + Sync {
    fn check(&self, ctx: &Context) -> bool;
    fn is_static(&self) -> bool;
}

enum Filter {
    Runtime(String),
    Application(Pattern),
    Server(Pattern),
    Host(Pattern),
    Url(Pattern),
    UrlPath(Pattern),
    Email(Pattern),
    Ip(Pattern),
    Environment(MapPattern),
    Context(MapPattern),
    // Todo: LibraryVersion
    // Todo: McsRunEnv
    Noop,
}

#[derive(Debug)]
struct Pattern {
    pattern: Regex,
}

impl Pattern {
    fn new(pattern: String) -> Self {
        Self {
            pattern: Regex::new((&format!("^{}$", pattern)).as_ref()).unwrap(),
        }
    }

    fn is_match(&self, val: Option<&str>) -> bool {
        match val {
            Some(val) => self.pattern.is_match(val),
            None => false,
        }
    }
}

#[derive(Debug)]
struct MapPattern {
    patterns: Vec<(String, Regex)>,
}

impl MapPattern {
    fn new(patterns_raw: Vec<&str>) -> Self {
        let mut patterns = vec![];
        for pattern_raw in patterns_raw {
            let items: Vec<&str> = pattern_raw.splitn(2, '=').collect();
            let pattern = match items[..] {
                [key, val] => (
                    key.into(),
                    Regex::new((&format!("^{}$", val)).as_ref()).unwrap(),
                ),
                _ => unimplemented!(), // todo: error
            };
            patterns.push(pattern);
        }

        Self { patterns }
    }

    fn is_match(&self, value: &HashMap<String, String>) -> bool {
        let mut result = true;
        for (key, pattern) in &self.patterns {
            match value.get(key.as_str()) {
                Some(val) => {
                    if !pattern.is_match(val) {
                        result = false;
                        break;
                    }
                }
                None => {
                    result = false;
                    break;
                }
            }
        }

        result
    }
}

impl RSFilter for Filter {
    fn check(&self, ctx: &Context) -> bool {
        match self {
            Filter::Runtime(p) => p.eq("rust"),
            Filter::Application(p) => p.is_match(Some(&*ctx.application)),
            Filter::Server(p) => p.is_match(Some(&*ctx.server)),
            Filter::Host(p) => p.is_match(ctx.host.as_deref()),
            Filter::Url(p) => p.is_match(ctx.url.as_deref()),
            Filter::UrlPath(p) => p.is_match(ctx.url_path.as_deref()),
            Filter::Email(p) => p.is_match(ctx.email.as_deref()),
            Filter::Ip(p) => p.is_match(ctx.ip.as_deref()),
            Filter::Environment(p) => p.is_match(&ctx.environment),
            Filter::Context(p) => p.is_match(&ctx.context),
            Filter::Noop => false,
        }
    }

    fn is_static(&self) -> bool {
        matches!(
            self,
            Filter::Noop
                | Filter::Runtime(_)
                | Filter::Host(_)
                | Filter::Application(_)
                | Filter::Server(_)
                | Filter::Environment(_)
        )
    }
}

// todo: move to try_from
impl From<(String, String)> for Filter {
    fn from((key, value): (String, String)) -> Self {
        match key.as_str() {
            "application" => Filter::Application(Pattern::new(value)),
            "server" => Filter::Server(Pattern::new(value)),
            "host" => Filter::Host(Pattern::new(value)),
            "url" => Filter::Url(Pattern::new(value)),
            "url_path" => Filter::UrlPath(Pattern::new(value)),
            "email" => Filter::Email(Pattern::new(value)),
            "ip" => Filter::Ip(Pattern::new(value)),
            "environment" => Filter::Environment(MapPattern::new(value.split(';').collect())),
            "context" => Filter::Context(MapPattern::new(value.split(';').collect())),
            _ => Filter::Noop,
        }
    }
}

pub struct SettingsService {
    pub setting: Box<Setting>,
    filters: Vec<Box<dyn RSFilter>>,
}

impl SettingsService {
    pub fn new(setting: Setting) -> Self {
        let mut filters: Vec<Box<dyn RSFilter>> = vec![];
        filters.push(Box::new(Filter::Runtime(setting.runtime.clone())));
        for item in setting.filter.clone() {
            filters.push(Box::new(Filter::from(item)));
        }

        SettingsService {
            setting: Box::new(setting),
            filters,
        }
    }

    pub fn is_suitable(&self, ctx: &Context) -> bool {
        (&self.filters).iter().all(|filter| filter.check(ctx))
    }
}

impl Debug for SettingsService {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SettingsService")
            .field("setting", &self.setting)
            .finish()
    }
}

impl PartialEq for SettingsService {
    fn eq(&self, other: &Self) -> bool {
        self.setting == other.setting
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

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

    fn _make_setting() -> Setting {
        Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filter: HashMap::new(),
            value: Some("foo".to_string()),
        }
    }

    #[test]
    fn test_settings_service_is_suitable_without_filters() {
        // arrange
        let setting = _make_setting();
        let ss = SettingsService::new(setting);

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_with_unknown_filter() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("unknown_filter".to_string(), "test".to_string())]);
        let ss = SettingsService::new(setting);

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_filter_for_another_application() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("application".to_string(), "some-mcs".to_string())]);
        let ss = SettingsService::new(setting);

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_application_filter() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("application".to_string(), "test-rust".to_string())]);
        let ss = SettingsService::new(setting);

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_url_filter() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("url".to_string(), "some-url".to_string())]);
        let ss = SettingsService::new(setting);

        let mut ctx = _make_ctx();
        ctx.url = Some("some-url".to_string());

        // act && assert
        assert!(ss.is_suitable(&ctx));
    }

    #[test]
    fn test_settings_service_is_not_suitable_env_filter_filter_env_not_exist() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("environment".to_string(), "SOME_ENV=123".to_string())]);
        let ss = SettingsService::new(setting);

        let mut ctx = _make_ctx();
        ctx.environment = HashMap::from([("FOO".into(), "BAR".into())]);

        // act && assert
        assert!(!ss.is_suitable(&ctx));
    }

    #[test]
    fn test_settings_service_is_not_suitable_env_filter_filter_env_exist_and_not_eq() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("environment".to_string(), "FOO=BAZ".to_string())]);
        let ss = SettingsService::new(setting);

        let mut ctx = _make_ctx();
        ctx.environment = HashMap::from([("FOO".into(), "BAR".into())]);

        // act && assert
        assert!(!ss.is_suitable(&ctx));
    }

    #[test]
    fn test_settings_service_is_not_suitable_env_filter_only_one_filter_env_exist_and_eq() {
        // arrange
        let mut setting = _make_setting();
        setting.filter =
            HashMap::from([("environment".to_string(), "FOO=BAR;BAZ=QUUX".to_string())]);
        let ss = SettingsService::new(setting);

        let mut ctx = _make_ctx();
        ctx.environment = HashMap::from([("FOO".into(), "BAR".into())]);

        // act && assert
        assert!(!ss.is_suitable(&ctx));
    }

    #[test]
    fn test_settings_service_is_suitable_env_filter_filter_env_exist_and_eq() {
        // arrange
        let mut setting = _make_setting();
        setting.filter = HashMap::from([("environment".to_string(), "FOO=BAR".to_string())]);
        let ss = SettingsService::new(setting);

        let mut ctx = _make_ctx();
        ctx.environment = HashMap::from([("FOO".into(), "BAR".into())]);

        // act && assert
        assert!(ss.is_suitable(&ctx));
    }
}
