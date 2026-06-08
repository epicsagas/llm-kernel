use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;

use crate::error::{KernelError, Result};

use super::atomic::write_atomic;

/// Credential store backed by a dotenv-style file.
///
/// Wraps a `HashMap<String, String>` with typed methods for load/save/normalize,
/// keeping the ergonomics of a map via `Deref`/`DerefMut`.
#[derive(Debug, Clone, Default)]
pub struct SecretVault(HashMap<String, String>);

impl SecretVault {
    pub fn empty() -> Self {
        Self(HashMap::new())
    }

    pub fn load_from(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();

        // Symlink check BEFORE read to prevent TOCTOU race.
        if path.exists() {
            Self::guard_not_symlink(path)?;
        }

        let raw = match std::fs::read(path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(e) => return Err(e.into()),
        };

        raw.split(|&b| b == b'\n')
            .enumerate()
            .filter(|(_, line)| {
                let text = std::str::from_utf8(line).unwrap_or("");
                let trimmed = text.trim();
                !trimmed.is_empty() && !trimmed.starts_with('#')
            })
            .try_fold(Self::empty(), |mut acc, (i, line)| {
                let text = std::str::from_utf8(line)
                    .map_err(|e| {
                        KernelError::Vault(format!("invalid UTF-8 on line {}: {}", i + 1, e))
                    })?
                    .trim();
                let (key, raw_val) = text.split_once('=').ok_or_else(|| {
                    KernelError::Vault(format!("invalid secrets file line {}", i + 1))
                })?;
                if !is_valid_env_key(key) {
                    return Err(KernelError::Vault(format!(
                        "invalid secrets file line {}",
                        i + 1
                    )));
                }
                acc.0.insert(key.to_owned(), decode_shell_value(raw_val)?);
                Ok(acc)
            })
    }

    pub fn persist_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let body = self
            .0
            .keys()
            .filter(|k| is_valid_env_key(k))
            .collect::<std::collections::BTreeSet<_>>()
            .iter()
            .map(|k| format!("{}={}\n", k, encode_for_shell(&self.0[*k])))
            .collect::<String>();

        write_atomic(&p.to_string_lossy(), body.as_bytes(), 0o600)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o600))?;
        }
        Ok(())
    }

    fn guard_not_symlink(path: &Path) -> Result<()> {
        let meta = std::fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            return Err(KernelError::Vault(format!(
                "secrets file is a symlink: {}",
                path.display()
            )));
        }
        Ok(())
    }
}

// --- Deref/DerefMut so callers can use `.get()`, `.iter()`, etc. ---

impl Deref for SecretVault {
    type Target = HashMap<String, String>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SecretVault {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<HashMap<String, String>> for SecretVault {
    fn from(map: HashMap<String, String>) -> Self {
        Self(map)
    }
}

impl IntoIterator for SecretVault {
    type Item = (String, String);
    type IntoIter = std::collections::hash_map::IntoIter<String, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a> IntoIterator for &'a SecretVault {
    type Item = (&'a String, &'a String);
    type IntoIter = std::collections::hash_map::Iter<'a, String, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Mask a credential for display, showing only first/last 4 chars.
pub fn redact_credential(value: &str) -> String {
    match value.len() {
        0 => String::new(),
        1..=8 => "****".to_owned(),
        _ => format!("{}****{}", &value[..4], &value[value.len() - 4..]),
    }
}

// --- Internal helpers ---

fn is_valid_env_key(key: &str) -> bool {
    let first = key.as_bytes().first();
    first.is_some_and(|&b| {
        (b.is_ascii_uppercase() || b == b'_')
            && key.as_bytes()[1..]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
    })
}

fn decode_shell_value(value: &str) -> Result<String> {
    let b = value.as_bytes();
    match b.first() {
        Some(b'\'') if b.last() == Some(&b'\'') && b.len() >= 2 => {
            Ok(value[1..value.len() - 1].to_owned())
        }
        Some(b'$') if b.get(1) == Some(&b'\'') && b.last() == Some(&b'\'') => {
            unescape_ansi(&value[2..value.len() - 1])
        }
        Some(b'"') if b.last() == Some(&b'"') && b.len() >= 2 => {
            Ok(value[1..value.len() - 1].replace("\\\"", "\""))
        }
        _ => Ok(value.to_owned()),
    }
}

fn unescape_ansi(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.as_bytes().iter().copied().peekable();
    while let Some(b) = chars.next() {
        if b != b'\\' {
            out.push(b as char);
            continue;
        }
        match chars.next() {
            None => return Err(KernelError::Vault("unterminated escape".into())),
            Some(b'n') => out.push('\n'),
            Some(b't') => out.push('\t'),
            Some(b'\\') => out.push('\\'),
            Some(b'\'') => out.push('\''),
            Some(other) => out.push(other as char),
        }
    }
    Ok(out)
}

fn encode_for_shell(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    let needs_quoting = value
        .as_bytes()
        .iter()
        .any(|b| matches!(b, b'\n' | b'\t' | b'\'' | b'\\' | b' '));
    if !needs_quoting {
        return value.to_owned();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
        .replace('\'', "\\'");
    format!("$'{}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_short() {
        assert_eq!(redact_credential("ab"), "****");
    }

    #[test]
    fn test_redact_empty() {
        assert_eq!(redact_credential(""), "");
    }

    #[test]
    fn test_redact_long() {
        assert_eq!(redact_credential("abcdefghijklmnop"), "abcd****mnop");
    }

    #[test]
    fn test_decode_single_quotes() {
        assert_eq!(decode_shell_value("'hello world'").unwrap(), "hello world");
    }

    #[test]
    fn test_decode_ansi_dollar_quotes() {
        assert_eq!(
            decode_shell_value("$'hello\\nworld'").unwrap(),
            "hello\nworld"
        );
        assert_eq!(decode_shell_value("$'tab\\there'").unwrap(), "tab\there");
        assert_eq!(
            decode_shell_value("$'back\\\\slash'").unwrap(),
            "back\\slash"
        );
        assert_eq!(decode_shell_value("$'quo\\'te'").unwrap(), "quo'te");
    }

    #[test]
    fn test_decode_double_quotes() {
        assert_eq!(
            decode_shell_value("\"hello \\\"world\\\"\"").unwrap(),
            "hello \"world\""
        );
    }

    #[test]
    fn test_decode_bare() {
        assert_eq!(decode_shell_value("simple123").unwrap(), "simple123");
    }

    #[test]
    fn test_encode_simple() {
        assert_eq!(encode_for_shell("hello"), "hello");
    }

    #[test]
    fn test_encode_empty() {
        assert_eq!(encode_for_shell(""), "''");
    }

    #[test]
    fn test_encode_special() {
        let quoted = encode_for_shell("hello world");
        assert!(
            quoted.starts_with("$'"),
            "expected $'...' for space, got {}",
            quoted
        );
    }

    #[test]
    fn test_is_valid_env_key() {
        assert!(is_valid_env_key("VALID_KEY"));
        assert!(is_valid_env_key("_LEADING"));
        assert!(!is_valid_env_key(""));
        assert!(!is_valid_env_key("lowercase"));
        assert!(!is_valid_env_key("1STARTS_NUM"));
        assert!(is_valid_env_key("HAS_123"));
        assert!(!is_valid_env_key("HAS-DASH"));
    }

    #[test]
    fn test_roundtrip_via_impl_methods() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");

        let secrets = SecretVault::from(HashMap::from([
            ("MY_KEY".to_string(), "my-value".to_string()),
            ("OTHER_KEY".to_string(), "other".to_string()),
        ]));

        secrets.persist_to(&path).expect("persist");
        let loaded = SecretVault::load_from(&path).expect("load");

        assert_eq!(loaded.get("MY_KEY").map(|s| s.as_str()), Some("my-value"));
        assert_eq!(loaded.get("OTHER_KEY").map(|s| s.as_str()), Some("other"));
    }

    #[test]
    fn test_load_missing_returns_empty() {
        let secrets =
            SecretVault::load_from("/nonexistent/path/secrets.env").expect("load missing");
        assert!(secrets.is_empty());
    }

    #[test]
    fn test_roundtrip_with_special_chars() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");

        let secrets = SecretVault::from(HashMap::from([(
            "MY_KEY".to_string(),
            "value with spaces\nand newlines".to_string(),
        )]));

        secrets.persist_to(&path).expect("persist");
        let loaded = SecretVault::load_from(&path).expect("load");

        assert_eq!(
            loaded.get("MY_KEY").map(|s| s.as_str()),
            Some("value with spaces\nand newlines")
        );
    }
}
