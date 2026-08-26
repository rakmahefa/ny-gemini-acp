use agent_client_protocol::schema::v1::{Content, ContentBlock, TextContent, ToolCallContent};
use serde_json::Value;

use super::types::{CardBodyKind, ToolVisual, MAX_CARD_BODY_CHARS, MAX_RAW_INPUT_CHARS};

pub(crate) fn bounded_raw_input(args: &Value) -> Value {
    let mut value = args.clone();
    let Some(object) = value.as_object_mut() else {
        return value;
    };
    let Some(content_value) = object.get_mut("content") else {
        return value;
    };
    let Some(content) = content_value.as_str() else {
        return value;
    };
    let count = content.chars().count();
    if count <= MAX_RAW_INPUT_CHARS {
        return value;
    }
    let preview: String = content.chars().take(MAX_RAW_INPUT_CHARS).collect();
    *content_value = Value::String(format!(
        "{preview}\n… [{} chars omitted from ACP display]",
        count - MAX_RAW_INPUT_CHARS
    ));
    value
}

pub(crate) fn ux_card(
    tool_name: &str,
    phase: &str,
    args: &Value,
    body: Option<(&str, CardBodyKind, bool)>,
    terminal: Option<&str>,
) -> ToolCallContent {
    let visual = ToolVisual::for_tool(tool_name, args);
    let mut text = format!(
        "**{} {}**\n{}  ·  {}  ·  {} {}",
        visual.icon,
        visual.label,
        phase,
        visual.permission,
        visual.risk.emoji(),
        visual.risk.label()
    );
    if let Some(terminal) = terminal {
        text.push_str(&format!("  ·  ▣ terminal {terminal}"));
    }
    let rendered_body = body
        .map(|(body, kind, error)| render_card_body(body, kind, error))
        .unwrap_or_else(|| render_card_body("_En attente du résultat…_", CardBodyKind::Content, false));
    text.push_str("\n\n");
    text.push_str(&rendered_body);
    text_content(&truncate(&text, MAX_CARD_BODY_CHARS), false)
}

fn render_card_body(body: &str, kind: CardBodyKind, error: bool) -> String {
    let label = match kind {
        CardBodyKind::Output => "Output",
        CardBodyKind::Content => "Content",
        CardBodyKind::Input => "Input",
    };
    if body.trim().is_empty() {
        return match kind {
            CardBodyKind::Output => format!("**{label}**\n_Sortie vide._"),
            CardBodyKind::Content => format!("**{label}**\n_Aucun contenu._"),
            CardBodyKind::Input => format!("**{label}**\n_Aucune donnée._"),
        };
    }
    let prefix = if error { "⚠️\n" } else { "" };
    format!("**{label}**\n```text\n{prefix}{body}\n```")
}

pub(crate) fn tool_visual(name: &str) -> (&'static str, &'static str) {
    match name {
        "file_read" => ("📖", "File Read"),
        "file_write" => ("📝", "File Write"),
        "file_edit" | "replace_in_file" => ("✏️", "File Edit"),
        "glob" => ("🧭", "Glob"),
        "list_directory" => ("📁", "Directory"),
        "search" => ("🔎", "Search"),
        "search_and_read" => ("🔎", "Search & Read"),
        "shell_exec" => ("▣", "Shell"),
        "AskUserQuestion" => ("⚙️", "Ask User"),
        "FollowUp" => ("↪", "Follow-up"),
        _ => ("⚙️", "Tool"),
    }
}

pub(crate) fn permission_label(name: &str) -> &'static str {
    match name {
        "file_write" | "file_edit" | "replace_in_file" | "shell_exec" => "🔐 permission",
        "AskUserQuestion" => "👤 user input",
        "FollowUp" => "🔓 no permission",
        _ => "🔓 no permission",
    }
}

pub(crate) fn text_content(text: &str, error: bool) -> ToolCallContent {
    let rendered = if error {
        format!("⚠️ {text}")
    } else {
        text.to_owned()
    };
    ToolCallContent::Content(Content::new(ContentBlock::Text(TextContent::new(rendered))))
}

pub(crate) fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    format!("{}…", value.chars().take(max).collect::<String>())
}

pub(crate) fn concise_args(args: &Value) -> String {
    let Some(obj) = args.as_object() else {
        return "{}".into();
    };
    format!(
        "Arguments: {}",
        obj.keys().cloned().collect::<Vec<_>>().join(", ")
    )
}
