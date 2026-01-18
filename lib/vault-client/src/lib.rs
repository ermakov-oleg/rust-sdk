//! vault-client - Rust client for HashiCorp Vault
//!
//! Auto-detects authentication method:
//! 1. VAULT_TOKEN → static token
//! 2. KUBERNETES_SERVICE_HOST → K8s auth
//! 3. Otherwise → OIDC (local development)

mod error;
mod models;

pub use error::VaultError;
pub use models::{KvData, KvMetadata, KvVersion};
