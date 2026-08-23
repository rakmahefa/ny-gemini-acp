//! Builtin web search tool backed by DuckDuckGo's non-JavaScript HTML search.
//!
//! The tool is intentionally provider-neutral at the runtime boundary: it
//! returns bounded plain text through the existing `Tool` contract. Network
//! access is restricted to DuckDuckGo's search endpoint; callers cannot supply
//! an arbitrary URL.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Client;
use serde_json::{json, Value};

use crate::tools::registry::{Tool, ToolDef, ToolResult};

const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const DEFAULT_MAX_RESULTS: usize = 8;
const MAX_RESULTS: usize = 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_RESULT_CHARS: usize = 8 * 1024;
const MAX_OUTPUT_CHARS: usize = 48 * 1024;

fn web_search_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "Requête web à rechercher. Les opérateurs de recherche du moteur sont acceptés."
            },
            "max_results": {
                "type": "integer",
                "minimum": 1,
                "maximum": 10,
                "description": "Nombre maximum de résultats. Défaut: 8, maximum: 10."
            },
            "region": {
                "type": "string",
                "description": "Région DuckDuckGo, par exemple fr-fr, en-us ou wt-wt. Défaut: wt-wt."
            }
        },
        "required": ["query"]
    })
}

fn web_search_def() -> ToolDef {
    ToolDef {
        name: "web_search",
        description: "Recherche sur le web et retourne des résultats bornés avec titre, URL et extrait.",
        parameters_fn: web_search_params,
    }
}

pub struct WebSearchTool;

#[async_trait::async_trait]
impl Tool for WebSearchTool {
    fn definition(&self) -> &ToolDef {
        static DEF: std::sync::OnceLock<ToolDef> = std::sync::OnceLock::new();
        DEF.get_or_init(web_search_def)
    }

    async fn execute(&self, args: &Value, _cwd: &Path, _allowed_dirs: &[PathBuf]) -> ToolResult {
        let query = match args.get("query").and_then(Value::as_str) {
            Some(value) if !value.trim().is_empty() => value.trim(),
            _ => return ToolResult::Err("paramètre 'query' manquant ou vide".into()),
        };

        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, MAX_RESULTS as u64) as usize;

        let region = args
            .get("region")
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or("wt-wt")
            .trim();

        match search_duckduckgo(query, region, max_results).await {
            Ok(results) => ToolResult::Ok(format_results(&results)),
            Err(error) => {
                tracing::warn!(query = %query, error = %error, "web_search failed");
                ToolResult::Err(error)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

async fn search_duckduckgo(
    query: &str,
    region: &str,
    max_results: usize,
) -> Result<Vec<SearchResult>, String> {
    let client = Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .user_agent("ny-gemini-acp/0.2 web_search")
        .build()
        .map_err(|error| format!("initialisation HTTP impossible: {error}"))?;

    let response = client
        .get(SEARCH_ENDPOINT)
        .query(&[("q", query), ("kl", region), ("kp", "1")])
        .send()
        .await
        .map_err(|error| format!("requête web échouée: {error}"))?;

    if !response.status().is_success() {
        return Err(format!("moteur web indisponible: HTTP {}", response.status()));
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("lecture de la réponse web échouée: {error}"))?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(format!(
            "réponse du moteur web trop volumineuse: {} octets > {}",
            bytes.len(), MAX_RESPONSE_BYTES
        ));
    }

    let html = String::from_utf8_lossy(&bytes);
    let results = parse_results(&html, max_results);
    if results.is_empty() {
        return Err("aucun résultat web exploitable n'a été trouvé".into());
    }
    Ok(results)
}

fn parse_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut cursor = 0usize;

    while results.len() < max_results {
        let Some(start) = html[cursor..].find("class=\"result__a\"") else {
            break;
        };
        let class_start = cursor + start;
        let Some(tag_end_rel) = html[class_start..].find('>') else {
            break;
        };
        let tag_end = class_start + tag_end_rel;
        let opening = &html[class_start..tag_end];
        let Some(href) = extract_attribute(opening, "href") else {
            cursor = tag_end + 1;
            continue;
        };

        let Some(title_end_rel) = html[tag_end + 1..].find("</a>") else {
            break;
        };
        let title_end = tag_end + 1 + title_end_rel;
        let title = clean_html_text(&html[tag_end + 1..title_end]);

        let result_end = html[title_end + 4..]
            .find("result__snippet")
            .map(|offset| title_end + 4 + offset)
            .unwrap_or_else(|| title_end + 4);
        let snippet = extract_snippet(html, result_end).unwrap_or_default();

        let url = normalize_result_url(&href);
        if !title.is_empty() && !url.is_empty() {
            results.push(SearchResult {
                title: truncate(&title, MAX_RESULT_CHARS),
                url,
                snippet: truncate(&snippet, MAX_RESULT_CHARS),
            });
        }
        cursor = title_end + 4;
    }

    results
}

fn extract_attribute(tag: &str, attribute: &str) -> Option<String> {
    let needle = format!("{attribute}=\"");
    let start = tag.find(&needle)? + needle.len();
    let end = tag[start..].find('"')? + start;
    Some(decode_html_entities(&tag[start..end]))
}

fn extract_snippet(html: &str, from: usize) -> Option<String> {
    let marker = "class=\"result__snippet\"";
    let marker_offset = html[from..].find(marker)?;
    let class_start = from + marker_offset;
    let tag_end = class_start + html[class_start..].find('>')?;
    let content_end = html[tag_end + 1..].find("</").map(|offset| tag_end + 1 + offset)?;
    Some(clean_html_text(&html[tag_end + 1..content_end]))
}

fn normalize_result_url(url: &str) -> String {
    if let Some((_, query)) = url.split_once("uddg=") {
        let encoded = query.split('&').next().unwrap_or(query);
        return percent_decode(encoded).unwrap_or_else(|| encoded.to_string());
    }
    url.to_string()
}

fn percent_decode(value: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(value.len());
    let raw = value.as_bytes();
    let mut i = 0usize;
    while i < raw.len() {
        if raw[i] == b'%' {
            if i + 2 >= raw.len() {
                return None;
            }
            let hex = std::str::from_utf8(&raw[i + 1..i + 3]).ok()?;
            let byte = u8::from_str_radix(hex, 16).ok()?;
            bytes.push(byte);
            i += 3;
        } else if raw[i] == b'+' {
            bytes.push(b' ');
            i += 1;
        } else {
            bytes.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(bytes).ok()
}

fn clean_html_text(input: &str) -> String {
    let mut text = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    decode_html_entities(text.trim())
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push_str("…");
    }
    output
}

fn format_results(results: &[SearchResult]) -> String {
    let mut output = String::new();
    for (index, result) in results.iter().enumerate() {
        let block = format!(
            "[{}] {}\n{}\n{}\n",
            index + 1,
            result.title,
            result.url,
            if result.snippet.is_empty() {
                "(sans extrait)"
            } else {
                &result.snippet
            }
        );
        if output.len() + block.len() > MAX_OUTPUT_CHARS {
            output.push_str("… résultats tronqués");
            break;
        }
        output.push_str(&block);
    }
    output.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn definition_exposes_bounded_search_contract() {
        let tool = WebSearchTool;
        let value = (tool.definition().parameters_fn)();
        assert_eq!(tool.definition().name, "web_search");
        assert_eq!(value["required"][0], "query");
        assert_eq!(value["properties"]["max_results"]["maximum"], 10);
    }

    #[test]
    fn parser_extracts_title_url_and_snippet() {
        let html = r#"
            <div class="result">
              <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs&amp;rut=1">Example &amp; Docs</a>
              <a class="result__snippet">A <b>useful</b> snippet.</a>
            </div>
        "#;
        let results = parse_results(html, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example & Docs");
        assert_eq!(results[0].url, "https://example.com/docs");
        assert_eq!(results[0].snippet, "A useful snippet.");
    }

    #[test]
    fn parser_respects_result_limit() {
        let html = (0..4)
            .map(|i| {
                format!(
                    r#"<a class="result__a" href="https://example.com/{i}">Result {i}</a><a class="result__snippet">Snippet {i}</a>"#
                )
            })
            .collect::<String>();
        let results = parse_results(&html, 2);
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn empty_query_is_rejected_without_network() {
        let result = WebSearchTool
            .execute(&json!({"query":"   "}), Path::new("."), &[])
            .await;
        assert!(matches!(result, ToolResult::Err(error) if error.contains("query")));
    }

    #[tokio::test]
    async fn result_limit_is_clamped() {
        let value = json!({"query":"rust","max_results":999});
        let max_results = value
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, MAX_RESULTS as u64) as usize;
        assert_eq!(max_results, MAX_RESULTS);
    }
}
