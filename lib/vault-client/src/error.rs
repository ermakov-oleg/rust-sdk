use thiserror::Error;

#[derive(Debug, Error)]
pub enum VaultError {
    #[error("Vault not detected: VAULT_ADDR not set")]
    VaultNotDetected,

    #[error("Secret not found: {path}")]
    SecretNotFound { path: String },

    #[error("Vault client error ({status}): {message}")]
    ClientError {
        status: u16,
        message: String,
        response_data: Option<serde_json::Value>,
    },

    #[error("Vault request error: {0}")]
    RequestError(String),

    #[error("Authentication failed: {0}")]
    AuthError(String),

    #[error("OIDC authentication failed: {0}")]
    OidcError(String),

    #[error("Kubernetes auth failed: {0}")]
    KubernetesError(String),

    #[error("Token expired and renewal failed")]
    TokenExpired,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}
