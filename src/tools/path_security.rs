use anyhow::Result;
use std::path::{Component, Path, PathBuf};

/// System-critical prefixes (Unix) — always blocked even if future allowlists match.
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

    // User requested Unix-only critical prefix hard blocks.
    if !cfg!(unix) {
        return Ok(());
    }

    let resolved = resolve_for_security(path, base_dir)?;
    for prefix in SYSTEM_BLOCKED_PREFIXES_UNIX {
        if resolved.starts_with(Path::new(prefix)) {
            anyhow::bail!(
                "Access to system-critical path is blocked by safety rule: {}",
                prefix
            );
        }
    }
    Ok(())
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

    if let Ok(canonical) = absolute.canonicalize() {
        return Ok(canonical);
    }

    if let Some(parent) = absolute.parent() {
        if let Ok(canonical_parent) = parent.canonicalize() {
            if let Some(name) = absolute.file_name() {
                return Ok(canonical_parent.join(name));
            }
            return Ok(canonical_parent);
        }
    }

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
    use super::{ensure_not_system_critical_path_with_base, SYSTEM_BLOCKED_PREFIXES_UNIX};
    use std::path::Path;

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
        let blocked =
            ensure_not_system_critical_path_with_base("/etc/ssh/sshd_config", None).is_err();
        assert!(blocked);
    }

    #[test]
    fn allows_non_system_paths() {
        if !cfg!(unix) {
            return;
        }
        let allowed =
            ensure_not_system_critical_path_with_base("src/main.rs", Some(Path::new("/workspace")))
                .is_ok();
        assert!(allowed);
    }

    #[test]
    fn does_not_block_partial_prefix_match() {
        if !cfg!(unix) {
            return;
        }
        let allowed = ensure_not_system_critical_path_with_base("/etc2/config", None).is_ok();
        assert!(allowed);
    }

    #[test]
    fn resolves_relative_parent_traversal_before_checking() {
        if !cfg!(unix) {
            return;
        }
        let blocked =
            ensure_not_system_critical_path_with_base("../etc/passwd", Some(Path::new("/")))
                .is_err();
        assert!(blocked);
    }
}
