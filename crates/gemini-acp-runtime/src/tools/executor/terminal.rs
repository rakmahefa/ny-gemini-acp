use agent_client_protocol::schema::v1::{
    CreateTerminalRequest, ReleaseTerminalRequest, Terminal, TerminalId,
    TerminalOutputRequest, ToolCallContent, ToolCallId, ToolCallStatus, WaitForTerminalExitRequest,
};
use serde_json::{Map, Value};

use super::super::lifecycle::{session_cancelled, wait_for_session_cancel, ToolLifecycle, ToolResultEnvelope};
use super::super::sandbox::ShellSandbox;
use super::{ExecutionOutcome, ToolExecutor, ToolResult};

impl<'a> ToolExecutor<'a> {
    pub(super) async fn execute_shell_via_acp_terminal(
        &self,
        arguments: &Value,
        call_id: &ToolCallId,
        lifecycle: &ToolLifecycle,
    ) -> anyhow::Result<ExecutionOutcome> {
        let command = arguments.get("command").and_then(Value::as_str).filter(|v| !v.trim().is_empty()).ok_or_else(|| anyhow::anyhow!("paramètre 'command' manquant ou vide"))?;
        ShellSandbox::new().analyze_command(command).map_err(|e| anyhow::anyhow!(e.to_string()))?;
        let timeout = arguments.get("timeout").and_then(Value::as_u64).unwrap_or(30).clamp(1, 120);
        let request = CreateTerminalRequest::new(self.session_id.clone(), "sh").args(vec!["-c".to_owned(), command.to_owned()]).cwd(self.cwd.to_path_buf()).output_byte_limit(64 * 1024);
        let response = self.cx.send_request(request).block_task().await?;
        let terminal_id = response.terminal_id;
        self.emit_update(call_id, ToolCallStatus::InProgress, vec![ToolCallContent::Terminal(Terminal::new(terminal_id.clone()))], vec![], Some(terminal_lifecycle_meta(&terminal_id.0, None, None)));

        let wait = WaitForTerminalExitRequest::new(self.session_id.clone(), terminal_id.clone());
        let wait_result = tokio::select! {
            result = tokio::time::timeout(std::time::Duration::from_secs(timeout + 5), self.cx.send_request(wait).block_task()) => result,
            _ = wait_for_session_cancel(self.session_id.0.as_ref()) => {
                let partial = self.fetch_terminal_output(&terminal_id).await;
                let _ = self.cx.send_request(ReleaseTerminalRequest::new(self.session_id.clone(), terminal_id.clone())).block_task().await;
                let terminal_text = terminal_output_text(partial);
                if !terminal_text.is_empty() { self.emit_partial_result(call_id, lifecycle, "shell_exec", arguments, &terminal_text); }
                let content = if terminal_text.is_empty() { "terminal annulé par session/cancel".to_owned() } else { terminal_text.clone() };
                return Ok(ExecutionOutcome { result: ToolResult::err(content), terminal_id: Some(terminal_id.0.to_string()), terminal_meta: Some(terminal_lifecycle_meta(&terminal_id.0, (!terminal_text.is_empty()).then_some(terminal_text.as_str()), None)), cancelled: true });
            }
        };

        let cancelled_after_wait = session_cancelled(self.session_id.0.as_ref());
        let (exit_code, signal, wait_error) = match wait_result { Ok(Ok(response)) => (response.exit_status.exit_code, response.exit_status.signal, None), Ok(Err(error)) => (None, None, Some(error.to_string())), Err(_) => (None, None, Some(format!("terminal timeout après {timeout}s"))) };
        let (output, truncated) = self.fetch_terminal_output(&terminal_id).await;
        let _ = self.cx.send_request(ReleaseTerminalRequest::new(self.session_id.clone(), terminal_id.clone())).block_task().await;
        let terminal_text = match &wait_error { Some(error) if output.trim().is_empty() => error.clone(), _ if output.trim().is_empty() => match exit_code { Some(code) => format!("exit code {code}"), None => "(sortie vide)".to_owned() }, _ if truncated => format!("{output}\n… (sortie tronquée par le client ACP)"), _ => output };
        let cancelled = cancelled_after_wait || session_cancelled(self.session_id.0.as_ref());
        if cancelled && !terminal_text.is_empty() { self.emit_partial_result(call_id, lifecycle, "shell_exec", arguments, &terminal_text); }
        let is_ok = !cancelled && wait_error.is_none() && signal.is_none() && exit_code.unwrap_or(0) == 0;
        Ok(ExecutionOutcome { result: ToolResult { content: terminal_text.clone(), is_ok }, terminal_id: Some(terminal_id.0.to_string()), terminal_meta: Some(terminal_lifecycle_meta(&terminal_id.0, Some(&terminal_text), Some((exit_code, signal.as_deref())))), cancelled })
    }

    fn emit_partial_result(&self, call_id: &ToolCallId, lifecycle: &ToolLifecycle, tool_name: &str, arguments: &Value, content: &str) {
        if content.is_empty() { return; }
        let envelope = ToolResultEnvelope::new(tool_name, content, ToolCallStatus::InProgress, lifecycle.sequence());
        let rendered = super::super::tool_ux::result_update(tool_name, arguments, &envelope.content, false, self.cwd, None);
        let meta = serde_json::json!({ "result": { "terminal": false, "sequence": envelope.sequence } });
        let _ = self.emit_update(call_id, envelope.status, rendered.content, rendered.locations, Some(meta.as_object().cloned().unwrap_or_default()));
    }

    pub(super) async fn fetch_terminal_output(&self, terminal_id: &TerminalId) -> (String, bool) {
        match self.cx.send_request(TerminalOutputRequest::new(self.session_id.clone(), terminal_id.clone())).block_task().await { Ok(response) => (response.output, response.truncated), Err(error) => (format!("terminal output indisponible: {error}"), false) }
    }
}

pub(super) fn terminal_output_text((output, truncated): (String, bool)) -> String { if output.trim().is_empty() { String::new() } else if truncated { format!("{output}\n… (sortie tronquée par le client ACP)") } else { output } }

pub(super) fn terminal_lifecycle_meta(terminal_id: &str, output: Option<&str>, exit: Option<(Option<u32>, Option<&str>)>) -> Map<String, Value> {
    let mut meta = Map::new();
    meta.insert("terminal_info".into(), serde_json::json!({ "terminal_id": terminal_id }));
    if let Some(output) = output { let preview: String = output.chars().take(16_384).collect(); meta.insert("terminal_output".into(), serde_json::json!({ "terminal_id": terminal_id, "data": preview })); }
    if let Some((exit_code, signal)) = exit { meta.insert("terminal_exit".into(), serde_json::json!({ "terminal_id": terminal_id, "exit_code": exit_code.map(i64::from).unwrap_or(-1), "signal": signal })); }
    meta
}
