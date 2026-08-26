//! Filesystem scope validation for tool paths.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SecurityError(pub String);

impl std::fmt::Display for SecurityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[Sécurité] {}", self.0)
    }
}

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

    let canonical = if resolved.exists() {
        resolved
            .canonicalize()
            .map_err(|e| SecurityError(format!("chemin invalide {} : {e}", resolved.display())))?
    } else {
        normalize_path(&resolved)
    };

    let cwd_canon = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
    if path_starts_with(&canonical, &cwd_canon) {
        return Ok(canonical);
    }

    for dir in allowed_dirs {
        let dir_canon = dir.canonicalize().unwrap_or_else(|_| dir.clone());
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

fn path_starts_with(child: &Path, parent: &Path) -> bool {
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
