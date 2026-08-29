//! Builtin terminal tool.
//!
//! The shell implementation remains on the existing `Tool` / `ToolRegistry`
//! architecture. It adds bounded execution, explicit security analysis,
//! deterministic output formatting, and process-tree cleanup on timeout.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::tools::registry::{Tool, ToolDef, ToolResult};
use crate::tools::sandbox;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 120;
const MAX_OUTPUT_BYTES: usize = 64 * 1024;

fn shell_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "command": {
                "type": "string",
                "description": "Commande shell à exécuter dans le répertoire de travail"
            },
            "timeout": {
                "type": "integer",
                "minimum": 1,
                "maximum": 120,
                "description": "Timeout en secondes. Défaut: 30, maximum: 120."
            }
        },
        "required": ["command"]
    })
}

fn shell_def() -> ToolDef {
    ToolDef {
        name: "shell_exec",
        description: "Exécute une commande shell dans le répertoire de travail avec sandbox, timeout et sortie bornée.",
        parameters_fn: shell_params,
    }
}

pub struct ShellExecTool;

#[async_trait::async_trait]
impl Tool for ShellExecTool {
    fn definition(&self) -> &ToolDef {
        static DEF: std::sync::OnceLock<ToolDef> = std::sync::OnceLock::new();
        DEF.get_or_init(shell_def)
    }

    async fn execute(&self, args: &Value, cwd: &Path, _allowed_dirs: &[PathBuf]) -> ToolResult {
        let command = match args.get("command").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value,
            _ => return ToolResult::Err("paramètre 'command' manquant ou vide".into()),
        };

        let timeout_secs = args
            .get("timeout")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, MAX_TIMEOUT_SECS);

        let analysis = match sandbox::ShellSandbox::new().analyze_command(command) {
            Ok(analysis) => analysis,
            Err(error) => {
                tracing::warn!(command = %command, error = %error, "commande shell bloquée");
                return ToolResult::Err(error.to_string());
            }
        };

        tracing::info!(
            command = %command,
            cwd = %cwd.display(),
            timeout_secs,
            risk = %analysis.risk,
            "shell_exec"
        );

        let mut command_builder = tokio::process::Command::new("sh");
        command_builder
            .arg("-c")
            .arg(command)
            .current_dir(cwd)
            .kill_on_drop(true)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command_builder.process_group(0);
        }

        let child = match command_builder.spawn() {
            Ok(child) => child,
            Err(error) => return ToolResult::Err(format!("échec du démarrage du shell: {error}")),
        };
        let process_group_id = child.id();

        let execution =
            tokio::time::timeout(Duration::from_secs(timeout_secs), child.wait_with_output());

        match execution.await {
            Ok(Ok(output)) => format_shell_output(&output, &analysis),
            Ok(Err(error)) => ToolResult::Err(format!("échec de l'exécution: {error}")),
            Err(_) => {
                #[cfg(unix)]
                if let Some(pid) = process_group_id {
                    let rc = unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
                    if rc != 0 {
                        tracing::debug!(pid, "impossible de tuer immédiatement le process group shell");
                    }
                }
                ToolResult::Err(format!(
                    "timeout après {timeout_secs}s; arbre de processus interrompu"
                ))
            }
        }
    }
}

fn format_shell_output(
    output: &std::process::Output,
    analysis: &sandbox::ShellAnalysis,
) -> ToolResult {
    let mut body = String::new();

    if !output.stdout.is_empty() {
        body.push_str(&String::from_utf8_lossy(&output.stdout));
    }

    if !output.stderr.is_empty() {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str("[stderr]\n");
        body.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    if body.is_empty() {
        body.push_str("(sortie vide)");
    }

    let truncated = truncate_utf8(&mut body, MAX_OUTPUT_BYTES);
    if truncated {
        body.push_str("\n… (sortie tronquée)");
    }

    let status = match output.status.code() {
        Some(code) => format!("exit code {code}"),
        None => "processus terminé par signal".to_string(),
    };

    let mut result = format!("[{status}]\n{body}");
    if truncated {
        result.push_str(&format!("\n[output_limit={} bytes]", MAX_OUTPUT_BYTES));
    }
    result.push_str(&format!("\n[risk={}]", analysis.risk.label()));

    match output.status.code() {
        Some(0) => ToolResult::Ok(result),
        Some(_) | None => ToolResult::Err(result),
    }
}

fn truncate_utf8(value: &mut String, max_bytes: usize) -> bool {
    if value.len() <= max_bytes {
        return false;
    }

    let cut = value
        .char_indices()
        .take_while(|(index, _)| *index < max_bytes)
        .map(|(index, _)| index)
        .last()
        .unwrap_or(0);
    value.truncate(cut);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn shell_echo() {
        let result = ShellExecTool
            .execute(&json!({"command": "echo hello"}), Path::new("/tmp"), &[])
            .await;

        assert!(matches!(result, ToolResult::Ok(output) if output.contains("hello") && output.contains("exit code 0")));
    }

    #[tokio::test]
    async fn shell_non_zero_exit_is_an_error_and_preserves_output() {
        let result = ShellExecTool
            .execute(
                &json!({"command": "git --no-such-option status"}),
                Path::new("/tmp"),
                &[],
            )
            .await;

        assert!(matches!(
            result,
            ToolResult::Err(output)
                if output.contains("exit code") && output.contains("[stderr]")
        ));
    }

    #[test]
    fn shell_signal_termination_is_an_error() {
        let output = std::process::Command::new("sh")
            .args(["-c", "kill -TERM $$"])
            .output()
            .expect("signal test process must start");
        let analysis = sandbox::ShellSandbox::new()
            .analyze_command("echo hello")
            .expect("safe analysis fixture must parse");

        assert!(matches!(format_shell_output(&output, &analysis), ToolResult::Err(error) if error.contains("signal")));
    }

    #[tokio::test]
    async fn shell_timeout_interrupts_process_tree() {
        let result = ShellExecTool
            .execute(
                &json!({"command": "sleep 60", "timeout": 1}),
                Path::new("/tmp"),
                &[],
            )
            .await;

        assert!(matches!(result, ToolResult::Err(error) if error.contains("arbre de processus")));
    }

    #[tokio::test]
    async fn shell_blocks_dangerous_command() {
        let result = ShellExecTool
            .execute(&json!({"command": "sudo rm -rf /"}), Path::new("/tmp"), &[])
            .await;

        assert!(matches!(result, ToolResult::Err(error) if error.contains("Sécurité")));
    }

    #[tokio::test]
    async fn shell_blocks_system_shutdown() {
        let result = ShellExecTool
            .execute(&json!({"command": "shutdown now"}), Path::new("/tmp"), &[])
            .await;

        assert!(matches!(result, ToolResult::Err(error) if error.contains("Sécurité")));
    }

    #[tokio::test]
    async fn shell_allows_git() {
        let result = ShellExecTool
            .execute(&json!({"command": "git status"}), Path::new("/tmp"), &[])
            .await;

        assert!(!matches!(result, ToolResult::Err(error) if error.contains("Sécurité")));
    }

    #[test]
    fn truncate_preserves_utf8() {
        let mut value = "é".repeat(8);
        assert!(truncate_utf8(&mut value, 9));
        assert_eq!(value, "é".repeat(4));
    }

    #[tokio::test]
    async fn shell_reports_risk_metadata() {
        let result = ShellExecTool
            .execute(&json!({"command": "ls"}), Path::new("/tmp"), &[])
            .await;

        assert!(matches!(result, ToolResult::Ok(output) if output.contains("[risk=low]")));
    }
}
