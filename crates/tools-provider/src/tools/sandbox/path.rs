//! Filesystem scope validation for tool paths.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SecurityError(pub String);

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[Sécurité] {}", self.0)
    }
}

/// Validate a path against the session CWD and explicit additional directories.
///
/// Existing symlinks are rejected at every component so a checked path cannot
/// silently escape the declared scope through a pre-existing symbolic link.
/// The final path may be non-existent, but every existing ancestor must be a
/// real directory. This removes the immediate symlink bypass while leaving a
/// platform-level openat/O_NOFOLLOW-style mechanism as the definitive TOCTOU
/// hardening step.
pub fn validate_path(
    raw: &str,
    cwd: &Path,
    allowed_dirs: &[PathBuf],
) -> Result<PathBuf, SecurityError> {
    let path = Path::new(raw);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    };

    reject_symlink_components(&resolved)?;

    let canonical = if resolved.exists() {
        resolved
            .canonicalize()
            .map_err(|e| SecurityError(format!("chemin invalide {} : {e}", resolved.display())))?
    } else {
        normalize_path(&resolved)
    };

    let cwd_canon = canonical_scope(cwd)?;
    if path_starts_with(&canonical, &cwd_canon) {
        return Ok(canonical);
    }

    for dir in allowed_dirs {
        let dir_canon = canonical_scope(dir)?;
        if path_starts_with(&canonical, &dir_canon) {
            return Ok(canonical);
        }
    }

    Err(SecurityError(format!(
        "chemin {} hors du périmètre autorisé (CWD={}, allowed_dirs={})",
        canonical.display(),
        cwd_canon.display(),
        allowed_dirs
            .iter()
            .map(|d| d.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    )))
}

fn canonical_scope(path: &Path) -> Result<PathBuf, SecurityError> {
    reject_symlink_components(path)?;
    path.canonicalize().map_err(|error| {
        SecurityError(format!(
            "périmètre autorisé inaccessible {} : {error}",
            path.display()
        ))
    })
}

fn reject_symlink_components(path: &Path) -> Result<(), SecurityError> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new("/")),
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
            }
            Component::Normal(part) => {
                current.push(part);
                match std::fs::symlink_metadata(&current) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(SecurityError(format!(
                            "lien symbolique interdit dans le chemin : {}",
                            current.display()
                        )));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(error) => {
                        return Err(SecurityError(format!(
                            "impossible d'inspecter {} : {error}",
                            current.display()
                        )));
                    }
                }
            }
        }
    }
    Ok(())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    normalized.iter().collect()
}

pub(super) fn path_starts_with(child: &Path, parent: &Path) -> bool {
    let mut child_components = child.components();
    let mut parent_components = parent.components();
    loop {
        match (parent_components.next(), child_components.next()) {
            (None, _) => return true,
            (Some(_), None) => return false,
            (Some(a), Some(b)) if a != b => return false,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_traversal_is_blocked() {
        let result = validate_path("../../etc/passwd", Path::new("/tmp/workspace"), &[]);
        assert!(result.is_err());
    }

    #[test]
    fn sibling_prefix_is_not_accepted() {
        assert!(!path_starts_with(Path::new("/tmp/workspace2"), Path::new("/tmp/workspace")));
    }

    #[test]
    fn existing_symlink_component_is_rejected() {
        let root = std::env::temp_dir().join(format!("acp-sandbox-symlink-{}", uuid::Uuid::new_v4().simple()));
        let target = root.join("target");
        let link = root.join("link");
        std::fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        let result = validate_path("link/file.txt", &root, &[]);
        assert!(matches!(result, Err(SecurityError(message)) if message.contains("lien symbolique")));

        let _ = std::fs::remove_dir_all(&root);
    }
}
