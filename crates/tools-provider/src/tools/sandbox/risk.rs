//! Risk classification for normalized shell commands.

use super::parser::{parse_shell, ParsedShellCommand, ShellOperator};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Low => "Lecture ou inspection — effets limités",
            Self::Medium => "Pipeline ou plusieurs opérations — effets locaux possibles",
            Self::High => "Écriture, compilation, package ou outil à effets de bord",
            Self::Critical => "Suppression massive, escalade ou exécution dynamique",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Low => "✅",
            Self::Medium => "⚠️",
            Self::High => "🛑",
            Self::Critical => "🔴",
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
pub struct ShellAnalysis {
    pub risk: RiskLevel,
    pub commands: Vec<String>,
    pub has_pipes: bool,
    pub has_env_injection: bool,
    pub has_dangerous_pipe_chain: bool,
    pub risk_description: String,
    pub line_count: usize,
}

impl ShellAnalysis {
    pub fn analyze(command: &str) -> Self {
        let parsed = parse_shell(command);
        match parsed {
            Ok(parsed) => Self::from_parsed(command, &parsed),
            Err(error) => Self {
                risk: RiskLevel::Critical,
                commands: Vec::new(),
                has_pipes: command.contains('|'),
                has_env_injection: command.contains('$') || command.contains('`'),
                has_dangerous_pipe_chain: true,
                risk_description: format!("commande non analysable : {error}"),
                line_count: command.lines().filter(|line| !line.trim().is_empty()).count(),
            },
        }
    }

    pub(crate) fn from_parsed(command: &str, parsed: &ParsedShellCommand) -> Self {
        let commands = parsed
            .segments
            .iter()
            .map(|segment| segment.normalized())
            .collect::<Vec<_>>();
        let has_pipes = parsed
            .operators
            .iter()
            .any(|operator| matches!(operator, ShellOperator::Pipe));
        let has_dangerous_pipe_chain = parsed.has_non_pipe_operator();
        let has_env_injection = parsed.has_environment_expansion;
        let risk = compute_risk(parsed, has_env_injection, has_dangerous_pipe_chain);
        let line_count = command.lines().filter(|line| !line.trim().is_empty()).count();
        let risk_description = build_risk_description(
            &risk,
            has_pipes,
            has_env_injection,
            has_dangerous_pipe_chain,
        );
        Self {
            risk,
            commands,
            has_pipes,
            has_env_injection,
            has_dangerous_pipe_chain,
            risk_description,
            line_count,
        }
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![format!("{} risk", self.risk.label())];
        if self.has_pipes {
            parts.push(format!("{} commands in pipe chain", self.commands.len()));
        } else if self.line_count > 1 {
            parts.push(format!("{} lines", self.line_count));
        }
        if self.has_env_injection {
            parts.push("env expansion detected".to_owned());
        }
        if self.has_dangerous_pipe_chain {
            parts.push("non-pipe shell operator".to_owned());
        }
        parts.join(" — ")
    }
}

fn compute_risk(
    parsed: &ParsedShellCommand,
    has_env_expansion: bool,
    has_non_pipe_operator: bool,
) -> RiskLevel {
    if has_non_pipe_operator {
        return RiskLevel::Critical;
    }

    let mut risk = if parsed.segments.len() > 1 {
        RiskLevel::Medium
    } else {
        RiskLevel::Low
    };

    for segment in &parsed.segments {
        let program = segment.program.rsplit('/').next().unwrap_or(&segment.program);
        if matches!(program, "rm" | "rmdir" | "chmod" | "chown")
            && segment.args.iter().any(|arg| arg == "/" || arg == "-rf" || arg == "-fr")
        {
            return RiskLevel::Critical;
        }
        if matches!(
            program,
            "rm" | "rmdir" | "mv" | "cp" | "chmod" | "chown" | "docker" | "podman"
                | "npm" | "npx" | "pnpm" | "yarn" | "bun" | "pip" | "pip3" | "cargo"
                | "go" | "make" | "cmake" | "gcc" | "g++" | "clang" | "patch"
        ) {
            risk = risk.max(RiskLevel::High);
        }
    }

    if has_env_expansion {
        risk = risk.max(if parsed.segments.len() > 1 {
            RiskLevel::High
        } else {
            RiskLevel::Medium
        });
    }

    risk
}

fn build_risk_description(
    risk: &RiskLevel,
    has_pipes: bool,
    has_env_expansion: bool,
    has_non_pipe_operator: bool,
) -> String {
    let mut parts = vec![risk.description().to_owned()];
    if has_pipes {
        parts.push("Pipeline analysé segment par segment".to_owned());
    }
    if has_non_pipe_operator {
        parts.push("Opérateur shell non-pipe détecté".to_owned());
    }
    if has_env_expansion {
        parts.push("Expansion de variable d'environnement détectée".to_owned());
    }
    parts.join(". ")
}
