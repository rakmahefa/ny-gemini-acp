use std::collections::BTreeMap;

/// Parses XML-like `key=value` attributes while accepting arbitrary ordering,
/// optional whitespace around `=`, and both single and double quoted values.
///
/// This helper is intentionally protocol-neutral: it does not know about any
/// provider, tool name, or ACP type.
pub fn parse_tag_attributes(input: &str) -> BTreeMap<String, String> {
    let mut attrs = BTreeMap::new();
    let bytes = input.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] == b'/' {
            break;
        }

        let key_start = index;
        while index < bytes.len()
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
        {
            index += 1;
        }
        if key_start == index {
            index += 1;
            continue;
        }

        let key = &input[key_start..index];
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            break;
        }
        index += 1;

        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() {
            break;
        }

        let value = if bytes[index] == b'\'' || bytes[index] == b'"' {
            let quote = bytes[index];
            index += 1;
            let value_start = index;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            let value = input[value_start..index].to_owned();
            if index < bytes.len() {
                index += 1;
            }
            value
        } else {
            let value_start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            input[value_start..index].to_owned()
        };

        attrs.insert(key.to_ascii_lowercase(), value);
    }

    attrs
}

#[cfg(test)]
mod tests {
    use super::parse_tag_attributes;

    #[test]
    fn accepts_reordered_double_quoted_attributes() {
        let attrs = parse_tag_attributes(r#"query="cargo test" label="Run tests""#);
        assert_eq!(attrs.get("label"), Some(&"Run tests".to_owned()));
        assert_eq!(attrs.get("query"), Some(&"cargo test".to_owned()));
    }

    #[test]
    fn accepts_single_quoted_attributes_and_whitespace() {
        let attrs = parse_tag_attributes("label = 'Run tests' query = 'cargo test'");
        assert_eq!(attrs.get("label"), Some(&"Run tests".to_owned()));
        assert_eq!(attrs.get("query"), Some(&"cargo test".to_owned()));
    }
}
