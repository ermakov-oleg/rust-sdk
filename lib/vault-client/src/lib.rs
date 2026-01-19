//! vault-client - Rust client for HashiCorp Vault
//!
//! Auto-detects authentication method:
//! 1. VAULT_TOKEN → static token
//! 2. KUBERNETES_SERVICE_HOST → K8s auth
//! 3. Otherwise → OIDC (local development)
//!
//! # Example
//!
//! ```ignore
//! use vault_client::VaultClient;
//!
//! let client = VaultClient::from_env().await?;
//! let secret = client.kv_read("secret", "my/path").await?;
//! println!("{:?}", secret.data);
//! ```

mod auth;
mod client;
mod error;
mod models;

pub use client::{VaultClient, VaultClientBuilder};
pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};

// Re-export for advanced usage
pub use auth::TokenInfo;
