use agent_client_protocol::schema::v1::ToolCallStatus;
use gemini_acp_runtime::tools::lifecycle::{LifecycleError, ToolLifecycle, ToolLifecycleState};

fn finish(lifecycle: &mut ToolLifecycle, is_ok: bool, cancelled: bool) -> ToolCallStatus {
    lifecycle
        .finish_with_result("test_tool", "terminal output", is_ok, cancelled)
        .expect("terminal path must produce one result envelope")
        .status
}

#[test]
fn pending_cancellation_is_terminal_once() {
    let mut lifecycle = ToolLifecycle::new();
    assert_eq!(finish(&mut lifecycle, false, true), ToolCallStatus::Failed);
    assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
    assert!(lifecycle.result_is_terminal());
    assert_eq!(
        lifecycle.finish_with_result("test_tool", "late", true, false),
        Err(LifecycleError::ResultAlreadyTerminal)
    );
}

#[test]
fn permission_rejection_is_terminal_once() {
    let mut lifecycle = ToolLifecycle::new();
    lifecycle.transition(ToolLifecycleState::Permission).unwrap();
    assert_eq!(finish(&mut lifecycle, false, false), ToolCallStatus::Failed);
    assert_eq!(lifecycle.state(), ToolLifecycleState::Failed);
    assert_eq!(
        lifecycle.finish_with_result("test_tool", "late", false, false),
        Err(LifecycleError::ResultAlreadyTerminal)
    );
}

#[test]
fn permission_cancellation_is_terminal_once() {
    let mut lifecycle = ToolLifecycle::new();
    lifecycle.transition(ToolLifecycleState::Permission).unwrap();
    assert_eq!(finish(&mut lifecycle, false, true), ToolCallStatus::Failed);
    assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
    assert_eq!(
        lifecycle.finish_with_result("test_tool", "late", true, false),
        Err(LifecycleError::ResultAlreadyTerminal)
    );
}

#[test]
fn cancellation_after_permission_allow_is_terminal_once() {
    let mut lifecycle = ToolLifecycle::new();
    lifecycle.transition(ToolLifecycleState::Permission).unwrap();
    assert_eq!(finish(&mut lifecycle, false, true), ToolCallStatus::Failed);
    assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
    assert_eq!(lifecycle.sequence(), 2);
    assert_eq!(
        lifecycle.finish_with_result("test_tool", "late execution", true, false),
        Err(LifecycleError::ResultAlreadyTerminal)
    );
}

#[test]
fn pre_execution_cancellation_without_permission_is_terminal_once() {
    let mut lifecycle = ToolLifecycle::new();
    assert_eq!(finish(&mut lifecycle, false, true), ToolCallStatus::Failed);
    assert_eq!(lifecycle.state(), ToolLifecycleState::Cancelled);
    assert_eq!(lifecycle.sequence(), 1);
    assert_eq!(
        lifecycle.finish_with_result("test_tool", "late execution", true, false),
        Err(LifecycleError::ResultAlreadyTerminal)
    );
}

#[test]
fn executing_terminal_outcomes_are_each_single_shot() {
    for (is_ok, cancelled, expected) in [
        (true, false, ToolCallStatus::Completed),
        (false, false, ToolCallStatus::Failed),
        (true, true, ToolCallStatus::Failed),
    ] {
        let mut lifecycle = ToolLifecycle::new();
        lifecycle.transition(ToolLifecycleState::Executing).unwrap();
        assert_eq!(finish(&mut lifecycle, is_ok, cancelled), expected);
        assert!(lifecycle.result_is_terminal());
        assert_eq!(
            lifecycle.finish_with_result("test_tool", "secondary", true, false),
            Err(LifecycleError::ResultAlreadyTerminal)
        );
    }
}

#[test]
fn executor_has_one_canonical_terminal_emission() {
    let source = include_str!("../src/tools/executor/mod.rs");
    assert!(!source.contains("emit_failed("));

    let start = source.find("fn finish_terminal(").unwrap();
    let helper = &source[start..];
    assert_eq!(helper.matches("self.emit_update(").count(), 1);
    assert!(helper.contains("finish_with_result("));
}
