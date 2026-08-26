//! Shell command sandbox and allow/block policy.

use regex::Regex;

use super::risk::ShellAnalysis;
use super::path::SecurityError;

#[derive(Clone)]
pub struct ShellSandbox {
    blocked_patterns: Vec<Regex>,
    allowed_prefixes: Vec<&'static str>,
    dangerous_pipe_patterns: Vec<Regex>,
}

impl Default for ShellSandbox {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellSandbox {
    fn get() -> Self {
        static SANDBOX: std::sync::LazyLock<ShellSandbox> = std::sync::LazyLock::new(ShellSandbox::build);
        SANDBOX.clone()
    }

    pub fn new() -> Self {
        Self::get()
    }

    fn build() -> Self {
        let blocked = [
            r"(?i)\brm\s+(-[a-zA-Z]*f[a-zA-Z]*\s+)?/",
            r"(?i)\bmkfs\b",
            r"(?i)\bdd\s+if=.*of=/",
            r"(?i)\bchmod\s+(-R\s+)?777\s+/",
            r"(?i)\bchown\s+(-R\s+)?\S+\s+/",
            r"(?i)\b(shutdown|reboot|halt|poweroff)\b",
            r"(?i)\b(umount|mount)\s+/",
            r"(?i)\bkill\s+(-9\s+)?1\b",
            r"(?i)\bsudo\s+",
            r"(?i)\bsu\s+",
            r"(?i)\bdoas\s+",
            r"(?i)\b(curl|wget)\s+",
            r"(?i)\b(nc|ncat|socat)\b",
            r"(?i)\b(crontab|systemctl|service)\b",
            r"(?i)\b(ba)?sh\s+-c\b",
            r"(?i)\bzsh\s+-c\b",
            r"(?i)\bpython[23]?\s+-c\b",
            r"(?i)\bperl\s+(-e|-E)\b",
            r"(?i)\bruby\s+-e\b",
            r"(?i)\bnode\s+-e\b",
            r"(?i)\beval\s+",
            r"(?i)\bexec\s+",
        ];
        let blocked_patterns = blocked
            .iter()
            .map(|p| Regex::new(p).expect("regex statique de sandbox invalide"))
            .collect();

        let dangerous_pipe_patterns = [
            r"(?i)\|\s*(sh|bash|zsh|dash|ksh)\b",
            r"(?i)\|\s*(python[23]?|perl|ruby|node)\b",
            r"(?i)-exec\s+(sh|bash|zsh|dash)\s+-c",
            r"(?i)xargs\s+(sh|bash|zsh|dash|ksh)\b",
            r"(?i)>\s*/(proc|dev|sys)/",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("regex pipe statique invalide"))
        .collect();

        let allowed_prefixes = vec![
            "cat ", "head ", "tail ", "less ", "ls ", "find ", "tree ", "grep ", "rg ",
            "ag ", "awk ", "sed ", "echo ", "printf ", "cd ", "pwd ", "mkdir ", "cp ",
            "mv ", "rm ", "touch ", "chmod ", "chown ", "git ", "gh ", "cargo ", "rustc ",
            "rustup ", "node ", "npm ", "npx ", "pnpm ", "yarn ", "bun ", "python ",
            "python3 ", "pip ", "pip3 ", "go ", "gcc ", "g++ ", "clang ", "make ",
            "cmake ", "docker ", "docker-compose ", "podman ", "jq ", "yq ", "wc ", "sort ",
            "uniq ", "tr ", "cut ", "xargs ", "date ", "whoami ", "id ", "env ", "printenv ",
            "export ", "basename ", "dirname ", "realpath ", "readlink ", "diff ", "patch ",
            "tar ", "zip ", "unzip ", "gzip ", "gunzip ", "which ", "command ", "type ",
            "file ", "stat ", "sleep ", "uv ", "test ", "[ ", "true ", "false ",
        ];

        Self { blocked_patterns, allowed_prefixes, dangerous_pipe_patterns }
    }

    #[allow(dead_code)]
    pub fn permissive() -> Self {
        Self {
            blocked_patterns: Vec::new(),
            allowed_prefixes: Vec::new(),
            dangerous_pipe_patterns: Vec::new(),
        }
    }

    pub fn analyze_command(&self, command: &str) -> Result<ShellAnalysis, SecurityError> {
        for line in command.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            for re in &self.blocked_patterns {
                if re.is_match(trimmed) {
                    return Err(SecurityError(format!(
                        "commande bloquée par la sandbox : {}",
                        trimmed
                    )));
                }
            }
        }

        for line in command.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            for re in &self.dangerous_pipe_patterns {
                if re.is_match(trimmed) {
                    return Err(SecurityError(format!(
                        "chaîne de pipes dangereuse bloquée par la sandbox : {}",
                        trimmed
                    )));
                }
            }
        }

        if !self.allowed_prefixes.is_empty() {
            for line in command.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                let first_word = trimmed.split_whitespace().next().unwrap_or("");
                let allowed = self.allowed_prefixes.iter().any(|p| p.trim() == first_word);
                if !allowed {
                    return Err(SecurityError(format!(
                        "commande non autorisée : '{}'. Commandes autorisées : {}",
                        first_word,
                        self.allowed_prefixes
                            .iter()
                            .map(|s| s.trim())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                }
            }
        }

        Ok(ShellAnalysis::analyze(command))
    }

    pub fn validate(&self, command: &str) -> Result<(), SecurityError> {
        self.analyze_command(command).map(|_| ())
    }
}
