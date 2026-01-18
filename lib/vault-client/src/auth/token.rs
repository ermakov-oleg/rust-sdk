use super::{AuthMethod, TokenInfo};
use crate::VaultError;
use async_trait::async_trait;

/// Static token authentication
pub struct StaticTokenAuth {
    token: String,
}

impl StaticTokenAuth {
    pub fn new(token: String) -> Self {
        Self { token }
    }
}

#[async_trait]
impl AuthMethod for StaticTokenAuth {
    async fn authenticate(&self, _base_url: &str) -> Result<TokenInfo, VaultError> {
        Ok(TokenInfo::static_token(self.token.clone()))
    }

    fn supports_renewal(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_static_token_auth() {
        let auth = StaticTokenAuth::new("my-token".to_string());
        let token_info = auth.authenticate("http://vault:8200").await.unwrap();
        assert_eq!(token_info.token, "my-token");
        assert!(!auth.supports_renewal());
    }
}
