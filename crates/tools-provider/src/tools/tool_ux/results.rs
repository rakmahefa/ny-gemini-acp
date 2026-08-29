use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::display::{truncate, ux_card};
use super::types::{CardBodyKind, ResultUpdate, MAX_DIFF_OLD_TEXT_BYTES, MAX_RESULT_LOCATIONS, MAX_RESULT_PREVIEW_CHARS};
use super::super::lifecycle::ToolLifecycleState;
use super::super::sandbox::{RiskLevel, ShellAnalysis, ShellSandbox};

pub fn result_update(
    tool_name: &str,
    args: &Value,
    result: &str,
    is_ok: bool,
    cwd: &Path,
    terminal_id: Option<&str>,
) -> ResultUpdate {
    let status = if is_ok { "completed" } else { "failed" };
    let phase = if is_ok { "🟢 completed" } else { "🔴 failed" };
    match tool_name {
        "file_read" => {
            let body = if is_ok { format_numbered_read(result, args) } else { result.to_owned() };
            ResultUpdate {
                status,
                content: vec![ux_card(tool_name, phase, args, Some((&body, CardBodyKind::Output, !is_ok)), terminal_id)],
                locations: file_location(args, cwd),
            }
        }
        "glob" | "list_directory" => ResultUpdate {
            status,
            content: vec![ux_card(tool_name, phase, args, Some((result.trim_end(), CardBodyKind::Output, !is_ok)), terminal_id)],
            locations: filesystem_result_locations(tool_name, result, cwd),
        },
        "shell_exec" => ResultUpdate {
            status,
            content: vec![ux_card(tool_name, phase, args, Some((result.trim_end(), CardBodyKind::Output, !is_ok)), terminal_id)],
            locations: vec![],
        },
        "file_write" | "file_edit" | "replace_in_file" => ResultUpdate {
            status,
            content: vec![ux_card(tool_name, phase, args, Some((result.trim_end(), CardBodyKind::Output, !is_ok)), terminal_id)],
            locations: file_location(args, cwd),
        },
        "search" | "search_and_read" => {
            let rendered = normalize_search_result(tool_name, result, cwd);
            ResultUpdate {
                status,
                content: vec![ux_card(tool_name, phase, args, Some((rendered.trim_end(), CardBodyKind::Output, !is_ok)), terminal_id)],
                locations: search_result_locations(result, cwd),
            }
        }
        "AskUserQuestion" => {
            let body = if is_ok { render_ask_user_result(result) } else { result.to_owned() };
            ResultUpdate {
                status,
                content: vec![ux_card(tool_name, phase, args, Some((&body, CardBodyKind::Content, !is_ok)), terminal_id)],
                locations: vec![],
            }
        }
        "FollowUp" => {
            let body = if is_ok { render_follow_up_result(result) } else { result.to_owned() };
            ResultUpdate {
                status,
                content: vec![ux_card(tool_name, phase, args, Some((&body, CardBodyKind::Content, !is_ok)), terminal_id)],
                locations: vec![],
            }
        }
        _ => ResultUpdate {
            status,
            content: vec![ux_card(tool_name, phase, args, Some((result.trim_end(), CardBodyKind::Output, !is_ok)), terminal_id)],
            locations: vec![],
        },
    }
}

pub fn classify_risk(name: &str, args: &Value) -> RiskLevel {
    match name {
        "shell_exec" => arg_str(args, "command")
            .and_then(|command| ShellSandbox::new().analyze_command(command).ok())
            .map(|ShellAnalysis { risk, .. }| risk)
            .unwrap_or(RiskLevel::Critical),
        "file_write" | "file_edit" | "replace_in_file" => RiskLevel::Medium,
        _ => RiskLevel::Low,
    }
}

pub fn lifecycle_label(state: ToolLifecycleState) -> &'static str {
    match state {
        ToolLifecycleState::Pending => "pending",
        ToolLifecycleState::Permission => "permission",
        ToolLifecycleState::Executing => "executing",
        ToolLifecycleState::Completed => "completed",
        ToolLifecycleState::Failed => "failed",
        ToolLifecycleState::Cancelled => "cancelled",
    }
}

pub fn lifecycle_icon(state: ToolLifecycleState) -> &'static str {
    match state {
        ToolLifecycleState::Pending => "⏳",
        ToolLifecycleState::Permission => "🔐",
        ToolLifecycleState::Executing => "▶",
        ToolLifecycleState::Completed => "🟢",
        ToolLifecycleState::Failed => "🔴",
        ToolLifecycleState::Cancelled => "⚪",
    }
}

fn render_follow_up_result(result: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(result) else { return result.to_owned(); };
    let label = value.get("label").and_then(Value::as_str).unwrap_or("Suggested next step");
    let query = value.get("query").and_then(Value::as_str).unwrap_or("");
    format!("{}\n→ {}", label, query)
}

fn render_ask_user_result(result: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(result) else { return result.to_owned(); };
    let Some(answers) = value.get("answers").and_then(Value::as_object) else { return result.to_owned(); };
    if answers.is_empty() { return "Aucune réponse sélectionnée.".into(); }
    answers.iter().map(|(question, answer)| format!("{question}\n{}", answer_display(answer))).collect::<Vec<_>>().join("\n\n")
}

fn answer_display(value: &Value) -> String {
    match value {
        Value::Array(items) => items.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", "),
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    }
}

fn format_numbered_read(result: &str, args: &Value) -> String {
    let start = args.get("offset").and_then(Value::as_u64).unwrap_or(1).max(1) as usize;
    result.trim_end_matches('\n').split('\n').enumerate().map(|(idx, line)| format!("{}\t{}", start + idx, line)).collect::<Vec<_>>().join("\n")
}

fn file_location(args: &Value, cwd: &Path) -> Vec<Value> {
    arg_str(args, "path").map(|path| vec![json!({ "path": resolve_path(path, cwd) })]).unwrap_or_default()
}

fn filesystem_result_locations(tool_name: &str, result: &str, cwd: &Path) -> Vec<Value> {
    if tool_name == "list_directory" { return vec![]; }
    result.lines().take(MAX_RESULT_LOCATIONS).filter_map(|line| {
        let path = line.trim();
        if path.is_empty() { None } else { Some(json!({ "path": resolve_path(path, cwd) })) }
    }).collect()
}

fn search_result_locations(result: &str, cwd: &Path) -> Vec<Value> {
    let mut locations = Vec::new();
    let mut seen = BTreeSet::new();
    for line in result.lines() {
        let candidate = line.strip_prefix("## ").unwrap_or(line);
        let Some((path, line_number, _)) = split_path_line(candidate) else { continue; };
        let resolved = resolve_path(path, cwd);
        let key = format!("{}:{line_number}", resolved.display());
        if seen.insert(key) { locations.push(json!({ "path": resolved, "line": line_number })); }
        if locations.len() >= MAX_RESULT_LOCATIONS { break; }
    }
    locations
}

fn normalize_search_result(tool_name: &str, result: &str, cwd: &Path) -> String {
    let mut output = String::new();
    for (index, line) in result.lines().enumerate() {
        if index > 0 { output.push('\n'); }
        if tool_name == "search_and_read" && line.starts_with("## ") {
            output.push_str(&normalize_heading_path(line, cwd));
        } else {
            output.push_str(&normalize_match_line(line, cwd));
        }
        if output.chars().count() >= MAX_RESULT_PREVIEW_CHARS { break; }
    }
    truncate(&output, MAX_RESULT_PREVIEW_CHARS)
}

fn normalize_heading_path(line: &str, cwd: &Path) -> String {
    let body = &line[3..];
    let Some((path, line_number, tail)) = split_path_line(body) else { return line.to_owned(); };
    format!("## {}:{}{}", display_path(path, cwd), line_number, tail)
}

fn normalize_match_line(line: &str, cwd: &Path) -> String {
    let Some((path, line_number, tail)) = split_path_line(line) else { return line.to_owned(); };
    if path.starts_with('(') || path.starts_with('…') { return line.to_owned(); }
    format!("{}:{}{}", display_path(path, cwd), line_number, tail)
}

fn split_path_line(line: &str) -> Option<(&str, u32, &str)> {
    let first_colon = line.find(':')?;
    let path = &line[..first_colon];
    if path.is_empty() { return None; }
    let after_path = &line[first_colon + 1..];
    let digit_len = after_path.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_len == 0 { return None; }
    let line_number = after_path[..digit_len].parse::<u32>().ok()?;
    Some((path, line_number, &after_path[digit_len..]))
}

pub(crate) fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

pub(crate) fn resolve_path(path: &str, cwd: &Path) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() { candidate } else { cwd.join(candidate) }
}

pub(crate) fn display_path(path: &str, cwd: &Path) -> String {
    let resolved = resolve_path(path, cwd);
    match resolved.strip_prefix(cwd) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        Ok(_) => ".".into(),
        Err(_) => path.to_owned(),
    }
}

pub(crate) fn read_old_text(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if metadata.len() > MAX_DIFF_OLD_TEXT_BYTES { return None; }
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn risk_for_file_write_is_medium() {
        assert_eq!(classify_risk("file_write", &serde_json::json!({})), RiskLevel::Medium);
    }

    #[test]
    fn lifecycle_labels_are_stable() {
        assert_eq!(lifecycle_label(ToolLifecycleState::Pending), "pending");
        assert_eq!(lifecycle_icon(ToolLifecycleState::Completed), "🟢");
    }
}
