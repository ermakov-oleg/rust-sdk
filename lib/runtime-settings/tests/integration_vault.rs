// lib/runtime-settings/tests/integration_vault.rs

use runtime_settings::SecretsService;
use vault_client::VaultClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn mock_vault_client(mock_uri: &str, token: &str) -> VaultClient {
    VaultClient::builder()
        .base_url(mock_uri)
        .token(token)
        .build()
        .await
        .unwrap()
}

/// Helper to create a Vault KV2 response in the expected format.
fn vault_kv2_response(data: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "request_id": "test-request-id",
        "lease_id": "",
        "renewable": false,
        "lease_duration": 0,
        "data": {
            "data": data,
            "metadata": {
                "created_time": "2024-01-01T00:00:00.000000000Z",
                "deletion_time": "",
                "destroyed": false,
                "version": 1,
                "custom_metadata": null
            }
        },
        "wrap_info": null,
        "warnings": null,
        "auth": null
    })
}

#[tokio::test]
async fn test_vault_get_secret() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Mock GET /v1/secret/data/database/credentials
    Mock::given(method("GET"))
        .and(path("/v1/secret/data/database/credentials"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vault_kv2_response(
            serde_json::json!({
                "username": "admin",
                "password": "secret123"
            }),
        )))
        .mount(&mock_server)
        .await;

    // Create VaultClient pointing to mock server
    let client = mock_vault_client(&mock_server.uri(), "test-token").await;

    // Create SecretsService with that client
    let secrets_service = SecretsService::new(client);

    // Verify get("secret/data/database/credentials", "password") returns the value
    let password = secrets_service
        .get("secret/data/database/credentials", "password")
        .await
        .expect("should get password");

    assert_eq!(password, serde_json::json!("secret123"));

    // Also verify username
    let username = secrets_service
        .get("secret/data/database/credentials", "username")
        .await
        .expect("should get username");

    assert_eq!(username, serde_json::json!("admin"));
}

#[tokio::test]
async fn test_vault_secret_caching() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Mock with .expect(1) to ensure only 1 call
    Mock::given(method("GET"))
        .and(path("/v1/secret/data/cached/secret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vault_kv2_response(
            serde_json::json!({
                "api_key": "cached-key-value"
            }),
        )))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token").await;
    let secrets_service = SecretsService::new(client);

    // First call - should hit the mock
    let value1 = secrets_service
        .get("secret/data/cached/secret", "api_key")
        .await
        .expect("should get value on first call");

    assert_eq!(value1, serde_json::json!("cached-key-value"));

    // Second call - should use cache (mock expects only 1 call)
    let value2 = secrets_service
        .get("secret/data/cached/secret", "api_key")
        .await
        .expect("should get value on second call from cache");

    assert_eq!(value2, serde_json::json!("cached-key-value"));

    // Mock server will verify expect(1) on drop
}

#[tokio::test]
async fn test_vault_secret_not_found() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Mock returning 404
    Mock::given(method("GET"))
        .and(path("/v1/secret/data/nonexistent/path"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "errors": ["secret not found"]
        })))
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token").await;
    let secrets_service = SecretsService::new(client);

    // Verify get() returns error
    let result = secrets_service.get("secret/data/nonexistent/path", "key").await;

    assert!(result.is_err(), "should return error for 404");
    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Vault error"),
        "error should be Vault error: {}",
        err_string
    );
}

#[tokio::test]
async fn test_vault_key_not_found_in_secret() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Mock returning secret with different keys
    Mock::given(method("GET"))
        .and(path("/v1/secret/data/app/config"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vault_kv2_response(
            serde_json::json!({
                "existing_key": "some_value",
                "another_key": "another_value"
            }),
        )))
        .mount(&mock_server)
        .await;

    let client = mock_vault_client(&mock_server.uri(), "test-token").await;
    let secrets_service = SecretsService::new(client);

    // Try to get non-existent key
    let result = secrets_service.get("secret/data/app/config", "nonexistent_key").await;

    // Verify get() returns error
    assert!(result.is_err(), "should return error for missing key");
    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Secret key not found"),
        "error should indicate key not found: {}",
        err_string
    );
    assert!(
        err_string.contains("nonexistent_key"),
        "error should contain key name: {}",
        err_string
    );
}

#[tokio::test]
async fn test_secrets_without_vault() {
    // Use SecretsService::new_without_vault()
    let secrets_service = SecretsService::new_without_vault();

    // Verify get() returns error
    let result = secrets_service.get("secret/data/any/path", "any_key").await;

    assert!(result.is_err(), "should return error without vault");
    let err = result.unwrap_err();
    let err_string = err.to_string();
    assert!(
        err_string.contains("Secret used but Vault not configured"),
        "error should indicate vault not configured: {}",
        err_string
    );
}
