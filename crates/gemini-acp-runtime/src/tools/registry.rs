//! Registre d'outils : définition, dispatch, résultats.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tool_provider::{ToolProvider as ToolProviderContract, ToolRequest, ToolResponse};

use super::mcp::McpCatalog;

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    pub allowed_dirs: Vec<PathBuf>,
    #[allow(dead_code)]
    pub shell_sandbox_enabled: bool,
}
impl Default for SandboxConfig {
    fn default() -> Self { Self { allowed_dirs: Vec::new(), shell_sandbox_enabled: true } }
}

#[derive(Clone)]
pub struct ToolDef { pub name: &'static str, pub description: &'static str, pub parameters_fn: fn() -> Value }
impl std::fmt::Debug for ToolDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("ToolDef").field("name", &self.name).field("description", &self.description).finish() }
}
impl ToolDef {
    pub fn to_json(&self) -> Value { serde_json::json!({ "name": self.name, "description": self.description, "parameters": (self.parameters_fn)() }) }
}

#[derive(Debug, Clone)]
pub enum ToolResult { Ok(String), Err(String) }
impl ToolResult {
    pub fn is_ok(&self) -> bool { matches!(self, ToolResult::Ok(_)) }
    pub fn to_history_text(&self) -> String { match self { ToolResult::Ok(s) => s.clone(), ToolResult::Err(e) => format!("[Erreur] {e}") } }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDef;
    async fn execute(&self, args: &Value, cwd: &Path, allowed_dirs: &[PathBuf]) -> ToolResult;
}

pub struct ToolRegistry { tools: Vec<Box<dyn Tool>>, sandbox: SandboxConfig, mcp: Option<Arc<McpCatalog>> }
impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.definition().name).collect();
        f.debug_struct("ToolRegistry").field("tools", &names).field("sandbox", &self.sandbox).field("mcp", &self.mcp.as_ref().map(|catalog| catalog.has_tools())).finish()
    }
}
impl Default for ToolRegistry { fn default() -> Self { Self::new() } }
impl ToolRegistry {
    pub fn new() -> Self { Self { tools: Vec::new(), sandbox: SandboxConfig::default(), mcp: None } }
    #[allow(dead_code)] pub fn with_sandbox(sandbox: SandboxConfig) -> Self { Self { tools: Vec::new(), sandbox, mcp: None } }
    pub fn register(&mut self, tool: Box<dyn Tool>) { tracing::debug!(name = tool.definition().name, "outil enregistré"); self.tools.push(tool); }
    pub fn register_mcp(&mut self, catalog: Arc<McpCatalog>) { tracing::info!(tools = catalog.definitions().len(), "MCP tools registered"); self.mcp = Some(catalog); }
    fn register_builtins(&mut self) {
        self.register(Box::new(crate::tools::builtin::file::FileReadTool));
        self.register(Box::new(crate::tools::builtin::file::FileWriteTool));
        self.register(Box::new(crate::tools::builtin::file::FileEditTool));
        self.register(Box::new(crate::tools::builtin::filesystem::GlobTool));
        self.register(Box::new(crate::tools::builtin::shell::ShellExecTool));
        self.register(Box::new(crate::tools::builtin::search::SearchTool));
        self.register(Box::new(crate::tools::builtin::composed::SearchAndReadTool));
        self.register(Box::new(crate::tools::builtin::composed::ReplaceInFileTool));
        self.register(Box::new(crate::tools::interactive::AskUserQuestionTool));
    }
    pub fn builtin() -> Self { let mut reg = Self::new(); reg.register_builtins(); reg }
    #[allow(dead_code)] pub fn builtin_with_sandbox(sandbox: SandboxConfig) -> Self { let mut reg = Self::with_sandbox(sandbox); reg.register_builtins(); reg }
    pub fn definitions(&self) -> Vec<Value> { let mut definitions = self.tools.iter().map(|t| t.definition().to_json()).collect::<Vec<_>>(); if let Some(mcp) = &self.mcp { definitions.extend(mcp.definitions()); } definitions }
    #[allow(dead_code)] pub fn sandbox(&self) -> &SandboxConfig { &self.sandbox }
    pub async fn call_async(&self, name: &str, args: &Value, cwd: &Path, extra_dirs: &[PathBuf]) -> Option<ToolResult> {
        if let Some(mcp) = &self.mcp { if let Some(result) = mcp.call_async(name, args, cwd, extra_dirs).await { return Some(result); } }
        let tool = self.tools.iter().find(|t| t.definition().name == name)?;
        let mut allowed = self.sandbox.allowed_dirs.clone();
        for dir in extra_dirs { if !allowed.contains(dir) { allowed.push(dir.clone()); } }
        Some(tool.execute(args, cwd, &allowed).await)
    }
    pub fn has_tools(&self) -> bool { !self.tools.is_empty() || self.mcp.as_ref().is_some_and(|catalog| catalog.has_tools()) }
}

#[async_trait::async_trait]
impl ToolProviderContract for ToolRegistry {
    fn definitions(&self) -> Vec<Value> { self.definitions() }
    fn has_tools(&self) -> bool { self.has_tools() }
    async fn execute(&self, request: ToolRequest) -> Option<ToolResponse> {
        self.call_async(&request.name, &request.arguments, &request.cwd, &request.additional_directories)
            .await
            .map(|result| ToolResponse { content: result.to_history_text(), is_error: !result.is_ok() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn registry_builtin_has_executable_tools_only() {
        let reg = ToolRegistry::builtin();
        let defs = reg.definitions();
        let names: Vec<&str> = defs.iter().filter_map(|d| d.get("name").and_then(Value::as_str)).collect();
        for expected in ["file_read", "file_write", "file_edit", "glob", "shell_exec", "search", "search_and_read", "replace_in_file", "AskUserQuestion"] { assert!(names.contains(&expected), "missing {expected}"); }
        assert!(!names.contains(&"FollowUp"), "FollowUp must not be an executable tool");
    }
}
