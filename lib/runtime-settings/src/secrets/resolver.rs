// lib/runtime-settings/src/secrets/resolver.rs

use super::{JsonPathKey, SecretUsage, SecretsService};
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

/// Synchronously resolve secrets using pre-parsed SecretUsage list
///
/// This is more efficient than recursive resolution because paths are pre-computed.
pub fn resolve_secrets_sync(
    value: &serde_json::Value,
    usages: &[SecretUsage],
    secrets: &SecretsService,
) -> Result<serde_json::Value, SettingsError> {
    if usages.is_empty() {
        return Ok(value.clone());
    }

    let mut result = value.clone();

    for usage in usages {
        let secret_value = secrets.get_sync(&usage.path, &usage.key)?;
        set_at_path(&mut result, &usage.value_path, secret_value)?;
    }

    Ok(result)
}

/// Set value at the given JSON path
fn set_at_path(
    root: &mut serde_json::Value,
    path: &[JsonPathKey],
    value: serde_json::Value,
) -> Result<(), SettingsError> {
    if path.is_empty() {
        *root = value;
        return Ok(());
    }

    let mut current = root;

    // Navigate to the parent of the target
    for key in &path[..path.len() - 1] {
        current = match key {
            JsonPathKey::Field(f) => current
                .get_mut(f)
                .ok_or(SettingsError::InvalidSecretPath)?,
            JsonPathKey::Index(i) => current
                .get_mut(i)
                .ok_or(SettingsError::InvalidSecretPath)?,
        };
    }

    // Set the value at the final key
    match path.last().unwrap() {
        JsonPathKey::Field(f) => {
            current
                .as_object_mut()
                .ok_or(SettingsError::InvalidSecretPath)?
                .insert(f.clone(), value);
        }
        JsonPathKey::Index(i) => {
            let arr = current
                .as_array_mut()
                .ok_or(SettingsError::InvalidSecretPath)?;
            if *i < arr.len() {
                arr[*i] = value;
            } else {
                return Err(SettingsError::InvalidSecretPath);
            }
        }
    }

    Ok(())
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

    #[test]
    fn test_set_at_path_root() {
        let mut root = serde_json::json!({"$secret": "path:key"});
        set_at_path(&mut root, &[], serde_json::json!("secret_value")).unwrap();
        assert_eq!(root, serde_json::json!("secret_value"));
    }

    #[test]
    fn test_set_at_path_nested_field() {
        let mut root = serde_json::json!({
            "database": {
                "password": {"$secret": "path:key"}
            }
        });
        let path = vec![
            JsonPathKey::Field("database".to_string()),
            JsonPathKey::Field("password".to_string()),
        ];
        set_at_path(&mut root, &path, serde_json::json!("secret123")).unwrap();
        assert_eq!(root["database"]["password"], serde_json::json!("secret123"));
    }

    #[test]
    fn test_set_at_path_array_index() {
        let mut root = serde_json::json!({
            "servers": [
                {"password": {"$secret": "path:key"}},
                {"password": "plain"}
            ]
        });
        let path = vec![
            JsonPathKey::Field("servers".to_string()),
            JsonPathKey::Index(0),
            JsonPathKey::Field("password".to_string()),
        ];
        set_at_path(&mut root, &path, serde_json::json!("secret123")).unwrap();
        assert_eq!(root["servers"][0]["password"], serde_json::json!("secret123"));
    }

    #[test]
    fn test_set_at_path_invalid_path() {
        let mut root = serde_json::json!({"foo": "bar"});
        // Try to navigate through a nonexistent nested path
        let path = vec![
            JsonPathKey::Field("nonexistent".to_string()),
            JsonPathKey::Field("nested".to_string()),
        ];
        let result = set_at_path(&mut root, &path, serde_json::json!("value"));
        assert!(matches!(result, Err(SettingsError::InvalidSecretPath)));
    }

    #[test]
    fn test_set_at_path_array_index_out_of_bounds() {
        let mut root = serde_json::json!({"arr": [1, 2]});
        let path = vec![
            JsonPathKey::Field("arr".to_string()),
            JsonPathKey::Index(10),
        ];
        let result = set_at_path(&mut root, &path, serde_json::json!("value"));
        assert!(matches!(result, Err(SettingsError::InvalidSecretPath)));
    }

    #[test]
    fn test_resolve_secrets_sync_no_secrets() {
        let secrets = SecretsService::new_without_vault();
        let value = serde_json::json!({"host": "localhost"});
        let usages = vec![];

        let result = resolve_secrets_sync(&value, &usages, &secrets).unwrap();
        assert_eq!(result, value);
    }

}
