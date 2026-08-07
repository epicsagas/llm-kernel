use std::io::Write;
use std::path::Path;

use crate::error::{KernelError, Result};

/// Write data to a file atomically using a temp file + rename.
///
/// On Unix, sets the file mode to `mode` (e.g. `0o600` for secrets).
pub(crate) fn write_atomic(path: impl AsRef<Path>, data: &[u8], mode: u32) -> Result<()> {
    let target = path.as_ref();
    let parent = target.parent().ok_or_else(|| {
        KernelError::Vault(format!(
            "path has no parent directory: {}",
            target.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    tmp.write_all(data)?;
    // Flush data blocks before the rename lands — a crash between rename and
    // writeback would otherwise replace the old file with a truncated one.
    tmp.as_file().sync_all()?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file_mut()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }

    tmp.persist(target)
        .map_err(|e| KernelError::Vault(format!("atomic persist failed: {}", e)))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_write_atomic_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let path_str = path.to_string_lossy().to_string();

        write_atomic(&path_str, b"hello", 0o644).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello");
    }

    #[test]
    fn test_write_atomic_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        let path_str = path.to_string_lossy().to_string();

        write_atomic(&path_str, b"first", 0o644).unwrap();
        write_atomic(&path_str, b"second", 0o644).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, "second");
    }
}
