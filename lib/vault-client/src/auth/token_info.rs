use std::time::{Duration, Instant};

/// Token information from authentication
#[derive(Debug, Clone)]
pub struct TokenInfo {
    pub token: String,
    pub lease_duration: Duration,
    pub renewable: bool,
    pub obtained_at: Instant,
}

impl TokenInfo {
    pub fn new(token: String, lease_duration: Duration, renewable: bool) -> Self {
        Self {
            token,
            lease_duration,
            renewable,
            obtained_at: Instant::now(),
        }
    }

    /// Static token (never expires)
    pub fn static_token(token: String) -> Self {
        Self {
            token,
            lease_duration: Duration::ZERO,
            renewable: false,
            obtained_at: Instant::now(),
        }
    }

    /// Check if token needs refresh (at threshold % of lease)
    pub fn needs_refresh(&self, threshold: f64) -> bool {
        if self.lease_duration.is_zero() {
            return false; // Static token
        }
        let elapsed = self.obtained_at.elapsed();
        let threshold_duration = Duration::from_secs_f64(self.lease_duration.as_secs_f64() * threshold);
        elapsed >= threshold_duration
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        if self.lease_duration.is_zero() {
            return false;
        }
        self.obtained_at.elapsed() >= self.lease_duration
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_static_token_never_expires() {
        let token = TokenInfo::static_token("test".to_string());
        assert!(!token.needs_refresh(0.75));
        assert!(!token.is_expired());
    }

    #[test]
    fn test_token_needs_refresh_at_threshold() {
        let mut token = TokenInfo::new("test".to_string(), Duration::from_secs(100), true);
        // Simulate time passing
        token.obtained_at = Instant::now() - Duration::from_secs(80);
        assert!(token.needs_refresh(0.75));
    }

    #[test]
    fn test_token_not_expired_before_lease() {
        let mut token = TokenInfo::new("test".to_string(), Duration::from_secs(100), true);
        token.obtained_at = Instant::now() - Duration::from_secs(50);
        assert!(!token.is_expired());
    }
}
