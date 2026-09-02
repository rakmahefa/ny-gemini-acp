//! Format Google natif `/v1beta/models`.

use serde_json::{json, Value};

use llm_provider::core::tool_prompt::{
    tool_result_line, tool_use_section, BlockKind, INSTRUCTION_FUNCTION_CALL,
};

fn function_call_block(fc: &Value) -> String {
    format!(
        "```function_call\n{}\n```",
        json!({"name":fc.get("name").and_then(Value::as_str).unwrap_or(""),"args":fc.get("args").cloned().unwrap_or_else(||json!({}))})
    )
}
fn google_tool_defs(req: &Value) -> Vec<Value> {
    let mut defs = Vec::new();
    if let Some(tools) = req.get("tools").and_then(Value::as_array) {
        for group in tools {
            if let Some(fns) = group.get("functionDeclarations").and_then(Value::as_array) {
                for fn_ in fns {
                    let mut td = json!({"name":fn_.get("name").and_then(Value::as_str).unwrap_or(""),"description":fn_.get("description").and_then(Value::as_str).unwrap_or("")});
                    if let Some(p) = fn_
                        .get("parameters")
                        .or_else(|| fn_.get("parametersJsonSchema"))
                    {
                        td["parameters"] = p.clone();
                    }
                    defs.push(td);
                }
            }
        }
    }
    defs
}
fn google_tools_section(defs: &[Value]) -> String {
    tool_use_section(BlockKind::FunctionCall, INSTRUCTION_FUNCTION_CALL, defs, "")
}
fn google_tool_choice_instruction(req: &Value) -> String {
    let config = req
        .get("toolConfig")
        .and_then(|c| c.get("functionCallingConfig"));
    let mode = config
        .and_then(|c| c.get("mode"))
        .and_then(Value::as_str)
        .unwrap_or("AUTO");
    match mode {
        "NONE" => "\n\nIMPORTANT: Do NOT call any tools. Respond with text only.".into(),
        "ANY" => {
            let allowed: Vec<String> = config
                .and_then(|c| c.get("allowedFunctionNames"))
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(String::from)
                        .collect()
                })
                .unwrap_or_default();
            if allowed.is_empty() {
                "\n\nIMPORTANT: You MUST call at least one tool. Do not respond with text only."
                    .into()
            } else {
                format!("\n\nIMPORTANT: You MUST call one of these tools: {}. Do not respond with text only.",allowed.iter().map(|n|format!("\"{n}\"")).collect::<Vec<_>>().join(", "))
            }
        }
        _ => String::new(),
    }
}

pub fn google_contents_to_prompt(req: &Value) -> (String, Vec<(String, String)>) {
    let mut parts = Vec::new();
    let mut images = Vec::new();
    let tool_defs = google_tool_defs(req);
    let sys_text = req
        .get("systemInstruction")
        .map(|sys| parts_text(sys.get("parts"), &mut Vec::new()))
        .unwrap_or_default();
    if !sys_text.is_empty() {
        if tool_defs.is_empty() {
            parts.push(format!("[System instruction]: {sys_text}"));
        } else {
            parts.push(format!(
                "[System instruction]: {sys_text}\n\n{}{}",
                google_tools_section(&tool_defs),
                google_tool_choice_instruction(req)
            ));
        }
    } else if !tool_defs.is_empty() {
        parts.push(format!(
            "{}{}",
            google_tools_section(&tool_defs),
            google_tool_choice_instruction(req)
        ));
    }
    if let Some(contents) = req.get("contents").and_then(Value::as_array) {
        for content in contents {
            let role = content
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("user");
            let text = parts_text(content.get("parts"), &mut images);
            match role {
                "model" => parts.push(format!("[Assistant]: {text}")),
                _ => {
                    if !text.is_empty() {
                        parts.push(text)
                    }
                }
            }
        }
    }
    (parts.join("\n\n"), images)
}
fn parts_text(parts: Option<&Value>, images: &mut Vec<(String, String)>) -> String {
    let Some(parts) = parts.and_then(Value::as_array) else {
        return String::new();
    };
    let mut out = Vec::new();
    for p in parts {
        match p.get("text").and_then(Value::as_str) {
            Some(t) if !t.is_empty() => out.push(t.to_string()),
            _ => {
                if let Some(fc) = p.get("functionCall") {
                    out.push(function_call_block(fc));
                } else if let Some(id) = p.get("inlineData") {
                    images.push((
                        id.get("data")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                        id.get("mimeType")
                            .and_then(Value::as_str)
                            .unwrap_or("image/png")
                            .to_string(),
                    ));
                } else if let Some(fr) = p.get("functionResponse") {
                    out.push(tool_result_line(
                        fr.get("name").and_then(Value::as_str).unwrap_or(""),
                        &serde_json::json!(fr.get("response")).to_string(),
                    ));
                }
            }
        }
    }
    out.join(" ")
}
pub fn parse_google_function_calls(text: &str) -> (String, Vec<Value>) {
    static RE1: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re1 = RE1.get_or_init(|| {
        regex::Regex::new(r"(?s)```function_call\s*\n(.*?)\n```").expect("regex statique")
    });
    let mut calls = Vec::new();
    let mut removals: Vec<(usize, usize)> = Vec::new();

    // Fenced blocks: remove only the ones that parse into a usable call.
    for cap in re1.captures_iter(text) {
        let whole = cap.get(0).expect("match has a whole span");
        match try_parse_function_call(cap[1].trim()) {
            Some(call) => {
                calls.push(call);
                removals.push((whole.start(), whole.end()));
            }
            None => tracing::warn!("function_call block left in the response: unusable payload"),
        }
    }

    // Bare (unfenced) markers: the previous single-line regex missed any
    // pretty-printed JSON. A fence + brace-balancing extractor handles
    // multi-line payloads (SPEC-P1-06, M7). Occurrences already covered by a
    // fenced removal are skipped so spans never overlap.
    let mut cursor = 0;
    while let Some(relative) = text[cursor..].find(BARE_MARKER) {
        let marker_start = cursor + relative;
        if removals
            .iter()
            .any(|&(start, end)| marker_start >= start && marker_start < end)
        {
            cursor = (marker_start + BARE_MARKER.len()).min(text.len());
            continue;
        }
        let brace_at = text[marker_start..]
            .find('{')
            .map(|offset| marker_start + offset);
        let Some(brace_at) = brace_at else { break };
        match extract_balanced_json(&text[brace_at..]) {
            Some((payload, consumed)) => {
                let end = brace_at + consumed;
                match try_parse_function_call(&payload) {
                    Some(call) => {
                        calls.push(call);
                        removals.push((marker_start, end));
                    }
                    None => tracing::warn!(
                        "bare function_call payload left in the response: unusable JSON"
                    ),
                }
                cursor = end;
            }
            None => {
                tracing::warn!("unbalanced function_call JSON left in the response");
                cursor = brace_at + 1;
            }
        }
    }

    let mut clean = remove_spans(text, &removals).trim().to_string();
    if calls.is_empty() && clean.trim_start().starts_with('{') {
        if let Some(call) = try_parse_function_call(clean.trim()) {
            calls.push(call);
            clean.clear();
        }
    }
    (clean, calls)
}

const BARE_MARKER: &str = "function_call";

/// Parses a candidate payload into a Google-format function call, or returns
/// None when the JSON is invalid or carries no tool name.
fn try_parse_function_call(payload: &str) -> Option<Value> {
    let data = serde_json::from_str::<Value>(payload).ok()?;
    let name = data.get("name").and_then(Value::as_str)?;
    if name.is_empty() {
        return None;
    }
    Some(
        json!({"name":name,"args":data.get("args").or_else(||data.get("arguments")).cloned().unwrap_or_else(||json!({}))}),
    )
}

/// Extracts a balanced JSON object starting at the first byte of `input`,
/// respecting string literals and escapes. Returns the extracted text and the
/// number of bytes consumed.
fn extract_balanced_json(input: &str) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' if in_string => escaped = true,
            '"' => in_string = !in_string,
            '{' if !in_string => depth += 1,
            '}' if !in_string => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some((input[..=index].to_string(), index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Builds `text` without the given spans, assumed non-overlapping and in scan
/// order.
fn remove_spans(text: &str, spans: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for &(start, end) in spans {
        out.push_str(&text[cursor..start]);
        cursor = end;
    }
    out.push_str(&text[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn google_contents_extrait_les_images_inline() {
        let req = json!({"contents":[{"role":"user","parts":[{"text":"que vois-tu ?"},{"inlineData":{"mimeType":"image/jpeg","data":"aGVsbG8="}}]}]});
        let (prompt, images) = google_contents_to_prompt(&req);
        assert!(prompt.contains("que vois-tu ?"));
        assert_eq!(
            images,
            vec![("aGVsbG8=".to_string(), "image/jpeg".to_string())]
        );
    }
    #[test]
    fn google_tools_section_function_call() {
        let req = json!({"systemInstruction":{"parts":[{"text":"sois utile"}]},"tools":[{"functionDeclarations":[{"name":"lire","description":"lit un fichier","parameters":{"type":"object"}}]}],"toolConfig":{"functionCallingConfig":{"mode":"ANY","allowedFunctionNames":["lire"]}},"contents":[{"role":"user","parts":[{"text":"lis /etc/hosts"}]}]});
        let (prompt, images) = google_contents_to_prompt(&req);
        assert!(images.is_empty());
        assert!(prompt.contains("```function_call"));
        assert!(prompt.contains("\"lire\""));
        assert!(prompt.contains("MUST call one of these tools: \"lire\""));
    }
    #[test]
    fn parse_google_function_calls_trois_formats() {
        let t = "texte\n```function_call\n{\"name\": \"a\", \"args\": {\"x\": 1}}\n```";
        let (clean, calls) = parse_google_function_calls(t);
        assert!(clean.contains("texte"));
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["name"], "a");
        assert_eq!(calls[0]["args"]["x"], 1);
        let t2 = "function_call\n{\"name\": \"b\", \"args\": {}}";
        let (_, calls2) = parse_google_function_calls(t2);
        assert_eq!(calls2.len(), 1);
        assert_eq!(calls2[0]["name"], "b");
        let t3 = "{\"name\": \"c\", \"arguments\": {\"y\": 2}}";
        let (clean3, calls3) = parse_google_function_calls(t3);
        assert!(clean3.is_empty());
        assert_eq!(calls3.len(), 1);
        assert_eq!(calls3[0]["args"]["y"], 2);
        let (clean4, calls4) = parse_google_function_calls("réponse simple");
        assert_eq!(clean4, "réponse simple");
        assert!(calls4.is_empty());
    }
}

#[cfg(test)]
mod extraction_tests {
    use super::*;

    /// SPEC-P1-06 (M7): a bare, pretty-printed (multi-line) function_call is
    /// parsed and removed — the former single-line regex missed it entirely.
    #[test]
    fn bare_multi_line_function_call_is_parsed_and_removed() {
        let text = "avant\nfunction_call\n{\n  \"name\": \"lire\",\n  \"args\": {\"path\": \"a.rs\"}\n}\nfin";
        let (clean, calls) = parse_google_function_calls(text);
        assert_eq!(calls.len(), 1, "calls = {calls:?}");
        assert_eq!(calls[0]["name"], "lire");
        assert_eq!(calls[0]["args"]["path"], "a.rs");
        assert!(
            clean.contains("avant") && clean.contains("fin"),
            "clean = {clean:?}"
        );
        assert!(!clean.contains("function_call"));
    }

    /// SPEC-P1-06 (M7): an invalid bare payload stays visible.
    #[test]
    fn bare_invalid_payload_stays_visible() {
        let text = "texte\nfunction_call\n{not-json}\nfin";
        let (clean, calls) = parse_google_function_calls(text);
        assert!(calls.is_empty());
        assert!(clean.contains("{not-json}"), "clean = {clean:?}");
    }
}
