//! Table des modèles Gemini web (cf. spec §4.4 — vérité =
//! `vendor/gemini-web2api/models.py`).
//!
//! `MODE_CATEGORY` : 1=FAST, 2=THINKING, 3=PRO, 4=AUTO,
//! 5=FAST_DYNAMIC_THINKING, 6=FLASH_LITE.

use crate::core::GeminiError;

pub const DEFAULT_MODEL: &str = "gemini-3.6-flash";
pub const MODEL_KEYS: &[&str] = &[
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3.5-flash-thinking",
    "gemini-3.1-pro",
    "gemini-3.1-pro-enhanced",
    "gemini-auto",
    "gemini-3.5-flash-thinking-lite",
    "gemini-flash-lite",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    pub mode: u32,
    pub think: u32,
    pub extra: Option<Vec<(u32, i64)>>,
}

fn table(key: &str) -> Option<Model> {
    Some(match key {
        "gemini-3.6-flash" | "gemini-3.5-flash" => Model {
            mode: 1,
            think: 4,
            extra: None,
        },
        "gemini-3.5-flash-thinking" => Model {
            mode: 2,
            think: 0,
            extra: None,
        },
        "gemini-3.1-pro" => Model {
            mode: 3,
            think: 4,
            extra: None,
        },
        "gemini-3.1-pro-enhanced" => Model {
            mode: 3,
            think: 4,
            extra: Some(vec![(31, 2), (80, 3)]),
        },
        "gemini-auto" => Model {
            mode: 4,
            think: 4,
            extra: None,
        },
        "gemini-3.5-flash-thinking-lite" => Model {
            mode: 5,
            think: 0,
            extra: None,
        },
        "gemini-flash-lite" => Model {
            mode: 6,
            think: 4,
            extra: None,
        },
        _ => return None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub name: String,
    pub mode: u32,
    pub think: u32,
    pub extra: Option<Vec<(u32, i64)>>,
}

pub fn resolve(model: &str, default: &str) -> Result<Resolved, GeminiError> {
    let mut name = model;
    let mut think_override = None;
    let count = name.matches("@think=").count();
    if count > 1 {
        return Err(GeminiError::Other(anyhow::anyhow!(
            "Multiple @think= suffixes in model name '{name}' (expected at most one)"
        )));
    }
    if let Some(idx) = name.find("@think=") {
        let level = &name[idx + "@think=".len()..];
        let parsed = level
            .parse::<u32>()
            .map_err(|_| GeminiError::Other(anyhow::anyhow!("Invalid think level: {level}")))?;
        if parsed > 4 {
            tracing::warn!(
                requested = parsed,
                "@think={level} exceeds the max (4), clamping to 4"
            );
        }
        think_override = Some(parsed.min(4));
        name = &name[..idx];
    }
    let cfg = match table(name) {
        Some(c) => c,
        None => {
            tracing::warn!("Unknown model '{name}', falling back to default '{default}'");
            name = default;
            // D-03 : le défaut provient de la configuration (GEMINI_ACP_MODEL)
            // et peut être invalide — erreur typée au lieu d'un panic par requête.
            table(default).ok_or_else(|| {
                GeminiError::UnknownModel(format!(
                    "{default} (default model must be one of: {})",
                    MODEL_KEYS.join(", ")
                ))
            })?
        }
    };
    Ok(Resolved {
        name: name.to_string(),
        mode: cfg.mode,
        think: think_override.unwrap_or(cfg.think),
        extra: cfg.extra,
    })
}

pub fn is_thinking_mode(mode: u32) -> bool {
    mode == 2 || mode == 5
}

#[cfg(test)]
#[path = "../test/models.rs"]
mod tests;
