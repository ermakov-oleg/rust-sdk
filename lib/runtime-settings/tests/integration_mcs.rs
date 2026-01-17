// lib/runtime-settings/tests/integration_mcs.rs

use runtime_settings::providers::{McsProvider, SettingsProvider};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_mcs_provider_loads_settings() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "settings": [
            {
                "key": "TEST_KEY",
                "priority": 100,
                "filter": {"application": "test-app"},
                "value": "test-value"
            },
            {
                "key": "ANOTHER_KEY",
                "priority": 50,
                "filter": {},
                "value": 42
            }
        ],
        "deleted": [],
        "version": "42"
    });

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .and(query_param("runtime", "rust"))
        .and(query_param("version", "0"))
        .and(query_param("application", "test-app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let result = provider.load("0").await.unwrap();

    assert_eq!(result.version, "42");
    assert_eq!(result.settings.len(), 2);
    assert!(result.deleted.is_empty());

    let first_setting = &result.settings[0];
    assert_eq!(first_setting.key, "TEST_KEY");
    assert_eq!(first_setting.priority, 100);
    assert_eq!(first_setting.value, serde_json::json!("test-value"));

    let second_setting = &result.settings[1];
    assert_eq!(second_setting.key, "ANOTHER_KEY");
    assert_eq!(second_setting.priority, 50);
    assert_eq!(second_setting.value, serde_json::json!(42));
}

#[tokio::test]
async fn test_mcs_provider_handles_deleted_keys() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "settings": [],
        "deleted": [
            {
                "key": "DELETED_KEY",
                "priority": 100
            }
        ],
        "version": "10"
    });

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .and(query_param("runtime", "rust"))
        .and(query_param("application", "test-app"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let result = provider.load("5").await.unwrap();

    assert_eq!(result.version, "10");
    assert!(result.settings.is_empty());
    assert_eq!(result.deleted.len(), 1);

    let deleted_key = &result.deleted[0];
    assert_eq!(deleted_key.key, "DELETED_KEY");
    assert_eq!(deleted_key.priority, 100);
}

#[tokio::test]
async fn test_mcs_provider_includes_mcs_run_env() {
    let mock_server = MockServer::start().await;

    let response_body = serde_json::json!({
        "settings": [],
        "deleted": [],
        "version": "1"
    });

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .and(query_param("runtime", "rust"))
        .and(query_param("application", "test-app"))
        .and(query_param("mcs_run_env", "PROD"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(
        mock_server.uri(),
        "test-app".to_string(),
        Some("PROD".to_string()),
    );
    let result = provider.load("0").await.unwrap();

    assert_eq!(result.version, "1");
}

#[tokio::test]
async fn test_mcs_provider_handles_error_response() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/v3/get-runtime-settings/"))
        .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let provider = McsProvider::new(mock_server.uri(), "test-app".to_string(), None);
    let result = provider.load("0").await;

    assert!(result.is_err());
    let error = result.unwrap_err();
    let error_string = error.to_string();
    assert!(
        error_string.contains("500"),
        "Error should contain status code 500: {}",
        error_string
    );
}
