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
        assert_eq!(take_partial_output("sess-partial"), "");
        assert_eq!(take_partial_output("sess-partial"), "");
    }
}
