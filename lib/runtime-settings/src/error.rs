// lib/runtime-settings/src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("Failed to read settings file: {0}")]
    FileRead(#[from] std::io::Error),

    #[error("Failed to parse settings JSON: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("MCS request failed: {0}")]
    McsRequest(#[from] reqwest::Error),

    #[error("MCS returned error: status={status}, message={message}")]
    McsResponse { status: u16, message: String },

    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Secret key not found: {key} in {path}")]
    SecretKeyNotFound { path: String, key: String },

    #[error("Invalid secret reference: {reference}")]
    InvalidSecretReference { reference: String },

    #[error("Secret used but Vault not configured")]
    SecretWithoutVault,

    #[error("Vault error: {0}")]
    Vault(String),

    #[error("Invalid regex pattern: {pattern}, error: {error}")]
    InvalidRegex { pattern: String, error: String },

    #[error("Invalid version specifier: {spec}")]
    InvalidVersionSpec { spec: String },
}
