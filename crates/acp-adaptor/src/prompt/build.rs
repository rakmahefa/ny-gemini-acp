use agent_runtime::persona;
use agent_runtime::prompt::{format_tool_call, format_tool_result};
use agent_runtime::state::{HistoryEntry, Session};
use agent_runtime::ToolProvider;

pub const MAX_MESSAGES: usize = 12;
pub const MAX_PROMPT_CHARS: usize = 32_000;

fn format_entry(entry: &HistoryEntry) -> String {
    match entry {
        HistoryEntry::User { content } => format!("<user_message>\n{content}\n</user_message>\n\n"),
        HistoryEntry::Assistant { content } => format!("<assistant_message>\n{content}\n</assistant_message>\n\n"),
        HistoryEntry::ToolCall { id, name, arguments } => {
            format!("{}\n\n", format_tool_call(id, name, arguments))
        }
        HistoryEntry::ToolResult {
            id,
            name,
            content,
            is_ok,
        } => {
            format!("{}\n\n", format_tool_result(id, name, content, *is_ok))
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

    let history = session.messages.entries();
    let n = history.len();
    if n == 0 {
        return system;
    }

    let lens: Vec<usize> = history.iter().map(|entry| format_entry(entry).chars().count()).collect();
    let prefix: Vec<usize> = std::iter::once(0)
        .chain(lens.iter().scan(0usize, |sum, len| {
            *sum += *len;
            Some(*sum)
        }))
        .collect();

    let mut turn_starts = vec![0usize];
    for (index, entry) in history.iter().enumerate().skip(1) {
        if matches!(entry, HistoryEntry::User { .. }) {
            turn_starts.push(index);
        }
    }

    let first_turn = turn_starts.len().saturating_sub(MAX_MESSAGES);
    let mut lo = first_turn;
    let mut hi = turn_starts.len().saturating_sub(1);
    let budget_ok = |turn_index: usize| {
        let start = turn_starts[turn_index];
        prefix[n] - prefix[start] <= MAX_PROMPT_CHARS
    };
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if budget_ok(mid) {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    let start = turn_starts[lo];
    let history_text: String = history.iter().skip(start).map(format_entry).collect();
    format!("{system}{history_text}")
}

#[cfg(test)]
#[path = "../test/build.rs"]
mod tests;
