//! Registre d'outils : définition, dispatch, résultats.
//!
//! Responsabilités : ToolDef, ToolResult, Tool et ToolRegistry.
//! Tous les builtin utilisent cette même abstraction; les outils composés
//! délèguent aux primitives plutôt que de créer un second runtime. Les outils
//! MCP sont découverts dynamiquement dans [`crate::tools::mcp::McpCatalog`]
//! et passent par la même surface de definitions/call.

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use agent_runtime::ToolConfigurationError;

use super::contracts::ToolCancellation;
use super::mcp::McpCatalog;

#[derive(Debug, Clone, Default)]
pub struct SandboxConfig {
    pub allowed_dirs: Vec<PathBuf>,
}

#[derive(Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters_fn: fn() -> Value,
}

impl std::fmt::Debug for ToolDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolDef")
            .field("name", &self.name)
            .field("description", &self.description)
            .finish()
    }
}

impl ToolDef {
    pub fn to_json(&self) -> Value {
        serde_json::json!({ "name": self.name, "description": self.description, "parameters": (self.parameters_fn)() })
    }
}

#[derive(Debug, Clone)]
pub enum ToolResult {
    Ok(String),
    Err(String),
}

impl ToolResult {
    pub fn is_ok(&self) -> bool {
        matches!(self, ToolResult::Ok(_))
    }
    pub fn to_history_text(&self) -> String {
        match self {
            ToolResult::Ok(s) => s.clone(),
            ToolResult::Err(e) => format!("[Erreur] {e}"),
        }
    }
}

#[async_trait::async_trait]
pub trait Tool: Send + Sync {
    fn definition(&self) -> &ToolDef;

    /// Executes the tool. `cancellation` is the session cancellation signal:
    /// long-running tools (shell, MCP) must observe it and abort promptly
    /// instead of running to their full timeout after a session/cancel.
    async fn execute(
        &self,
        args: &Value,
        cwd: &Path,
        allowed_dirs: &[PathBuf],
        cancellation: &ToolCancellation,
    ) -> ToolResult;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
    sandbox: SandboxConfig,
    mcp: Option<Arc<McpCatalog>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let names: Vec<&str> = self.tools.iter().map(|t| t.definition().name).collect();
        f.debug_struct("ToolRegistry")
            .field("tools", &names)
            .field("sandbox", &self.sandbox)
            .field("mcp", &self.mcp.as_ref().map(|catalog| catalog.has_tools()))
            .finish()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            sandbox: SandboxConfig::default(),
            mcp: None,
        }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.definition().name;
        if self
            .tools
            .iter()
            .any(|existing| existing.definition().name == name)
        {
            tracing::error!(name, "duplicate builtin tool identity rejected");
            return;
        }
        tracing::debug!(name, "outil enregistré");
        self.tools.push(tool);
    }

    pub fn register_mcp(&mut self, catalog: Arc<McpCatalog>) -> Result<(), ToolConfigurationError> {
        let builtin_names: std::collections::HashSet<&str> = self
            .tools
            .iter()
            .map(|tool| tool.definition().name)
            .collect();
        let conflicts: Vec<String> = catalog
            .definitions()
            .into_iter()
            .filter_map(|definition| {
                definition
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .filter(|name| builtin_names.contains(name.as_str()))
            .collect();
        if !conflicts.is_empty() {
            return Err(ToolConfigurationError::InvalidConfiguration(format!(
                "MCP tool identity collision with builtin(s): {}",
                conflicts.join(", ")
            )));
        }
        tracing::info!(tools = catalog.definitions().len(), "MCP tools registered");
        self.mcp = Some(catalog);
        Ok(())
    }

    fn register_builtins(&mut self) {
        self.register(Box::new(crate::tools::builtin::file::FileReadTool));
        self.register(Box::new(crate::tools::builtin::file::FileWriteTool));
        self.register(Box::new(crate::tools::builtin::file::FileEditTool));
        self.register(Box::new(crate::tools::builtin::filesystem::GlobTool));
        self.register(Box::new(
            crate::tools::builtin::filesystem::ListDirectoryTool,
        ));
        self.register(Box::new(crate::tools::builtin::shell::ShellExecTool));
        self.register(Box::new(crate::tools::builtin::search::SearchTool));
        self.register(Box::new(crate::tools::builtin::web_search::WebSearchTool));
        self.register(Box::new(crate::tools::builtin::composed::SearchAndReadTool));
        self.register(Box::new(crate::tools::builtin::composed::ReplaceInFileTool));
        self.register(Box::new(crate::tools::interactive::AskUserQuestionTool));
    }

    pub fn builtin() -> Self {
        let mut reg = Self::new();
        reg.register_builtins();
        reg
    }

    pub fn definitions(&self) -> Vec<Value> {
        let mut definitions = self
            .tools
            .iter()
            .map(|t| t.definition().to_json())
            .collect::<Vec<_>>();
        if let Some(mcp) = &self.mcp {
            definitions.extend(mcp.definitions());
        }
        definitions.sort_by(|a, b| {
            a.get("name")
                .and_then(Value::as_str)
                .cmp(&b.get("name").and_then(Value::as_str))
        });
        definitions
    }

    pub async fn call_async(
        &self,
        name: &str,
        args: &Value,
        cwd: &Path,
        extra_dirs: &[PathBuf],
        cancellation: &ToolCancellation,
    ) -> Option<ToolResult> {
        let mut allowed = self.sandbox.allowed_dirs.clone();
        for dir in extra_dirs {
            if !allowed.contains(dir) {
                allowed.push(dir.clone());
            }
        }

        if cancellation.is_cancelled() {
            return Some(ToolResult::Err(
                "outil annulé avant son démarrage (session/cancel)".into(),
            ));
        }

        // Builtins own their canonical identities. An MCP catalog can only
        // service names which are not already claimed by a builtin.
        if let Some(tool) = self.tools.iter().find(|t| t.definition().name == name) {
            return Some(tool.execute(args, cwd, &allowed, cancellation).await);
        }
        if let Some(mcp) = &self.mcp {
            // D-18 : même périmètre fusionné (sandbox + extra) que pour les
            // builtins — les deux types d'outils d'une même session ne doivent
            // pas avoir des périmètres d'accès différents.
            return mcp
                .call_async(name, args, cwd, &allowed, cancellation)
                .await;
        }
        None
    }

    pub fn has_tools(&self) -> bool {
        !self.tools.is_empty() || self.mcp.as_ref().is_some_and(|catalog| catalog.has_tools())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builtin_has_executable_tools_only() {
        let reg = ToolRegistry::builtin();
        let defs = reg.definitions();
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|d| d.get("name").and_then(Value::as_str))
            .collect();
        for expected in [
            "file_read",
            "file_write",
            "file_edit",
            "glob",
            "list_directory",
            "shell_exec",
            "search",
            "web_search",
            "search_and_read",
            "replace_in_file",
            "AskUserQuestion",
        ] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        assert!(
            !names.contains(&"FollowUp"),
            "FollowUp must not be an executable tool"
        );
    }

    #[test]
    fn builtin_definitions_are_sorted_deterministically() {
        let reg = ToolRegistry::builtin();
        let names: Vec<String> = reg
            .definitions()
            .iter()
            .filter_map(|definition| {
                definition
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }
}

/// SPEC-P1-04 acceptance: a `session/cancel` during a long MCP tool call
/// returns in well under 2 seconds instead of waiting the full request
/// timeout. A fake stdio MCP server sleeps 30 s before answering tools/call.
#[tokio::test]
async fn session_cancel_interrupts_a_long_mcp_tool_call_within_two_seconds() {
    use crate::tools::contracts::ToolCancellation;
    use crate::tools::mcp::{McpCatalog, McpServerConfig, McpTransportKind};
    use crate::tools::registry::ToolRegistry;
    use std::path::Path;
    use std::time::{Duration, Instant};

    let script =
        std::env::temp_dir().join(format!("fake-mcp-{}.py", uuid::Uuid::new_v4().simple()));
    std::fs::write(
        &script,
        r#"import sys, json, time
for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    req = json.loads(line)
    resp_id = req.get("id")
    if resp_id is None:
        continue
    method = req.get("method", "")
    if method == "tools/call":
        time.sleep(30)
        result = {"content": [{"type": "text", "text": "done"}]}
    elif method == "tools/list":
        result = {"tools": [{"name": "slow", "description": "slow tool", "inputSchema": {"type": "object"}}]}
    else:
        result = {}
    print(json.dumps({"jsonrpc": "2.0", "id": resp_id, "result": result}), flush=True)
"#,
    )
    .expect("fake MCP server script must be written");

    let catalog = McpCatalog::from_configs(vec![McpServerConfig {
        name: "fake".into(),
        transport: McpTransportKind::Stdio,
        command: Some("python3".into()),
        args: vec![script.display().to_string()],
        env: Default::default(),
        cwd: None,
        url: None,
        headers: Default::default(),
    }])
    .await
    .expect("fake MCP catalog must build");
    assert!(catalog.has_tools());

    let mut registry = ToolRegistry::new();
    registry
        .register_mcp(std::sync::Arc::new(catalog))
        .expect("registration must succeed");

    let (tx, rx) = tokio::sync::watch::channel(false);
    let cancellation = ToolCancellation::from_receiver(rx);

    let canceller = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = tx.send(true);
    });

    let start = Instant::now();
    let result = registry
        .call_async(
            "mcp__fake__slow",
            &serde_json::json!({}),
            Path::new("/tmp"),
            &[],
            &cancellation,
        )
        .await;
    canceller.await.expect("canceller task must finish");
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(2),
        "cancelled MCP call must return promptly, took {elapsed:?}"
    );
    assert!(
        matches!(&result, Some(crate::tools::registry::ToolResult::Err(message)) if message.contains("annulé")),
        "cancelled MCP call must be an error result, got {result:?}"
    );
    std::fs::remove_file(&script).ok();
}
