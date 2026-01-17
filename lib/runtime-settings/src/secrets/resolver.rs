// lib/runtime-settings/src/secrets/resolver.rs

use super::SecretsService;
use crate::error::SettingsError;
use std::future::Future;
use std::pin::Pin;

/// Recursively resolve {"$secret": "path:key"} references in a JSON value
pub fn resolve_secrets<'a>(
    value: &'a serde_json::Value,
    secrets: &'a SecretsService,
) -> Pin<Box<dyn Future<Output = Result<serde_json::Value, SettingsError>> + Send + 'a>> {
    Box::pin(async move {
        match value {
            serde_json::Value::Object(map) => {
                // Check if this is a secret reference
                if map.len() == 1 {
                    if let Some(serde_json::Value::String(reference)) = map.get("$secret") {
                        return resolve_secret_reference(reference, secrets).await;
                    }
                }

                // Recursively resolve nested objects
                let mut result = serde_json::Map::new();
                for (k, v) in map {
                    result.insert(k.clone(), resolve_secrets(v, secrets).await?);
                }
                Ok(serde_json::Value::Object(result))
            }
            serde_json::Value::Array(arr) => {
                let mut result = Vec::new();
                for item in arr {
                    result.push(resolve_secrets(item, secrets).await?);
                }
                Ok(serde_json::Value::Array(result))
            }
            _ => Ok(value.clone()),
        }
    })
}

/// Resolve a single secret reference like "path/to/secret:key"
async fn resolve_secret_reference(
    reference: &str,
    secrets: &SecretsService,
) -> Result<serde_json::Value, SettingsError> {
    let parts: Vec<&str> = reference.splitn(2, ':').collect();
    if parts.len() != 2 {
        return Err(SettingsError::InvalidSecretReference {
            reference: reference.to_string(),
        });
    }

    let path = parts[0];
    let key = parts[1];

    secrets.get(path, key).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_resolve_no_secrets() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"host": "localhost", "port": 5432});

        let resolved = resolve_secrets(&value, &secrets).await.unwrap();
        assert_eq!(resolved, value);
    }

    #[tokio::test]
    async fn test_resolve_secret_without_vault() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"password": {"$secret": "db/creds:password"}});

        let result = resolve_secrets(&value, &secrets).await;
        assert!(matches!(result, Err(SettingsError::SecretWithoutVault)));
    }

    #[tokio::test]
    async fn test_invalid_secret_reference() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"password": {"$secret": "invalid-no-colon"}});

        let result = resolve_secrets(&value, &secrets).await;
        assert!(matches!(
            result,
            Err(SettingsError::InvalidSecretReference { .. })
        ));
    }
}
