//! Shell command normalization and execution policy.
//!
//! **Honest security statement**: this module is a *heuristic allowlist
//! filter*, not an OS-level confinement. Commands run inside `sh -c` with
//! full user privileges. Every refusal message below must therefore state
//! that no OS confinement backs the decision. Arbitrary-execution vectors
//! known today (git config/alias hooks, `ext::` transports, `sed`'s `e`
//! capability, `tar` post-archive hooks, `find` destructive predicates) are
//! blocked on a best-effort basis at argument-validation time — see
//! `docs/adr/0001-sandbox-execution.md`.

use std::collections::HashSet;
use std::path::Path;

use super::parser::{parse_shell, ParsedShellCommand};
use super::path::SecurityError;
use super::risk::{RiskLevel, ShellAnalysis};

#[derive(Clone, Debug)]
pub struct ShellSandbox {
    allowed_programs: HashSet<&'static str>,
    blocked_programs: HashSet<&'static str>,
}

impl Default for ShellSandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellSandbox {
    pub fn new() -> Self {
        Self {
            allowed_programs: allowed_programs(),
            blocked_programs: blocked_programs(),
        }
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub fn permissive() -> Self {
        Self {
            allowed_programs: HashSet::new(),
            blocked_programs: HashSet::new(),
        }
    }

    pub fn analyze_command(&self, command: &str) -> Result<ShellAnalysis, SecurityError> {
        let parsed = parse_shell(command).map_err(|error| SecurityError(error.to_string()))?;
        if self.is_permissive() {
            return Ok(ShellAnalysis::from_parsed(command, &parsed));
        }
        self.validate_structure(&parsed)?;
        let analysis = ShellAnalysis::from_parsed(command, &parsed);
        self.validate_programs(&parsed)?;
        self.validate_arguments(&parsed)?;
        Ok(analysis)
    }

    pub fn validate(&self, command: &str) -> Result<(), SecurityError> {
        self.analyze_command(command).map(|_| ())
    }

    pub fn normalize(&self, command: &str) -> Result<String, SecurityError> {
        let parsed = parse_shell(command).map_err(|error| SecurityError(error.to_string()))?;
        if self.is_permissive() {
            return Ok(parsed.normalized());
        }
        self.validate_structure(&parsed)?;
        self.validate_programs(&parsed)?;
        self.validate_arguments(&parsed)?;
        Ok(parsed.normalized())
    }

    fn is_permissive(&self) -> bool {
        self.allowed_programs.is_empty() && self.blocked_programs.is_empty()
    }

    /// Single classification entry point: validation first, then risk.
    /// A command that would be refused by the sandbox is classified
    /// `Critical` so permission policies always treat it as worst-case.
    pub fn classify(&self, command: &str) -> RiskLevel {
        self.analyze_command(command)
            .map(|analysis| analysis.risk)
            .unwrap_or(RiskLevel::Critical)
    }

    fn validate_structure(&self, parsed: &ParsedShellCommand) -> Result<(), SecurityError> {
        if parsed.has_environment_expansion {
            return Err(SecurityError(
                "expansion de variable shell non autorisée par le filtre heuristique de la sandbox (sans confinement OS)".into(),
            ));
        }
        if parsed.has_non_pipe_operator() {
            return Err(SecurityError(
                "seul l'opérateur pipe '|' est autorisé par le filtre heuristique de la sandbox (sans confinement OS)".into(),
            ));
        }
        Ok(())
    }

    fn validate_programs(&self, parsed: &ParsedShellCommand) -> Result<(), SecurityError> {
        for segment in &parsed.segments {
            let program = command_name(&segment.program)?;
            if self.blocked_programs.contains(program.as_str()) {
                return Err(SecurityError(format!(
                    "commande bloquée par la politique sandbox : '{program}'"
                )));
            }
            if !self.allowed_programs.is_empty()
                && !self.allowed_programs.contains(program.as_str())
            {
                return Err(SecurityError(format!(
                    "commande non autorisée par le filtre heuristique de la sandbox (sans confinement OS) : '{program}'"
                )));
            }
        }
        Ok(())
    }

    fn validate_arguments(&self, parsed: &ParsedShellCommand) -> Result<(), SecurityError> {
        for segment in &parsed.segments {
            let program = command_name(&segment.program)?;
            let args = &segment.args;

            if dynamic_capability_program(program.as_str()) {
                return Err(SecurityError(format!(
                    "programme '{program}' interdit sans confinement OS : la politique applicative ne peut pas garantir son périmètre"
                )));
            }

            if args.iter().any(|arg| {
                arg == "/" || arg.starts_with('/') || arg.starts_with('~') || arg.contains("../")
            }) {
                return Err(SecurityError(
                    "chemin absolu ou traversal hors périmètre interdit dans une commande shell sans confinement OS".into(),
                ));
            }

            if args.iter().any(|arg| {
                arg.split_once('=').is_some_and(|(_, value)| {
                    value.starts_with('/') || value.starts_with('~') || value.contains("../")
                })
            }) {
                return Err(SecurityError(
                    "option contenant un chemin absolu ou hors périmètre interdite par le filtre heuristique de la sandbox (sans confinement OS)".into(),
                ));
            }

            // Arbitrary-execution vector (audit chap. 3.1): a `!` inside an
            // argument value reaches shell hooks. Covers `git -c alias.x='!cmd'`,
            // `core.fsmonitor=!cmd` and every git/hook variant; the character is
            // never needed by a legitimate read-oriented workload.
            if args.iter().any(|arg| arg.contains('!')) {
                return Err(SecurityError(
                    "caractère '!' interdit dans les arguments : vecteur d'exécution arbitraire via hooks (alias git, fsmonitor)".into(),
                ));
            }

            // Arbitrary-execution vector: `git clone ext::sh -c cmd` style
            // transports hand the command line to an arbitrary program.
            if args.iter().any(|arg| arg.starts_with("ext::")) {
                return Err(SecurityError(
                    "transport 'ext::' interdit : il délègue l'exécution à un programme arbitraire"
                        .into(),
                ));
            }

            match program.as_str() {
                // `git -c key=value` overrides reach shell-executing config
                // hooks (core.pager, core.editor, core.fsmonitor,
                // gpg.program, diff.external, ...). Filtering every executing
                // key is not tractable, so config overrides are refused whole.
                "git"
                    if args.iter().any(|arg| {
                        arg == "-c" || arg.starts_with("--config") || arg.starts_with("--exec-path")
                    }) =>
                {
                    return Err(SecurityError(
                        "git -c/--config/--exec-path interdits sans confinement OS : ces surcharges de configuration peuvent exécuter des programmes arbitraires (pager, editor, fsmonitor, hooks)".into(),
                    ));
                }
                // GNU sed can execute the pattern space through `sh` via the
                // `e` command / `s///e` flag, with delimiters and quoting
                // variants that defeat fine-grained filtering. Fail closed:
                // sed is removed from the allowlist (same posture as awk).
                "sed" => {
                    return Err(SecurityError(
                        "sed interdit sans confinement OS : ses capacités d'exécution embarquées (commande 'e', drapeau s///e) ne sont pas filtrables finement".into(),
                    ));
                }
                // tar post-archive hooks execute arbitrary commands.
                "tar"
                    if args.iter().any(|arg| {
                        arg.starts_with("--to-command") || arg.starts_with("--checkpoint-action")
                    }) =>
                {
                    return Err(SecurityError(
                        "tar --to-command/--checkpoint-action interdits : hooks d'exécution arbitraire".into(),
                    ));
                }
                // find destructive/dynamic predicates: `-delete` is a bare rm;
                // -exec/-execdir were already refused; -ok/-okdir prompt-execute;
                // -fls/-fprint* write files outside the read-only contract.
                "find"
                    if args.iter().any(|arg| {
                        matches!(
                            arg.as_str(),
                            "-delete"
                                | "-exec"
                                | "-execdir"
                                | "-ok"
                                | "-okdir"
                                | "-fls"
                                | "-fprint"
                                | "-fprint0"
                                | "-fprintf"
                        )
                    }) =>
                {
                    return Err(SecurityError(
                        "find avec prédicat destructeur ou d'exécution interdit : find ne passe qu'en lecture".into(),
                    ));
                }
                _ => {}
            }

            if matches!(program.as_str(), "sh" | "bash" | "zsh" | "dash" | "ksh") {
                return Err(SecurityError(format!(
                    "interpréteur shell '{program}' interdit dans la sandbox"
                )));
            }

            if args
                .iter()
                .any(|arg| matches!(arg.as_str(), "-exec" | "-execdir"))
            {
                return Err(SecurityError(
                    "find/xargs avec exécution dynamique interdits dans la sandbox".into(),
                ));
            }

            if matches!(program.as_str(), "xargs") {
                return Err(SecurityError(
                    "xargs interdit sans confinement OS : il peut déléguer l'exécution à des programmes arbitraires".into(),
                ));
            }

            if matches!(program.as_str(), "rm" | "rmdir" | "chmod" | "chown") {
                return Err(SecurityError(format!(
                    "commande mutante '{program}' interdite sans confinement OS"
                )));
            }

            if args.iter().any(|arg| arg == "--no-preserve-root") {
                return Err(SecurityError(
                    "option '--no-preserve-root' interdite dans la sandbox".into(),
                ));
            }
        }
        Ok(())
    }
}

fn dynamic_capability_program(program: &str) -> bool {
    matches!(
        program,
        "python"
            | "python2"
            | "python3"
            | "perl"
            | "ruby"
            | "node"
            | "nodejs"
            | "awk"
            | "sed"
            | "lua"
            | "php"
            | "java"
            | "js"
            | "deno"
            | "bun"
            | "cargo"
            | "rustc"
            | "rustup"
            | "go"
            | "gcc"
            | "g++"
            | "clang"
            | "make"
            | "cmake"
            | "npm"
            | "npx"
            | "pnpm"
            | "yarn"
            | "pip"
            | "pip3"
            | "docker"
            | "docker-compose"
            | "podman"
            | "uv"
    )
}

fn command_name(program: &str) -> Result<String, SecurityError> {
    if program.is_empty() {
        return Err(SecurityError("programme shell vide".into()));
    }
    if program.contains('=') {
        return Err(SecurityError(
            "affectation d'environnement en tête de commande interdite".into(),
        ));
    }
    if program.contains('/') || Path::new(program).is_absolute() {
        return Err(SecurityError(
            "exécution via chemin de programme explicite interdite".into(),
        ));
    }
    Ok(program.to_owned())
}

fn allowed_programs() -> HashSet<&'static str> {
    // `sed` is deliberately absent: its embedded `e` execution capability is
    // not finely filterable (see validate_arguments).
    [
        "cat", "head", "tail", "less", "ls", "find", "tree", "grep", "rg", "ag", "echo", "printf",
        "cd", "pwd", "mkdir", "cp", "mv", "git", "gh", "jq", "yq", "wc", "sort", "uniq", "tr",
        "cut", "date", "whoami", "id", "printenv", "basename", "dirname", "realpath", "readlink",
        "diff", "patch", "tar", "zip", "unzip", "gzip", "gunzip", "which", "file", "stat", "sleep",
        "test", "true", "false",
    ]
    .into_iter()
    .collect()
}

fn blocked_programs() -> HashSet<&'static str> {
    [
        "sudo",
        "su",
        "doas",
        "mkfs",
        "dd",
        "shutdown",
        "reboot",
        "halt",
        "poweroff",
        "mount",
        "umount",
        "kill",
        "curl",
        "wget",
        "nc",
        "ncat",
        "socat",
        "crontab",
        "systemctl",
        "service",
        "eval",
        "exec",
        "env",
        "command",
        "type",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dynamic_programs_fail_closed() {
        for command in [
            "python script.py",
            "node script.js",
            "awk '{print $1}'",
            "cargo test",
        ] {
            assert!(
                ShellSandbox::new().validate(command).is_err(),
                "must reject {command}"
            );
        }
    }

    #[test]
    fn direct_path_escape_is_rejected() {
        for command in [
            "cat /etc/passwd",
            "cat ../secret",
            "git --git-dir=/etc status",
        ] {
            assert!(
                ShellSandbox::new().validate(command).is_err(),
                "must reject {command}"
            );
        }
    }
}
