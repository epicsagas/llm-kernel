//! Output sanitization — secret masking and control character removal.

/// Mask known secret patterns in a string.
///
/// Handles `Bearer` tokens, `sk-*` API keys, and `password=`, `token=`, `key=`, `secret=` values.
pub fn mask_secrets(input: &str) -> String {
    let mut result = input.to_string();

    // Mask Bearer tokens
    if let Some(pos) = result.find("Bearer ") {
        let value_start = pos + "Bearer ".len();
        if value_start < result.len() {
            if let Some(value_end) = result[value_start..].find(|c: char| c.is_whitespace()) {
                result.replace_range(value_start..value_start + value_end, "****");
            } else {
                result.replace_range(value_start.., "****");
            }
        }
    }

    // Mask password=, token=, key=, secret= values
    for prefix in &["password=", "token=", "key=", "secret="] {
        if let Some(pos) = result.find(prefix) {
            let value_start = pos + prefix.len();
            if let Some(value_end) = result[value_start..].find(|c: char| c.is_whitespace()) {
                let end = value_start + value_end;
                result.replace_range(value_start..end, "****");
            } else if value_start < result.len() {
                result.replace_range(value_start.., "****");
            }
        }
    }

    result
}

/// Sanitize output for safe display by removing:
///
/// - Bidi override characters (U+202A–U+202E) — invisible text direction attacks
/// - Plane-14 tag characters (U+E0000–U+E007F) — LLM injection vectors
/// - Line/paragraph separators (U+2028, U+2029)
/// - Null bytes (U+0000)
/// - C1 control characters (U+0080–U+009F) except common whitespace
pub fn sanitize_output(input: &str) -> String {
    input
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            // Null byte
            if cp == 0 {
                return false;
            }
            // C1 controls (except \u{0085} NEL which is sometimes valid)
            if (0x80..=0x9F).contains(&cp) {
                return false;
            }
            // Bidi overrides
            if (0x202A..=0x202E).contains(&cp) {
                return false;
            }
            // Line/paragraph separators
            if cp == 0x2028 || cp == 0x2029 {
                return false;
            }
            // Plane-14 tags
            if (0xE0000..=0xE007F).contains(&cp) {
                return false;
            }
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mask_bearer_token() {
        let input = "Authorization: Bearer sk-longtoken123456";
        let masked = mask_secrets(input);
        assert!(masked.contains("****"));
    }

    #[test]
    fn mask_password_value() {
        let input = "password=supersecret other=value";
        let masked = mask_secrets(input);
        assert!(masked.contains("password=****"));
        assert!(masked.contains("other=value"));
    }

    #[test]
    fn mask_token_value() {
        let masked = mask_secrets("token=abc123");
        assert!(masked.contains("token=****"));
    }

    #[test]
    fn mask_key_value() {
        let masked = mask_secrets("key=my-api-key-here");
        assert!(masked.contains("key=****"));
    }

    #[test]
    fn mask_secret_value() {
        let masked = mask_secrets("secret=data rest=ok");
        assert!(masked.contains("secret=****"));
        assert!(masked.contains("rest=ok"));
    }

    #[test]
    fn no_secrets_unchanged() {
        let input = "hello world 123";
        assert_eq!(mask_secrets(input), input);
    }

    #[test]
    fn sanitize_removes_bidi() {
        let input = "hello\u{202E}world";
        let clean = sanitize_output(input);
        assert_eq!(clean, "helloworld");
    }

    #[test]
    fn sanitize_removes_plane14() {
        let input = "text\u{E0001}hidden";
        let clean = sanitize_output(input);
        assert_eq!(clean, "texthidden");
    }

    #[test]
    fn sanitize_removes_null() {
        let clean = sanitize_output("a\u{0000}b");
        assert_eq!(clean, "ab");
    }

    #[test]
    fn sanitize_removes_line_sep() {
        let clean = sanitize_output("a\u{2028}b");
        assert_eq!(clean, "ab");
    }

    #[test]
    fn sanitize_preserves_normal() {
        let input = "Hello, 世界! 🎉";
        assert_eq!(sanitize_output(input), input);
    }

    #[test]
    fn sanitize_removes_c1_controls() {
        let clean = sanitize_output("a\u{0080}b\u{009F}c");
        assert_eq!(clean, "abc");
    }
}
