use std::sync::Arc;

use agent_client_protocol::schema::v1::{SessionId, ToolCallId};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::state::{Session, SessionMode, SessionPermissionRule, Store};
use agent_runtime::{
    Cancellation, ToolPermissionDecision, ToolPermissionHandler, ToolPermissionRequest,
};
use tokio::sync::Mutex;
use tools_provider::tools::executor::{
    PermissionKind, PermissionRequest, PermissionResult, ToolExecutor,
};

pub(crate) struct AcpToolPermissionHandler {
    cx: ConnectionTo<Client>,
    store: Arc<Store>,
    /// Mid-turn memory of "always allow/reject" choices. `needs_permission`
    /// receives the turn's session copy, which does not see rules persisted
    /// by `update_session` during the same turn — this covers that gap, while
    /// the session rules cover subsequent turns.
    memory: Arc<Mutex<Vec<SessionPermissionRule>>>,
}

impl AcpToolPermissionHandler {
    pub(crate) fn new(cx: ConnectionTo<Client>, store: Arc<Store>) -> Self {
        Self {
            cx,
            store,
            memory: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn permission_request(&self, request: &ToolPermissionRequest) -> PermissionRequest {
        PermissionRequest::from_tool_call(&request.name, &request.arguments, &request.cwd)
    }
}

/// Single rule lookup shared by `needs_permission` and `request_permission`.
/// Returns `Some(true)` for a matching always-allow rule, `Some(false)` for a
/// matching always-reject rule, `None` when no rule covers the pair.
pub(crate) fn lookup_session_rule(
    rules: &[SessionPermissionRule],
    tool_name: &str,
    kind: PermissionKind,
) -> Option<bool> {
    rules
        .iter()
        .find(|rule| rule.tool == tool_name && rule.kind == kind.label())
        .map(|rule| rule.allow)
}

fn record_rule(rules: &mut Vec<SessionPermissionRule>, rule: SessionPermissionRule) {
    rules.retain(|existing| !(existing.tool == rule.tool && existing.kind == rule.kind));
    rules.push(rule);
}

#[async_trait::async_trait]
impl ToolPermissionHandler for AcpToolPermissionHandler {
    fn needs_permission(&self, session: &Session, request: &ToolPermissionRequest) -> bool {
        let permission = self.permission_request(request);
        // A remembered "always reject" must still route through
        // `request_permission` (which rejects without prompting), so only an
        // "always allow" rule can skip the permission path here.
        let remembered = lookup_session_rule(
            &session.permission_rules,
            &permission.tool_name,
            permission.kind,
        )
        .or_else(|| {
            self.memory.try_lock().ok().and_then(|memory| {
                lookup_session_rule(&memory, &permission.tool_name, permission.kind)
            })
        });
        if remembered == Some(true) {
            return false;
        }
        match permission.kind {
            PermissionKind::Read => false,
            PermissionKind::Write | PermissionKind::Execute => match session.mode {
                SessionMode::BypassPermissions => false,
                SessionMode::AcceptEdits => {
                    permission.kind == PermissionKind::Execute
                        && permission.risk >= tools_provider::tools::sandbox::RiskLevel::High
                }
                SessionMode::Default => true,
            },
        }
    }

    async fn request_permission(
        &self,
        session: &Session,
        request: &ToolPermissionRequest,
        cancellation: Cancellation,
    ) -> ToolPermissionDecision {
        let permission = self.permission_request(request);
        // A remembered decision is enforced without contacting the client.
        let remembered = lookup_session_rule(
            &session.permission_rules,
            &permission.tool_name,
            permission.kind,
        )
        .or_else(|| {
            self.memory.try_lock().ok().and_then(|memory| {
                lookup_session_rule(&memory, &permission.tool_name, permission.kind)
            })
        });
        if remembered == Some(false) {
            return ToolPermissionDecision::Reject(format!(
                "{} ({}) refusé : « Toujours refuser » a été mémorisé pour cet outil dans cette session.",
                permission.kind.label(),
                permission.summary
            ));
        }

        let session_id = SessionId::from(request.session_id.clone());
        let executor = ToolExecutor::new(
            &self.cx,
            &session_id,
            &request.cwd,
            cancellation.clone().subscribe(),
        );
        match executor
            .request_permission(&permission, &ToolCallId::from(request.call_id.clone()))
            .await
        {
            PermissionResult::Allow => ToolPermissionDecision::Allow,
            PermissionResult::AllowAlways => {
                let rule = SessionPermissionRule {
                    tool: permission.tool_name.clone(),
                    kind: permission.kind.label().to_string(),
                    allow: true,
                };
                self.remember(&request.session_id, rule).await;
                ToolPermissionDecision::Allow
            }
            PermissionResult::Reject(message) => ToolPermissionDecision::Reject(message),
            PermissionResult::RejectAlways => {
                let rule = SessionPermissionRule {
                    tool: permission.tool_name.clone(),
                    kind: permission.kind.label().to_string(),
                    allow: false,
                };
                self.remember(&request.session_id, rule).await;
                ToolPermissionDecision::Reject(format!(
                    "{} ({}) refusé par l'utilisateur (mémorisé pour cette session).",
                    permission.kind.label(),
                    permission.summary
                ))
            }
            PermissionResult::Cancelled => ToolPermissionDecision::Cancelled,
            PermissionResult::TransportError(error) => ToolPermissionDecision::Reject(format!(
                "Échec de la demande de permission ACP : {error}"
            )),
        }
    }
}

impl AcpToolPermissionHandler {
    /// Records an "always allow/reject" choice in the mid-turn memory and
    /// persists it through the store (session-scoped, never cross-session).
    async fn remember(&self, session_id: &str, rule: SessionPermissionRule) {
        if let Ok(mut memory) = self.memory.try_lock() {
            record_rule(&mut memory, rule.clone());
        }
        if let Err(error) = self
            .store
            .update_session(session_id, |session| {
                record_rule(&mut session.permission_rules, rule.clone());
            })
            .await
        {
            tracing::warn!(session = %session_id, tool = %rule.tool, error = %error,
                "failed to persist session permission rule");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(tool: &str, kind: &str, allow: bool) -> SessionPermissionRule {
        SessionPermissionRule {
            tool: tool.to_string(),
            kind: kind.to_string(),
            allow,
        }
    }

    #[test]
    fn always_allow_rule_is_found_for_the_matching_tool_and_kind() {
        let rules = vec![rule("shell_exec", "execute", true)];
        assert_eq!(
            lookup_session_rule(&rules, "shell_exec", PermissionKind::Execute),
            Some(true)
        );
        assert_eq!(
            lookup_session_rule(&rules, "file_write", PermissionKind::Write),
            None,
            "rules are tool-scoped"
        );
        assert_eq!(
            lookup_session_rule(&rules, "shell_exec", PermissionKind::Write),
            None,
            "rules are kind-scoped"
        );
    }

    #[test]
    fn always_reject_rule_is_found_and_distinct_from_allow() {
        let rules = vec![rule("shell_exec", "execute", false)];
        assert_eq!(
            lookup_session_rule(&rules, "shell_exec", PermissionKind::Execute),
            Some(false)
        );
    }

    #[test]
    fn recording_a_new_rule_replaces_the_previous_one_for_the_pair() {
        let mut rules = vec![rule("shell_exec", "execute", false)];
        record_rule(&mut rules, rule("shell_exec", "execute", true));
        assert_eq!(
            lookup_session_rule(&rules, "shell_exec", PermissionKind::Execute),
            Some(true),
            "a new decision for the same (tool, kind) pair replaces the old rule"
        );
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn permission_rules_are_session_scoped_in_serialization() {
        // Old persisted sessions (without the field) load with no rules.
        let legacy = serde_json::json!({
            "id": "sess_00000000000000000000000000000000",
            "cwd": "/tmp",
            "additional_directories": [],
            "title": null,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "model": "m",
            "think": null,
            "tools_enabled": true,
            "messages": []
        });
        let session: Session = serde_json::from_value(legacy).expect("legacy session must load");
        assert!(session.permission_rules.is_empty());
    }
}
