use std::collections::HashSet;

use crate::state::Role;

pub const CONTEXT_WINDOW_CHARS: usize = 1_000_000;
pub const COMPACTION_THRESHOLD_CHARS: usize = CONTEXT_WINDOW_CHARS * 9 / 10;
pub const EMERGENCY_COMPACTION_CHARS: usize = CONTEXT_WINDOW_CHARS * 7 / 10;

const PRESERVE_TURNS: usize = 10;

/// Evicts the largest old conversation turns until the requested character budget is met.
/// The newest `PRESERVE_TURNS` turns are always retained.
pub fn compact_messages(messages: &mut Vec<(Role, String)>, target_chars: usize) {
    if messages.len() <= 1 {
        return;
    }

    let mut turns: Vec<Vec<(Role, String)>> = Vec::new();
    let mut current = Vec::new();
    for message in messages.iter() {
        if message.0 == Role::User && !current.is_empty() {
            turns.push(std::mem::take(&mut current));
        }
        current.push(message.clone());
    }
    if !current.is_empty() {
        turns.push(current);
    }

    if turns.len() <= PRESERVE_TURNS {
        return;
    }

    let current_chars: usize = messages.iter().map(|(_, text)| text.len()).sum();
    if current_chars <= target_chars {
        return;
    }

    let tail_end = turns.len().saturating_sub(PRESERVE_TURNS);
    let mut candidates: Vec<(usize, usize)> = (0..tail_end)
        .map(|index| {
            (
                index,
                turns[index]
                    .iter()
                    .map(|(_, text)| text.len())
                    .sum::<usize>(),
            )
        })
        .collect();
    candidates.sort_by_key(|(_, chars)| std::cmp::Reverse(*chars));

    let mut remaining = current_chars;
    let mut evict = HashSet::new();
    for (index, chars) in candidates {
        if remaining <= target_chars {
            break;
        }
        evict.insert(index);
        remaining -= chars;
    }

    let mut compacted = Vec::new();
    for (index, turn) in turns.into_iter().enumerate() {
        if index >= tail_end || !evict.contains(&index) {
            compacted.extend(turn);
        }
    }
    *messages = compacted;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_newest_turns() {
        let mut messages = Vec::new();
        for index in 0..12 {
            messages.push((Role::User, format!("user-{index}")));
            messages.push((Role::Assistant, format!("assistant-{index}")));
        }

        compact_messages(&mut messages, 1);

        assert_eq!(messages.len(), PRESERVE_TURNS * 2);
        assert_eq!(messages[0].1, "user-2");
        assert_eq!(messages.last().unwrap().1, "assistant-11");
    }

    #[test]
    fn leaves_messages_unchanged_under_budget() {
        let mut messages = vec![(Role::User, "hello".into()), (Role::Assistant, "world".into())];
        let original = messages.clone();

        compact_messages(&mut messages, 10_000);

        assert_eq!(messages, original);
    }
}
