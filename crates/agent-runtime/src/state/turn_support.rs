//! Runtime-owned transient turn state.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

type PartialOutputMap = HashMap<String, String>;
static PARTIAL_OUTPUT: OnceLock<Mutex<PartialOutputMap>> = OnceLock::new();

fn partial_output_map() -> &'static Mutex<PartialOutputMap> {
    PARTIAL_OUTPUT.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn begin_partial_output(session_id: &str) {
    partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .insert(session_id.to_owned(), String::new());
}

pub fn clear_partial_output(session_id: &str) {
    if let Some(output) = partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .get_mut(session_id)
    {
        output.clear();
    }
}

pub fn record_partial_output(session_id: &str, text: &str) {
    if text.is_empty() {
        return;
    }
    partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .entry(session_id.to_owned())
        .or_default()
        .push_str(text);
}

pub fn take_partial_output(session_id: &str) -> String {
    partial_output_map()
        .lock()
        .expect("partial output mutex poisoned")
        .remove(session_id)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_output_is_scoped_to_a_turn() {
        begin_partial_output("sess-partial");
        record_partial_output("sess-partial", "Hello ");
        record_partial_output("sess-partial", "world");
        assert_eq!(take_partial_output("sess-partial"), "Hello world");
        assert_eq!(take_partial_output("sess-partial"), "");
    }

    #[test]
    fn partial_output_can_be_reset_before_tool_execution() {
        begin_partial_output("sess-tool");
        record_partial_output("sess-tool", "before tool");
        clear_partial_output("sess-tool");
        record_partial_output("sess-tool", "after tool");
        assert_eq!(take_partial_output("sess-tool"), "after tool");
    }
}
