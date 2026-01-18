mod kubernetes;
mod token;
mod token_info;

pub use kubernetes::KubernetesAuth;
pub use token::StaticTokenAuth;
pub use token_info::TokenInfo;

use crate::VaultError;
use async_trait::async_trait;

/// Trait for authentication methods
#[async_trait]
pub trait AuthMethod: Send + Sync {
    /// Perform initial authentication
    async fn authenticate(&self, base_url: &str) -> Result<TokenInfo, VaultError>;

    /// Whether this method supports token renewal via renew-self
    fn supports_renewal(&self) -> bool;
}
