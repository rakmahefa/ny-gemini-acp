use agent_runtime::persona;
use agent_runtime::state::{HistoryEntry, Session};
use agent_runtime::ToolProvider;
pub const MAX_MESSAGES: usize = 12;
pub const MAX_PROMPT_CHARS: usize = 32_000;

fn format_entry(entry: &HistoryEntry) -> String {
    match entry {
        HistoryEntry::User { content } => format!("[User]: {content}\n\n"),
        HistoryEntry::Assistant { content } => format!("[Assistant]: {content}\n\n"),
        HistoryEntry::ToolCall { id, name, arguments } => {
            format!("[tool_call {name} id={id}] {arguments}\n\n")
        }
        HistoryEntry::ToolResult {
            id,
            name,
            content,
            is_ok,
        } if name == "legacy" && id.is_empty() => format!("{content}\n\n"),
        HistoryEntry::ToolResult {
            id,
            name,
            content,
            is_ok,
        } => {
            let status = if *is_ok { "ok" } else { "error" };
            format!("[tool_result {name} id={id} status={status}] {content}\n\n")
        }
    }
}

pub fn build_prompt(session: &Session, provider: Option<&dyn ToolProvider>) -> String {
    let system = persona::system_prompt(session, None);
    let tools_section = if session.tools_enabled {
        provider.and_then(ToolProvider::prompt_fragment)
    } else {
        None
    };
    let system = match tools_section {
        Some(ts) => format!("{system}{ts}\n\n"),
        None => system,
    };
    let n = session.messages.len();
    if n == 0 {
        return system;
    }

    let lens: Vec<usize> = session
        .messages
        .iter()
        .map(|entry| format_entry(entry).chars().count())
        .collect();
    let prefix: Vec<usize> = std::iter::once(0)
        .chain(lens.iter().scan(0usize, |sum, len| {
            *sum += *len;
            Some(*sum)
        }))
        .collect();

    let min_start = n.saturating_sub(MAX_MESSAGES);
    let mut lo = min_start;
    let mut hi = n.saturating_sub(1);
    let budget_ok = |start: usize| prefix[n] - prefix[start] <= MAX_PROMPT_CHARS;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if budget_ok(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    format!("{system}{}", format_history(session, lo))
}

fn format_history(session: &Session, start: usize) -> String {
    session.messages.iter().skip(start).map(format_entry).collect()
}

#[cfg(test)]
#[path = "../test/build.rs"]
mod tests;
