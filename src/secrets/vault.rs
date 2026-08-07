use std::collections::HashMap;
use std::ops::{Deref, DerefMut};
use std::path::Path;

use zeroize::Zeroize;

use crate::error::{KernelError, Result};

use super::atomic::write_atomic;

/// Credential store backed by a dotenv-style file.
///
/// Wraps a `HashMap<String, String>` with typed methods for load/save/normalize,
/// keeping the ergonomics of a map via `Deref`/`DerefMut`.
///
/// # Security model
///
/// Values are stored **in plaintext**, in a file written `0o600` (owner-only)
/// via an atomic temp-file rename. This protects against other local users
/// and against torn writes; it does **not** protect against an attacker who
/// already runs as this user, nor against disk forensics. It is not an OS
/// keychain — do not describe it as one. For stronger guarantees, hold the
/// key in an OS keychain and pass it in rather than persisting it here.
///
/// Values are zeroized on drop and after the serialized body is written, so
/// they do not linger in freed heap pages. This is best-effort: `DerefMut`
/// and `IntoIterator` let copies escape, and those are the caller's to wipe.
#[derive(Clone, Default)]
pub struct SecretVault(HashMap<String, String>);

/// Wipe every value when the vault goes away, so credentials do not linger
/// in freed heap pages (core dumps, swap). Best-effort: `DerefMut` lets a
/// caller clone a value out, and that copy is theirs to manage.
impl Drop for SecretVault {
    fn drop(&mut self) {
        for value in self.0.values_mut() {
            value.zeroize();
        }
    }
}

/// Deriving `Debug` would print every secret verbatim into logs and panic
/// messages — show only the key names.
impl std::fmt::Debug for SecretVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut keys: Vec<&str> = self.0.keys().map(String::as_str).collect();
        keys.sort_unstable();
        f.debug_tuple("SecretVault").field(&keys).finish()
    }
}

impl SecretVault {
    /// Create an empty vault with no credentials loaded.
    pub fn empty() -> Self {
        Self(HashMap::new())
    }

    /// Load a vault from a dotenv-style file at `path`.
    ///
    /// Returns an empty vault if the file does not exist.
    /// Errors if the file is a symlink, has invalid UTF-8, or contains malformed lines.
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
                // Invalid UTF-8 must NOT be filtered out here: treating it as
                // an empty line would silently drop the entry, and the next
                // persist_to would erase it from disk for good. Let it reach
                // the fold and error there.
                match std::str::from_utf8(line) {
                    Ok(text) => {
                        let trimmed = text.trim();
                        !trimmed.is_empty() && !trimmed.starts_with('#')
                    }
                    Err(_) => true,
                }
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

    /// Persist the vault to a dotenv-style file at `path` using an atomic write.
    pub fn persist_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let p = path.as_ref();
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // A key that cannot be written must fail loudly: silently skipping it
        // makes `insert(...); persist_to(...)` report success while the
        // credential never reaches disk.
        if let Some(bad) = self.0.keys().find(|k| !is_valid_env_key(k)) {
            return Err(KernelError::Vault(format!(
                "cannot persist invalid secret key {bad:?} (expected [A-Z_][A-Z0-9_]*)"
            )));
        }

        let mut body = self
            .0
            .keys()
            .collect::<std::collections::BTreeSet<_>>()
            .iter()
            .map(|k| format!("{}={}\n", k, encode_for_shell(&self.0[*k])))
            .collect::<String>();

        // Pass the Path itself — a lossy string conversion would silently
        // write to a DIFFERENT file on a non-UTF-8 path.
        let result = write_atomic(p, body.as_bytes(), 0o600);
        // `body` is a full plaintext copy of every secret; wipe it before the
        // allocation goes back to the heap (readable in a core dump or swap).
        Zeroize::zeroize(&mut body);
        result?;
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
    /// Takes the map out so the `Drop` impl (which zeroizes) can still run —
    /// moving a field out of a `Drop` type is not allowed. The values handed
    /// to the caller are theirs to wipe.
    fn into_iter(mut self) -> Self::IntoIter {
        std::mem::take(&mut self.0).into_iter()
    }
}

impl<'a> IntoIterator for &'a SecretVault {
    type Item = (&'a String, &'a String);
    type IntoIter = std::collections::hash_map::Iter<'a, String, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Mask a credential for display, showing only first/last 4 characters.
///
/// Counts characters, not bytes: byte slicing at fixed offsets panics on any
/// multi-byte credential, and this runs on error/log paths where a panic is
/// the worst possible outcome.
pub fn redact_credential(value: &str) -> String {
    let count = value.chars().count();
    match count {
        0 => String::new(),
        1..=8 => "****".to_owned(),
        _ => {
            let head: String = value.chars().take(4).collect();
            let tail: String = value.chars().skip(count - 4).collect();
            format!("{head}****{tail}")
        }
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
        // len >= 3 so the opening `$'` and the closing `'` are distinct bytes
        // — the bare string `$'` would otherwise slice [2..1] and panic.
        Some(b'$') if b.len() >= 3 && b.get(1) == Some(&b'\'') && b.last() == Some(&b'\'') => {
            unescape_ansi(&value[2..value.len() - 1])
        }
        Some(b'"') if b.last() == Some(&b'"') && b.len() >= 2 => {
            Ok(value[1..value.len() - 1].replace("\\\"", "\""))
        }
        _ => Ok(value.to_owned()),
    }
}

fn unescape_ansi(s: &str) -> Result<String> {
    // Iterate CHARS, not bytes — `b as char` reinterprets each UTF-8 byte as
    // Latin-1, silently corrupting any non-ASCII secret on load.
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            None => return Err(KernelError::Vault("unterminated escape".into())),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('\\') => out.push('\\'),
            Some('\'') => out.push('\''),
            Some(other) => out.push(other),
        }
    }
    Ok(out)
}

fn encode_for_shell(value: &str) -> String {
    if value.is_empty() {
        return "''".to_owned();
    }
    // A value written bare must decode back to itself. `decode_shell_value`
    // strips a surrounding pair of `'` or `"` and treats a leading `$'` as an
    // ANSI-C string, so any value that could be mistaken for one of those
    // forms has to be explicitly quoted or the round-trip silently mangles it.
    let needs_quoting = value
        .as_bytes()
        .iter()
        .any(|b| matches!(b, b'\n' | b'\t' | b'\'' | b'"' | b'\\' | b' '))
        || value.starts_with('$');
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
    fn test_redact_multibyte_does_not_panic() {
        // Byte slicing at fixed offsets would panic mid-codepoint here.
        // 8 chars or fewer are fully masked; 9+ show first/last 4.
        assert_eq!(redact_credential("한국어키값입니다"), "****");
        assert_eq!(
            redact_credential("한국어키값입니다요"),
            "한국어키****입니다요"
        );
        assert_eq!(redact_credential(&"é".repeat(12)).chars().count(), 12);
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
    fn test_roundtrip_non_ascii_with_quoting_trigger() {
        // Space forces $'...' encoding; the decoder must not corrupt UTF-8.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");
        let secrets = SecretVault::from(HashMap::from([(
            "MY_KEY".to_string(),
            "한국어 키 값".to_string(),
        )]));
        secrets.persist_to(&path).expect("persist");
        let loaded = SecretVault::load_from(&path).expect("load");
        assert_eq!(
            loaded.get("MY_KEY").map(|s| s.as_str()),
            Some("한국어 키 값")
        );
    }

    #[test]
    fn test_roundtrip_values_that_look_like_quoting() {
        // Every form decode_shell_value would strip must survive persist+load.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");
        for val in ["\"quoted\"", "'single'", "$'ansi'", "$plain", "sk-normal"] {
            let v = SecretVault::from(HashMap::from([("K".to_string(), val.to_string())]));
            v.persist_to(&path).expect("persist");
            let loaded = SecretVault::load_from(&path).expect("load");
            assert_eq!(
                loaded.get("K").map(|s| s.as_str()),
                Some(val),
                "value {val:?}"
            );
        }
    }

    #[test]
    fn test_persist_rejects_invalid_key_instead_of_dropping_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");
        let v = SecretVault::from(HashMap::from([("lowercase".to_string(), "v".to_string())]));
        assert!(v.persist_to(&path).is_err(), "silent drop is data loss");
    }

    #[test]
    fn test_invalid_utf8_line_errors_instead_of_vanishing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("secrets.env");
        std::fs::write(&path, b"GOOD=1\nBAD=\xff\xfe\n").expect("write");
        assert!(SecretVault::load_from(&path).is_err());
    }

    #[test]
    fn test_decode_bare_dollar_quote_does_not_panic() {
        // A value of exactly `$'` must not slice out of bounds.
        assert_eq!(decode_shell_value("$'").unwrap(), "$'");
    }

    #[test]
    fn test_debug_never_prints_secret_values() {
        let vault = SecretVault::from(HashMap::from([(
            "API_KEY".to_string(),
            "sk-super-secret".to_string(),
        )]));
        let dbg = format!("{vault:?}");
        assert!(dbg.contains("API_KEY"));
        assert!(!dbg.contains("sk-super-secret"), "{dbg}");
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
