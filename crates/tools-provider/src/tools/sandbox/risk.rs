//! Risk classification and shell command analysis.

/// Niveau de risque d'une opération outil.
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
            Self::Low => "Lecture ou listing — aucun effet de bord",
            Self::Medium => "Écriture ou compilation — modifications locales possibles",
            Self::High => "Suppression ou commande réseau — effets irréversibles possibles",
            Self::Critical => "Destruction massive ou escalade de privilèges",
        }
    }

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Low => "\u{2705}",
            Self::Medium => "\u{26a0}\u{fe0f}",
            Self::High => "\u{1f6d1}",
            Self::Critical => "\u{1f534}",
        }
    }

    #[allow(dead_code)]
    pub fn as_u8(&self) -> u8 {
        match self {
            Self::Low => 0,
            Self::Medium => 1,
            Self::High => 2,
            Self::Critical => 3,
        }
    }
}

impl std::fmt::Display for RiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
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
        let lines: Vec<&str> = command
            .lines()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.starts_with('#')
            })
            .collect();
        let line_count = lines.len();
        let full_trimmed = command.trim();
        let has_pipes = full_trimmed.contains('|');
        let commands: Vec<String> = if has_pipes {
            full_trimmed
                .split('|')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        } else {
            lines.iter().map(|s| s.trim().to_string()).collect()
        };
        let has_env_injection = contains_env_injection(command);
        let has_dangerous_pipe_chain = detect_dangerous_pipe_chain(command);
        let risk = compute_risk(
            command,
            &commands,
            has_pipes,
            has_env_injection,
            has_dangerous_pipe_chain,
        );
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
            parts.push("env var injection detected".to_string());
        }
        if self.has_dangerous_pipe_chain {
            parts.push("dangerous pipe chain".to_string());
        }
        parts.join(" — ")
    }
}

fn contains_env_injection(command: &str) -> bool {
    if command.contains("$(") && command.contains(')') {
        return true;
    }
    if command.contains('`') {
        return true;
    }
    command.contains("${")
}

fn detect_dangerous_pipe_chain(command: &str) -> bool {
    let lower = command.to_lowercase();
    if lower.contains("| sh") || lower.contains("|bash") || lower.contains("|zsh") {
        return true;
    }
    if lower.contains("-exec") && (lower.contains("sh") || lower.contains("bash")) {
        return true;
    }
    if lower.contains("xargs") && (lower.contains("sh") || lower.contains("bash")) {
        return true;
    }
    if lower.contains("eval ") {
        return true;
    }
    lower.contains("exec ")
}

const HIGH_RISK_COMMANDS: &[&str] = &[
    "rm", "rmdir", "mv", "docker", "podman", "npm", "npx", "pnpm", "yarn", "bun", "pip",
    "pip3", "cargo", "go", "make", "cmake", "gcc", "g++", "clang", "patch",
];

const CRITICAL_RISK_COMMANDS: &[&str] = &["rm", "chmod", "chown"];

fn compute_risk(
    command: &str,
    _commands: &[String],
    has_pipes: bool,
    has_env_injection: bool,
    has_dangerous_pipe_chain: bool,
) -> RiskLevel {
    if has_dangerous_pipe_chain {
        return RiskLevel::Critical;
    }

    let lower = command.to_lowercase();
    let first_word = command.split_whitespace().next().unwrap_or("");
    for cmd in CRITICAL_RISK_COMMANDS {
        if first_word == *cmd && first_word == "rm" && (lower.contains("-rf") || lower.contains("-fr")) {
            return RiskLevel::Critical;
        }
    }
    for cmd in HIGH_RISK_COMMANDS {
        if first_word == *cmd {
            return RiskLevel::High;
        }
    }
    if has_env_injection {
        return if has_pipes { RiskLevel::High } else { RiskLevel::Medium };
    }
    if has_pipes {
        return RiskLevel::Medium;
    }
    let non_empty_lines: Vec<&str> = command
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty() && !t.starts_with('#')
        })
        .collect();
    if non_empty_lines.len() > 1 {
        return RiskLevel::Medium;
    }
    RiskLevel::Low
}

fn build_risk_description(
    risk: &RiskLevel,
    has_pipes: bool,
    has_env_injection: bool,
    has_dangerous_pipe_chain: bool,
) -> String {
    let mut parts = vec![risk.description().to_string()];
    if has_dangerous_pipe_chain {
        parts.push("Chaîne de pipes dangereuse détectée (exécution dynamique possible)".to_string());
    }
    if has_env_injection {
        parts.push("Injection de variables d'environnement détectée ($(), backticks, ${VAR})".to_string());
    }
    if has_pipes && !has_dangerous_pipe_chain {
        parts.push("Commande pipée — vérifie chaque segment".to_string());
    }
    parts.join(". ")
}
