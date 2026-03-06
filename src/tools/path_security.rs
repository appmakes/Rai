use anyhow::Result;
use std::path::{Component, Path, PathBuf};

/// System-critical prefixes (Unix) — always blocked even if future allowlists match.
/// Inspired by nullclaw's path_security.zig — protects OS-level paths from any tool.
pub const SYSTEM_BLOCKED_PREFIXES_UNIX: &[&str] = &[
    "/System",
    "/Library",
    "/bin",
    "/sbin",
    "/usr/bin",
    "/usr/sbin",
    "/usr/lib",
    "/usr/libexec",
    "/etc",
    "/private/etc",
    "/private/var",
    "/dev",
    "/boot",
    "/proc",
    "/sys",
];

/// System-critical prefixes (Windows) — always blocked.
/// Case-insensitive matching is applied at check time.
pub const SYSTEM_BLOCKED_PREFIXES_WINDOWS: &[&str] = &[
    r"C:\Windows",
    r"C:\Program Files",
    r"C:\Program Files (x86)",
    r"C:\ProgramData",
    r"C:\Recovery",
    r"C:\$Recycle.Bin",
];

/// Sensitive dot-files and directories that should not be written to.
/// Read access is allowed; write/edit/append are blocked.
const SENSITIVE_DOTFILES: &[&str] = &[
    ".ssh",
    ".gnupg",
    ".gpg",
    ".aws",
    ".docker",
    ".kube",
    ".npmrc",
    ".netrc",
    ".env",
    ".git/config",
    ".gitconfig",
];

pub fn ensure_not_system_critical_path(path: &str) -> Result<()> {
    ensure_not_system_critical_path_with_base(path, None)
}

pub fn ensure_not_system_critical_path_with_base(
    path: &str,
    base_dir: Option<&Path>,
) -> Result<()> {
    if path.trim().is_empty() {
        anyhow::bail!("Path must not be empty");
    }

    // Block null bytes (CWE-158) — prevents bypassing path checks.
    if path.contains('\0') {
        anyhow::bail!("Path contains null byte — blocked for safety");
    }

    // Block URL-encoded traversal patterns (case-insensitive).
    // Catches ..%2f, %2f.., ..%5c, %5c.. and mixed encodings.
    let lower = path.to_ascii_lowercase();
    if lower.contains("..%2f")
        || lower.contains("%2f..")
        || lower.contains("..%5c")
        || lower.contains("%5c..")
    {
        anyhow::bail!("Path contains URL-encoded traversal — blocked for safety");
    }

    let resolved = resolve_for_security(path, base_dir)?;

    if cfg!(unix) {
        for prefix in SYSTEM_BLOCKED_PREFIXES_UNIX {
            if path_starts_with_dir(&resolved, Path::new(prefix)) {
                anyhow::bail!(
                    "Access to system-critical path is blocked: {}",
                    prefix
                );
            }
        }
    }

    if cfg!(windows) {
        let resolved_lower = resolved.to_string_lossy().to_ascii_lowercase();
        for prefix in SYSTEM_BLOCKED_PREFIXES_WINDOWS {
            let prefix_lower = prefix.to_ascii_lowercase();
            if resolved_lower.starts_with(&prefix_lower)
                && (resolved_lower.len() == prefix_lower.len()
                    || resolved_lower.as_bytes().get(prefix_lower.len()) == Some(&b'\\'))
            {
                anyhow::bail!(
                    "Access to system-critical path is blocked: {}",
                    prefix
                );
            }
        }
    }

    Ok(())
}

/// Additional check for write operations — also blocks sensitive dotfiles.
pub fn ensure_safe_write_path(path: &str) -> Result<()> {
    ensure_safe_write_path_with_base(path, None)
}

pub fn ensure_safe_write_path_with_base(path: &str, base_dir: Option<&Path>) -> Result<()> {
    // First run the standard system-critical check.
    ensure_not_system_critical_path_with_base(path, base_dir)?;

    let resolved = resolve_for_security(path, base_dir)?;
    let resolved_str = resolved.to_string_lossy();

    for dotfile in SENSITIVE_DOTFILES {
        // Check with both separators so this works on Unix and Windows.
        let unix_pattern = format!("/{}", dotfile);
        let win_pattern = format!("\\{}", dotfile);
        if resolved_str.contains(&unix_pattern)
            || resolved_str.contains(&win_pattern)
            || resolved_str.ends_with(dotfile)
        {
            anyhow::bail!(
                "Write to sensitive path is blocked: {} (contains {})",
                path,
                dotfile
            );
        }
    }

    Ok(())
}

/// Directory-aware prefix matching (from nullclaw).
/// Requires exact match OR path separator boundary to prevent partial prefix bypass.
/// e.g. /etc matches /etc/passwd but NOT /etc2/config.
fn path_starts_with_dir(path: &Path, prefix: &Path) -> bool {
    path.starts_with(prefix)
}

fn resolve_for_security(path: &str, base_dir: Option<&Path>) -> Result<PathBuf> {
    let input = Path::new(path);
    let base = match base_dir {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|e| anyhow::anyhow!("Failed to get current directory: {}", e))?,
    };
    let absolute = if input.is_absolute() {
        input.to_path_buf()
    } else {
        base.join(input)
    };

    // Try real canonicalization first (follows symlinks).
    if let Ok(canonical) = absolute.canonicalize() {
        return Ok(canonical);
    }

    // If file doesn't exist yet, canonicalize the parent and append the filename.
    if let Some(parent) = absolute.parent() {
        if let Ok(canonical_parent) = parent.canonicalize() {
            if let Some(name) = absolute.file_name() {
                return Ok(canonical_parent.join(name));
            }
            return Ok(canonical_parent);
        }
    }

    // Fallback: lexical normalization (resolves .. without filesystem access).
    Ok(normalize_lexical(&absolute))
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            Component::Normal(segment) => normalized.push(segment),
        }
    }
    if normalized.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        normalized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    // ---------- system-critical path tests ----------

    #[test]
    fn blocks_exact_system_prefixes() {
        if !cfg!(unix) {
            return;
        }
        for prefix in SYSTEM_BLOCKED_PREFIXES_UNIX {
            assert!(
                ensure_not_system_critical_path_with_base(prefix, None).is_err(),
                "expected blocked prefix: {}",
                prefix
            );
        }
    }

    #[test]
    fn blocks_subpath_under_system_prefix() {
        if !cfg!(unix) {
            return;
        }
        assert!(ensure_not_system_critical_path_with_base("/etc/ssh/sshd_config", None).is_err());
    }

    #[test]
    fn allows_non_system_paths() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_not_system_critical_path_with_base("src/main.rs", Some(Path::new("/workspace")))
                .is_ok()
        );
    }

    #[test]
    fn does_not_block_partial_prefix_match() {
        if !cfg!(unix) {
            return;
        }
        assert!(ensure_not_system_critical_path_with_base("/etc2/config", None).is_ok());
    }

    #[test]
    fn resolves_relative_parent_traversal_before_checking() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_not_system_critical_path_with_base("../etc/passwd", Some(Path::new("/")))
                .is_err()
        );
    }

    // ---------- null byte and URL-encoded traversal ----------

    #[test]
    fn blocks_null_bytes() {
        assert!(ensure_not_system_critical_path("src/\0evil").is_err());
    }

    #[test]
    fn blocks_url_encoded_traversal() {
        assert!(ensure_not_system_critical_path("..%2fetc/passwd").is_err());
        assert!(ensure_not_system_critical_path("%2f..%2f..%2fetc").is_err());
        assert!(ensure_not_system_critical_path("..%5Cwindows").is_err());
    }

    // ---------- sensitive dotfile write tests ----------

    #[test]
    fn blocks_write_to_ssh_dir() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_safe_write_path_with_base(".ssh/id_rsa", Some(Path::new("/home/user"))).is_err()
        );
    }

    #[test]
    fn blocks_write_to_env_file() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_safe_write_path_with_base(".env", Some(Path::new("/project"))).is_err()
        );
    }

    #[test]
    fn blocks_write_to_aws_credentials() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_safe_write_path_with_base(".aws/credentials", Some(Path::new("/home/user")))
                .is_err()
        );
    }

    #[test]
    fn allows_write_to_normal_file() {
        if !cfg!(unix) {
            return;
        }
        assert!(
            ensure_safe_write_path_with_base("src/main.rs", Some(Path::new("/workspace"))).is_ok()
        );
    }

    // ---------- Windows system path tests ----------

    #[test]
    fn blocks_windows_system_paths() {
        if !cfg!(windows) {
            return;
        }
        for prefix in SYSTEM_BLOCKED_PREFIXES_WINDOWS {
            assert!(
                ensure_not_system_critical_path_with_base(prefix, None).is_err(),
                "expected blocked prefix: {}",
                prefix
            );
        }
    }

    #[test]
    fn blocks_windows_subpath() {
        if !cfg!(windows) {
            return;
        }
        assert!(
            ensure_not_system_critical_path_with_base(
                r"C:\Windows\System32\drivers",
                None
            )
            .is_err()
        );
    }

    #[test]
    fn blocks_windows_case_insensitive() {
        if !cfg!(windows) {
            return;
        }
        assert!(
            ensure_not_system_critical_path_with_base(r"c:\windows\system32", None).is_err()
        );
        assert!(
            ensure_not_system_critical_path_with_base(r"C:\PROGRAM FILES\app", None).is_err()
        );
    }

    #[test]
    fn allows_windows_non_system_paths() {
        if !cfg!(windows) {
            return;
        }
        assert!(
            ensure_not_system_critical_path_with_base(
                r"C:\Users\dev\project\src\main.rs",
                None
            )
            .is_ok()
        );
    }

    #[test]
    fn allows_read_of_sensitive_dotfile() {
        if !cfg!(unix) {
            return;
        }
        // Read (ensure_not_system_critical_path) should NOT block dotfiles.
        assert!(
            ensure_not_system_critical_path_with_base(
                ".ssh/id_rsa",
                Some(Path::new("/home/user"))
            )
            .is_ok()
        );
    }
}
