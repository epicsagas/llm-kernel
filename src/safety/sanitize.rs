//! Output sanitization — secret masking and control character removal.

use regex::Regex;
use std::sync::LazyLock;

// Single compiled pattern covering all secret types in one pass:
//   group 1 — Bearer/Basic prefix (case-insensitive)
//   group 2 — Bearer/Basic token value
//   group 3 — key=value prefix (password=, token=, ...)
//   group 4 — key=value value
//   groups 5–7 — standalone sk-*, AKIA*, gh[posu]_* tokens
static SECRET_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i:bearer |basic )(\S+)|((?:password|token|key|secret|api_key|apikey|access_token|private_key)=)(\S+)|(sk-\S+)|(AKIA[A-Za-z0-9]+)|(gh[posu]_[A-Za-z0-9]+)"
    ).expect("SECRET_RE is valid")
});

/// Mask known secret patterns in a string in a single pass.
///
/// Handles `Bearer`/`Basic` auth headers (case-insensitive), `sk-*` API keys,
/// AWS keys (`AKIA...`), GitHub tokens (`ghp_`/`gho_`/`ghs_`/`ghu_`), and
/// `password=`, `token=`, `key=`, `secret=`, `api_key=`, `apikey=`,
/// `access_token=`, `private_key=` values. All occurrences are masked.
pub fn mask_secrets(input: &str) -> String {
    SECRET_RE
        .replace_all(input, |caps: &regex::Captures| -> String {
            if caps.get(1).is_some() {
                // Bearer/Basic: full match = "<prefix> <value>"; group 1 = value only
                let full = caps.get(0).unwrap().as_str();
                let val = caps.get(1).unwrap().as_str();
                let prefix = &full[..full.len() - val.len()];
                format!("{prefix}****")
            } else if let Some(key_prefix) = caps.get(2) {
                // key=value: group 2 = "key=", group 3 = value
                format!("{}****", key_prefix.as_str())
            } else {
                // standalone: sk-*, AKIA*, gh[posu]_*
                "****".to_string()
            }
        })
        .into_owned()
}

/// Remove ANSI escape sequences from text.
pub fn strip_ansi(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\x1B' {
            // Skip ESC and the following sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip parameter bytes (0x30-0x3F), intermediate bytes (0x20-0x2F), final byte (0x40-0x7E)
                while let Some(&next) = chars.peek() {
                    let cp = next as u32;
                    if (0x30..=0x3F).contains(&cp) || (0x20..=0x2F).contains(&cp) {
                        chars.next();
                    } else if (0x40..=0x7E).contains(&cp) {
                        chars.next(); // consume final byte
                        break;
                    } else {
                        break;
                    }
                }
            }
            continue;
        }
        result.push(ch);
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
    fn mask_multiple_passwords() {
        let masked = mask_secrets("password=a password=b");
        assert!(masked.contains("password=****"), "got: {masked}");
    }

    #[test]
    fn mask_multiple_bearer() {
        let masked = mask_secrets("Bearer token1 and Bearer token2");
        assert_eq!(masked, "Bearer **** and Bearer ****");
    }

    #[test]
    fn mask_standalone_sk_key() {
        let masked = mask_secrets("key is sk-proj-abc123 here");
        assert!(masked.contains("****"), "got: {masked}");
        assert!(!masked.contains("sk-proj"), "got: {masked}");
    }

    #[test]
    fn mask_api_key_value() {
        let masked = mask_secrets("api_key=my-secret-key other=ok");
        assert!(masked.contains("api_key=****"));
        assert!(masked.contains("other=ok"));
    }

    #[test]
    fn mask_apikey_nounderscore() {
        let masked = mask_secrets("apikey=abc123");
        assert!(masked.contains("apikey=****"));
    }

    #[test]
    fn mask_access_token_value() {
        let masked = mask_secrets("access_token=eyJhbGciOiJI");
        assert!(masked.contains("access_token=****"));
    }

    #[test]
    fn mask_private_key_value() {
        let masked = mask_secrets("private_key=-----BEGIN RSA");
        assert!(masked.contains("private_key=****"));
    }

    #[test]
    fn mask_basic_auth() {
        let masked = mask_secrets("Authorization: Basic dXNlcjpwYXNz");
        assert!(masked.contains("Basic ****"));
    }

    #[test]
    fn mask_bearer_case_insensitive() {
        let masked = mask_secrets("auth: bearer token123 here");
        assert!(masked.contains("bearer ****"));
    }

    #[test]
    fn mask_aws_access_key() {
        let masked = mask_secrets("key=AKIAIOSFODNN7EXAMPLE end");
        assert!(masked.contains("key=****"));
    }

    #[test]
    fn mask_standalone_aws_key() {
        let masked = mask_secrets("found AKIAIOSFODNN7EXAMPLE in config");
        assert!(!masked.contains("AKIAIOSFODNN7EXAMPLE"), "got: {masked}");
        assert!(masked.contains("****"));
    }

    #[test]
    fn mask_github_pat() {
        let masked = mask_secrets("token ghp_ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefgh end");
        assert!(!masked.contains("ghp_"), "got: {masked}");
        assert!(masked.contains("****"));
    }

    #[test]
    fn mask_github_oauth() {
        let masked = mask_secrets("using gho_ABCDEF1234567890 here");
        assert!(!masked.contains("gho_"), "got: {masked}");
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

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1B[31mHello\x1B[0m \x1B[1;32mWorld\x1B[0m";
        let clean = strip_ansi(input);
        assert_eq!(clean, "Hello World");
    }

    #[test]
    fn strip_ansi_preserves_plain_text() {
        let input = "Hello, 世界! 🎉";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn strip_ansi_handles_complex_sequence() {
        // 256-color and RGB sequences
        let input = "\x1B[38;5;196mRed\x1B[0m \x1B[38;2;0;255;0mGreen\x1B[0m";
        let clean = strip_ansi(input);
        assert_eq!(clean, "Red Green");
    }
}
