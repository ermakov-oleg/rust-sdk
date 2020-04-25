use crate::context::Context;
use crate::entities::{Filter, Setting};
use regex::Regex;

pub trait RSFilter {
    fn check(&self, _ctx: &Context) -> bool {
        false
    }
}

pub enum RSFilterImpl {
    Filter(Box<dyn RSFilter + Sync + Send>),
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
            pattern: Regex::new((&format!("^{}$", pattern)).as_ref()).unwrap(),
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
        RSFilterImpl::Filter(Box::new(PatternRSFilter::new(ctx_attr.into(), pattern)))
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

pub struct SettingsService {
    pub setting: Box<Setting>,
    filters: Vec<RSFilterImpl>,
}

impl SettingsService {
    pub fn new(setting: Setting) -> Self {
        let filters = match &setting.filters {
            Some(f) => f.iter().map(make_rs_filter).collect(),
            None => vec![],
        };

        SettingsService {
            setting: Box::new(setting),
            filters,
        }
    }

    pub fn is_suitable(&self, ctx: &Context) -> bool {
        (&self.filters).iter().all(|filter| {
            let RSFilterImpl::Filter(flt) = filter;
            flt.check(ctx)
        })
    }
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
        let ss = SettingsService::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: None,
            value: Some("foo".to_string()),
        });

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_with_unknown_filter() {
        // arrange
        let ss = SettingsService::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "unknown_filter".to_string(),
                value: "test".to_string(),
            }]),
            value: Some("foo".to_string()),
        });

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_not_suitable_filter_for_another_application() {
        // arrange
        let ss = SettingsService::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "application".to_string(),
                value: "python-mcs-test".to_string(),
            }]),
            value: Some("foo".to_string()),
        });

        // act && assert
        assert!(!ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_application_filter() {
        // arrange
        let ss = SettingsService::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "application".to_string(),
                value: "test-rust".to_string(),
            }]),
            value: Some("foo".to_string()),
        });

        // act && assert
        assert!(ss.is_suitable(&_make_ctx()));
    }

    #[test]
    fn test_settings_service_is_suitable_url_filter() {
        // arrange
        let ss = SettingsService::new(Setting {
            key: "TEST_KEY".to_string(),
            priority: 0,
            runtime: "rust".to_string(),
            filters: Some(vec![Filter {
                name: "url".to_string(),
                value: "some-url".to_string(),
            }]),
            value: Some("foo".to_string()),
        });

        let mut ctx = _make_ctx();
        ctx.url = Some("some-url".to_string());

        // act && assert
        assert!(ss.is_suitable(&ctx));
    }
}
