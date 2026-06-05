//! Bearer token authentication for MCP servers.

/// Constant-time-ish string comparison for bearer tokens.
///
/// Not true constant-time (uses length-based early exit), but avoids
/// the obvious timing leak of `==` on short strings.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes().zip(b.bytes()).fold(0, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Bearer token authenticator for MCP HTTP transport.
#[derive(Debug)]
pub struct BearerAuth {
    token: String,
}

impl BearerAuth {
    /// Create a new bearer auth with the given token.
    pub fn new(token: impl Into<String>) -> Self {
        Self { token: token.into() }
    }

    /// Generate a random bearer token.
    pub fn generate() -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::{SystemTime, UNIX_EPOCH};

        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64;
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        // Simple xorshift for a hex token — no external rand dep.
        let mut s = time.wrapping_add(counter).wrapping_add(0x9e3779b97f4a7c15);
        let mut token = String::with_capacity(32);
        for _ in 0..32 {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let nibble = (s & 0xF) as u8;
            token.push(if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 } as char);
        }
        Self { token }
    }

    /// Validate a bearer token from an Authorization header.
    pub fn validate(&self, header_value: &str) -> bool {
        if let Some(token) = header_value.strip_prefix("Bearer ") {
            constant_time_eq(token.trim(), &self.token)
        } else {
            false
        }
    }

    /// Get the raw token value.
    pub fn token(&self) -> &str {
        &self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_correct_token() {
        let auth = BearerAuth::new("my-secret-token");
        assert!(auth.validate("Bearer my-secret-token"));
    }

    #[test]
    fn reject_wrong_token() {
        let auth = BearerAuth::new("correct");
        assert!(!auth.validate("Bearer wrong"));
    }

    #[test]
    fn reject_missing_prefix() {
        let auth = BearerAuth::new("token");
        assert!(!auth.validate("token"));
        assert!(!auth.validate("Basic token"));
    }

    #[test]
    fn generate_produces_32_char_hex() {
        let auth = BearerAuth::generate();
        let token = auth.token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn generate_unique() {
        let a = BearerAuth::generate();
        let b = BearerAuth::generate();
        assert_ne!(a.token(), b.token());
    }
}
