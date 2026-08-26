//! Shell command normalization and execution policy.

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
    pub fn permissive() -> Self {
        Self {
            allowed_programs: HashSet::new(),
            blocked_programs: HashSet::new(),
        }
    }

    pub fn analyze_command(&self, command: &str) -> Result<ShellAnalysis, SecurityError> {
        let parsed = parse_shell(command).map_err(|error| SecurityError(error.to_string()))?;
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
        self.validate_structure(&parsed)?;
        self.validate_programs(&parsed)?;
        self.validate_arguments(&parsed)?;
        Ok(parsed.normalized())
    }

    fn validate_structure(&self, parsed: &ParsedShellCommand) -> Result<(), SecurityError> {
        if parsed.has_environment_expansion {
            return Err(SecurityError(
                "expansion de variable shell non autorisée dans la sandbox".into(),
            ));
        }
        if parsed.has_non_pipe_operator() {
            return Err(SecurityError(
                "seul l'opérateur pipe '|' est autorisé dans la sandbox".into(),
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
            if !self.allowed_programs.is_empty() && !self.allowed_programs.contains(program.as_str()) {
                return Err(SecurityError(format!(
                    "commande non autorisée par la politique sandbox : '{program}'"
                )));
            }
        }
        Ok(())
    }

    fn validate_arguments(&self, parsed: &ParsedShellCommand) -> Result<(), SecurityError> {
        for segment in &parsed.segments {
            let program = command_name(&segment.program)?;
            let args = &segment.args;

            if matches!(program.as_str(), "sh" | "bash" | "zsh" | "dash" | "ksh") {
                return Err(SecurityError(format!(
                    "interpréteur shell '{program}' interdit dans la sandbox"
                )));
            }

            if matches!(program.as_str(), "python" | "python2" | "python3" | "perl" | "ruby" | "node")
                && args.iter().any(|arg| matches!(arg.as_str(), "-c" | "-e" | "-E"))
            {
                return Err(SecurityError(format!(
                    "exécution de code inline interdite pour '{program}'"
                )));
            }

            if args.iter().any(|arg| matches!(arg.as_str(), "-exec" | "-execdir")) {
                return Err(SecurityError(
                    "find/xargs avec exécution dynamique interdits dans la sandbox".into(),
                ));
            }

            if matches!(program.as_str(), "xargs")
                && args.iter().any(|arg| {
                    matches!(arg.as_str(), "sh" | "bash" | "zsh" | "dash" | "ksh" | "python" | "python3")
                })
            {
                return Err(SecurityError("xargs vers un interpréteur est interdit".into()));
            }

            if matches!(program.as_str(), "rm" | "rmdir" | "chmod" | "chown") {
                if args.iter().any(|arg| arg == "/" || is_absolute_path(arg) || arg.contains("../")) {
                    return Err(SecurityError(format!(
                        "cible absolue ou hors périmètre interdite pour '{program}'"
                    )));
                }
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

fn is_absolute_path(value: &str) -> bool {
    value.starts_with('/') || value.starts_with('~')
}

fn allowed_programs() -> HashSet<&'static str> {
    [
        "cat", "head", "tail", "less", "ls", "find", "tree", "grep", "rg", "ag", "awk",
        "sed", "echo", "printf", "cd", "pwd", "mkdir", "cp", "mv", "rm", "touch", "chmod",
        "chown", "git", "gh", "cargo", "rustc", "rustup", "node", "npm", "npx", "pnpm",
        "yarn", "bun", "python", "python3", "pip", "pip3", "go", "gcc", "g++", "clang", "make",
        "cmake", "docker", "docker-compose", "podman", "jq", "yq", "wc", "sort", "uniq", "tr",
        "cut", "xargs", "date", "whoami", "id", "env", "printenv", "basename", "dirname",
        "realpath", "readlink", "diff", "patch", "tar", "zip", "unzip", "gzip", "gunzip",
        "which", "command", "type", "file", "stat", "sleep", "uv", "test", "true", "false",
    ]
    .into_iter()
    .collect()
}

fn blocked_programs() -> HashSet<&'static str> {
    [
        "sudo", "su", "doas", "mkfs", "dd", "shutdown", "reboot", "halt", "poweroff", "mount",
        "umount", "kill", "curl", "wget", "nc", "ncat", "socat", "crontab", "systemctl", "service",
        "eval", "exec",
    ]
    .into_iter()
    .collect()
}

pub(crate) fn risk_for_command(command: &str) -> RiskLevel {
    ShellAnalysis::analyze(command).risk
}
