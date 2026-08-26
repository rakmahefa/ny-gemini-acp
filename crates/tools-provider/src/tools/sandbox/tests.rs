use super::*;

#[test]
fn analysis_backtick_is_critical() {
    let analysis = ShellAnalysis::analyze("echo `whoami`");
    assert!(analysis.has_env_injection);
    assert_eq!(analysis.risk, RiskLevel::Critical);
}

#[test]
fn parser_ignores_trailing_comment() {
    let parsed = parse_shell("git status\n# trailing comment\n").unwrap();
    assert_eq!(parsed.segments[0].program, "git");
    assert!(parsed.operators.is_empty());
}
