use serde_json::Value;

pub(super) fn render_tool_content(content: &[Value], structured_content: Option<&Value>) -> String {
    let mut rendered = Vec::new();
    for value in content {
        match value.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    rendered.push(text.to_owned());
                }
            }
            Some("resource") => {
                if let Some(text) = value
                    .get("resource")
                    .and_then(|resource| resource.get("text"))
                    .and_then(Value::as_str)
                {
                    rendered.push(text.to_owned());
                } else {
                    rendered.push(
                        serde_json::to_string(value)
                            .unwrap_or_else(|_| "<invalid MCP resource>".into()),
                    );
                }
            }
            _ => rendered.push(
                serde_json::to_string(value)
                    .unwrap_or_else(|_| "<unserializable MCP content>".into()),
            ),
        }
    }
    if rendered.is_empty() {
        if let Some(structured_content) = structured_content {
            rendered.push(
                serde_json::to_string(structured_content)
                    .unwrap_or_else(|_| "<unserializable MCP structuredContent>".into()),
            );
        }
    } else if let Some(structured_content) = structured_content {
        rendered.push(
            serde_json::to_string(structured_content)
                .unwrap_or_else(|_| "<unserializable MCP structuredContent>".into()),
        );
    }
    rendered.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn renders_content_blocks_without_leaking_raw_text_blocks() {
        let content = vec![
            json!({"type":"text","text":"hello"}),
            json!({"type":"resource","resource":{"uri":"file:///x","text":"world"}}),
        ];
        assert_eq!(render_tool_content(&content, None), "hello\nworld");
    }

    #[test]
    fn renders_structured_content_when_no_text_exists() {
        let structured = json!({"answer": 42});
        assert_eq!(
            render_tool_content(&[], Some(&structured)),
            r#"{"answer":42}"#
        );
    }
}
