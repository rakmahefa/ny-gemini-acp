//! Native filesystem discovery builtins: `glob` and `list_directory`.

use crate::tools::contracts::ToolCancellation;
use crate::tools::{
    registry::{Tool, ToolDef, ToolResult},
    sandbox,
};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;
const MAX_RESULTS: usize = 500;
const MAX_ENTRIES: usize = 2_000;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(10);
pub struct GlobTool;
pub struct ListDirectoryTool;
fn glob_def() -> ToolDef {
    ToolDef {
        name: "glob",
        description: "Find filesystem paths matching a glob pattern within the allowed workspace.",
        parameters_fn: || json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"},"max_results":{"type":"integer","minimum":1,"maximum":500}},"required":["pattern"]}),
    }
}
fn list_directory_def() -> ToolDef {
    ToolDef {
        name: "list_directory",
        description: "List the direct children of a directory with stable, bounded output.",
        parameters_fn: || json!({"type":"object","properties":{"path":{"type":"string"}}}),
    }
}
#[async_trait::async_trait]
impl Tool for GlobTool {
    fn definition(&self) -> &ToolDef {
        static DEF: std::sync::OnceLock<ToolDef> = std::sync::OnceLock::new();
        DEF.get_or_init(glob_def)
    }
    async fn execute(
        &self,
        args: &Value,
        cwd: &Path,
        allowed_dirs: &[PathBuf],
        _cancellation: &ToolCancellation,
    ) -> ToolResult {
        let pattern = match args.get("pattern").and_then(Value::as_str) {
            Some(pattern) if !pattern.trim().is_empty() => pattern,
            _ => return ToolResult::Err("paramètre 'pattern' manquant ou vide".into()),
        };
        let root = match args
            .get("path")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            Some(path) => match sandbox::validate_path(path, cwd, allowed_dirs) {
                Ok(path) => path,
                Err(error) => return ToolResult::Err(error.to_string()),
            },
            None => cwd.to_path_buf(),
        };
        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(100)
            .clamp(1, MAX_RESULTS as u64) as usize;
        match tokio::time::timeout(
            DISCOVERY_TIMEOUT,
            collect_glob(root, pattern.replace('\\', "/"), max_results),
        )
        .await
        {
            Ok(Ok(paths)) => {
                if paths.is_empty() {
                    ToolResult::Ok("Aucun chemin correspondant.".into())
                } else {
                    ToolResult::Ok(format_paths(paths))
                }
            }
            Ok(Err(error)) => ToolResult::Err(error),
            Err(_) => ToolResult::Err(format!(
                "glob interrompu après {}s",
                DISCOVERY_TIMEOUT.as_secs()
            )),
        }
    }
}
#[async_trait::async_trait]
impl Tool for ListDirectoryTool {
    fn definition(&self) -> &ToolDef {
        static DEF: std::sync::OnceLock<ToolDef> = std::sync::OnceLock::new();
        DEF.get_or_init(list_directory_def)
    }
    async fn execute(
        &self,
        args: &Value,
        cwd: &Path,
        allowed_dirs: &[PathBuf],
        _cancellation: &ToolCancellation,
    ) -> ToolResult {
        let root = match args
            .get("path")
            .and_then(Value::as_str)
            .filter(|v| !v.is_empty())
        {
            Some(path) => match sandbox::validate_path(path, cwd, allowed_dirs) {
                Ok(path) => path,
                Err(error) => return ToolResult::Err(error.to_string()),
            },
            None => cwd.to_path_buf(),
        };
        match tokio::time::timeout(DISCOVERY_TIMEOUT, list_directory(root)).await {
            Ok(Ok(output)) => ToolResult::Ok(output),
            Ok(Err(error)) => ToolResult::Err(error),
            Err(_) => ToolResult::Err(format!(
                "list_directory interrompu après {}s",
                DISCOVERY_TIMEOUT.as_secs()
            )),
        }
    }
}
async fn collect_glob(
    root: PathBuf,
    pattern: String,
    max_results: usize,
) -> Result<Vec<PathBuf>, String> {
    let metadata = tokio::fs::metadata(&root)
        .await
        .map_err(|e| format!("chemin introuvable {}: {e}", root.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} n'est pas un répertoire", root.display()));
    }
    let mut stack = vec![root.clone()];
    let mut matches = Vec::new();
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| format!("lecture impossible {}: {e}", dir.display()))?;
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("lecture impossible {}: {e}", dir.display()))?
        {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if is_ignored_dir(&name) {
                continue;
            }
            let file_type = entry
                .file_type()
                .await
                .map_err(|e| format!("type impossible {}: {e}", path.display()))?;
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if glob_matches(&pattern, &relative, &name) {
                matches.push(path.clone());
                if matches.len() >= max_results {
                    return Ok(matches);
                }
            }
            if file_type.is_dir() && stack.len() < MAX_ENTRIES {
                stack.push(path);
            }
            if matches.len() >= MAX_RESULTS {
                break;
            }
        }
    }
    matches.sort();
    Ok(matches)
}
async fn list_directory(root: PathBuf) -> Result<String, String> {
    let metadata = tokio::fs::metadata(&root)
        .await
        .map_err(|e| format!("chemin introuvable {}: {e}", root.display()))?;
    if !metadata.is_dir() {
        return Err(format!("{} n'est pas un répertoire", root.display()));
    }
    let mut entries = Vec::new();
    let mut dir = tokio::fs::read_dir(&root)
        .await
        .map_err(|e| format!("lecture impossible {}: {e}", root.display()))?;
    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(|e| format!("lecture impossible {}: {e}", root.display()))?
    {
        let file_type = entry
            .file_type()
            .await
            .map_err(|e| format!("type impossible {}: {e}", entry.path().display()))?;
        let kind = if file_type.is_dir() {
            "dir"
        } else if file_type.is_file() {
            "file"
        } else {
            "other"
        };
        entries.push(format!("{kind}\t{}", entry.file_name().to_string_lossy()));
        if entries.len() >= MAX_ENTRIES {
            break;
        }
    }
    entries.sort();
    if entries.is_empty() {
        return Ok("Répertoire vide.".into());
    }
    let truncated = entries.len() >= MAX_ENTRIES;
    let mut output = entries.join("\n");
    if truncated {
        output.push_str("\n… résultats tronqués");
    }
    Ok(output)
}
fn is_ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".venv" | "__pycache__"
    )
}
fn glob_matches(pattern: &str, relative: &str, basename: &str) -> bool {
    let shared = crate::tools::glob::glob_matches;
    shared(pattern, relative) || shared(pattern, basename)
}
fn format_paths(paths: Vec<PathBuf>) -> String {
    paths
        .into_iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join("\n")
}
#[cfg(test)]
mod tests {

    fn no_cancel() -> ToolCancellation {
        let (_tx, rx) = tokio::sync::watch::channel(false);
        ToolCancellation::from_receiver(rx)
    }

    use super::*;
    #[tokio::test]
    async fn glob_finds_matching_files() {
        let dir = std::env::temp_dir().join(format!("acp-glob-{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::create_dir_all(dir.join("src")).await.unwrap();
        tokio::fs::write(dir.join("src/lib.rs"), "pub fn x() {}\n")
            .await
            .unwrap();
        tokio::fs::write(dir.join("src/lib.txt"), "x\n")
            .await
            .unwrap();
        let result = GlobTool
            .execute(&json!({"pattern":"**/*.rs"}), &dir, &[], &no_cancel())
            .await;
        assert!(matches!(&result, ToolResult::Ok(value) if value.contains("lib.rs")));
        assert!(!matches!(&result, ToolResult::Ok(value) if value.contains("lib.txt")));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
    #[tokio::test]
    async fn list_directory_is_stable() {
        let dir = std::env::temp_dir().join(format!("acp-list-{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        tokio::fs::write(dir.join("b.txt"), "b").await.unwrap();
        tokio::fs::create_dir(dir.join("a-dir")).await.unwrap();
        let result = ListDirectoryTool
            .execute(&json!({}), &dir, &[], &no_cancel())
            .await;
        assert!(
            matches!(result, ToolResult::Ok(value) if value.starts_with("dir\ta-dir\nfile\tb.txt"))
        );
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
    #[tokio::test]
    async fn traversal_is_blocked() {
        let dir = std::env::temp_dir().join(format!("acp-fs-{}", uuid::Uuid::new_v4().simple()));
        tokio::fs::create_dir_all(&dir).await.unwrap();
        let result = GlobTool
            .execute(
                &json!({"pattern":"*","path":"/etc"}),
                &dir,
                &[],
                &no_cancel(),
            )
            .await;
        assert!(matches!(result, ToolResult::Err(error) if error.contains("Sécurité")));
        let _ = tokio::fs::remove_dir_all(dir).await;
    }
}
