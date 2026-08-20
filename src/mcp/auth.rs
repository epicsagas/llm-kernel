//! Bearer token authentication for MCP servers.

/// Constant-time-ish string comparison for bearer tokens.
///
/// Not true constant-time (uses length-based early exit), but avoids
/// the obvious timing leak of `==` on short strings.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Bearer token authenticator for MCP HTTP transport.
pub struct BearerAuth {
    token: String,
}

/// Deriving `Debug` would print the token into logs and panic messages.
impl std::fmt::Debug for BearerAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BearerAuth")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl BearerAuth {
    /// Create a new bearer auth with the given token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    /// Generate a random bearer token (128 bits of OS entropy, hex-encoded).
    ///
    /// Uses the OS CSPRNG via `getrandom`. If the OS entropy source is
    /// unavailable the call fails rather than falling back to a guessable
    /// token — see [`BearerAuth::try_generate`].
    ///
    /// # Panics
    ///
    /// Panics if the OS entropy source is unavailable. Use
    /// [`BearerAuth::try_generate`] to handle that case, or
    /// [`BearerAuth::new`] to supply an externally generated token.
    pub fn generate() -> Self {
        Self::try_generate().expect("OS entropy source unavailable")
    }

    /// Fallible [`BearerAuth::generate`] — never falls back to a weak token.
    pub fn try_generate() -> crate::error::Result<Self> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|e| {
            crate::error::KernelError::Config(format!("failed to read OS entropy for token: {e}"))
        })?;
        let mut token = String::with_capacity(32);
        for b in bytes {
            use std::fmt::Write;
            let _ = write!(token, "{b:02x}");
        }
        Ok(Self { token })
    }

    /// Validate a bearer token from an Authorization header.
    ///
    /// The auth scheme is matched case-insensitively, as RFC 7235 requires
    /// (`bearer x` and `Bearer x` are equivalent).
    pub fn validate(&self, header_value: &str) -> bool {
        match header_value.split_once(' ') {
            Some((scheme, token)) if scheme.eq_ignore_ascii_case("Bearer") => {
                constant_time_eq(token.trim(), &self.token)
            }
            _ => false,
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
    fn scheme_match_is_case_insensitive() {
        let auth = BearerAuth::new("token");
        assert!(auth.validate("bearer token"));
        assert!(auth.validate("BEARER token"));
        assert!(auth.validate("Bearer token"));
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

    #[test]
    fn debug_never_prints_token() {
        let auth = BearerAuth::new("super-secret-token");
        let dbg = format!("{auth:?}");
        assert!(!dbg.contains("super-secret-token"), "{dbg}");
    }
}
