//! Builtin web search tool backed by DuckDuckGo's non-JavaScript HTML search.
//!
//! The runtime sees only the existing `Tool` contract. Network access stays
//! fixed to the search provider and every response is bounded before parsing.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::{Client, Url};
use serde_json::{json, Value};

use crate::tools::registry::{Tool, ToolDef, ToolResult};

const SEARCH_ENDPOINT: &str = "https://html.duckduckgo.com/html/";
const DEFAULT_MAX_RESULTS: usize = 8;
const MAX_RESULTS: usize = 10;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE_BYTES: usize = 512 * 1024;
const MAX_RESULT_CHARS: usize = 8 * 1024;
const MAX_OUTPUT_CHARS: usize = 48 * 1024;
const MAX_QUERY_CHARS: usize = 512;
const MAX_REGION_CHARS: usize = 32;

fn web_search_params() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "minLength": 1,
                "maxLength": MAX_QUERY_CHARS,
                "description": "Requête web à rechercher."
            },
            "max_results": {
                "type": "integer",
                "minimum": 1,
                "maximum": MAX_RESULTS,
                "description": "Nombre maximum de résultats. Défaut: 8, maximum: 10."
            },
            "region": {
                "type": "string",
                "maxLength": MAX_REGION_CHARS,
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
        let query = match normalized_bounded_string(args.get("query"), MAX_QUERY_CHARS) {
            Some(value) if !value.is_empty() => value,
            _ => return ToolResult::Err("paramètre 'query' manquant, vide ou trop long".into()),
        };

        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_MAX_RESULTS as u64)
            .clamp(1, MAX_RESULTS as u64) as usize;

        let region = match normalized_bounded_string(args.get("region"), MAX_REGION_CHARS) {
            Some(value) if !value.is_empty() => value,
            _ => "wt-wt".to_owned(),
        };

        match search_duckduckgo(query.as_str(), region.as_str(), max_results).await {
            Ok(results) => ToolResult::Ok(format_results(&results)),
            Err(error) => {
                tracing::warn!(query = %query, error = %error, "web_search failed");
                ToolResult::Err(error)
            }
        }
    }
}

fn normalized_bounded_string(value: Option<&Value>, max_chars: usize) -> Option<String> {
    let value = value?.as_str()?.trim();
    if value.is_empty() || value.chars().count() > max_chars {
        return None;
    }
    Some(value.to_owned())
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
    search_endpoint(&client, SEARCH_ENDPOINT, query, region, max_results).await
}

async fn search_endpoint(
    client: &Client,
    endpoint: &str,
    query: &str,
    region: &str,
    max_results: usize,
) -> Result<Vec<SearchResult>, String> {
    let endpoint = Url::parse(endpoint).map_err(|error| format!("endpoint web invalide: {error}"))?;
    if endpoint.scheme() != "https" && endpoint.scheme() != "http" {
        return Err("endpoint web non autorisé".into());
    }

    let response = client
        .get(endpoint)
        .query(&[("q", query), ("kl", region), ("kp", "1")])
        .send()
        .await
        .map_err(|error| format!("requête web échouée: {error}"))?;

    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            429 => "limite de requêtes du moteur web atteinte".to_owned(),
            403 => "moteur web a refusé la requête".to_owned(),
            500..=599 => format!("moteur web indisponible: HTTP {status}"),
            _ => format!("moteur web indisponible: HTTP {status}"),
        });
    }

    if let Some(length) = response.content_length() {
        if length > MAX_RESPONSE_BYTES as u64 {
            return Err(format!(
                "réponse du moteur web trop volumineuse: {length} octets > {MAX_RESPONSE_BYTES}"
            ));
        }
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if !content_type.is_empty()
        && !content_type.starts_with("text/html")
        && !content_type.starts_with("application/xhtml+xml")
    {
        return Err(format!("réponse web inattendue: Content-Type {content_type}"));
    }

    let bytes = read_body_bounded(response).await?;
    let html = String::from_utf8_lossy(&bytes);
    let results = parse_results(&html, max_results);
    if results.is_empty() {
        return Err("aucun résultat web exploitable n'a été trouvé".into());
    }
    Ok(results)
}

async fn read_body_bounded(response: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::with_capacity(16 * 1024);

    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|error| format!("lecture de la réponse web échouée: {error}"))?;
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err(format!(
                "réponse du moteur web trop volumineuse: > {MAX_RESPONSE_BYTES} octets"
            ));
        }
        body.extend_from_slice(&chunk);
    }

    Ok(body)
}

fn parse_results(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut cursor = 0usize;

    while results.len() < max_results {
        let Some(relative) = html[cursor..].find("result__a") else {
            break;
        };
        let marker_start = cursor + relative;
        let Some(tag_start) = html[..marker_start].rfind('<') else {
            cursor = marker_start + 1;
            continue;
        };
        if !html[tag_start..marker_start].contains("<a") {
            cursor = marker_start + 1;
            continue;
        }
        let Some(tag_end_rel) = html[marker_start..].find('>') else {
            break;
        };
        let tag_end = marker_start + tag_end_rel;
        let opening = &html[tag_start..tag_end];
        let Some(href) = extract_attribute(opening, "href") else {
            cursor = tag_end + 1;
            continue;
        };

        let Some(title_end_rel) = html[tag_end + 1..].find("</a>") else {
            break;
        };
        let title_end = tag_end + 1 + title_end_rel;
        let title = clean_html_text(&html[tag_end + 1..title_end]);

        let snippet = extract_snippet(html, title_end + 4).unwrap_or_default();
        let url = normalize_result_url(&href);
        if !title.is_empty() && is_http_url(&url) {
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
    let tag = tag.trim_start_matches('<').trim_start();
    let mut rest = tag;
    let tag_name_end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest = &rest[tag_name_end..];

    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.starts_with('>') {
            break;
        }

        let key_end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let key = &rest[..key_end];
        rest = rest[key_end..].trim_start();
        if !rest.starts_with('=') {
            let skip = rest.find(char::is_whitespace).unwrap_or(rest.len());
            rest = &rest[skip..];
            continue;
        }
        rest = rest[1..].trim_start();
        let quote = rest.chars().next()?;
        if quote != '\'' && quote != '"' {
            let value_end = rest
                .find(char::is_whitespace)
                .unwrap_or(rest.len());
            let value = &rest[..value_end];
            rest = &rest[value_end..];
            if key.eq_ignore_ascii_case(attribute) {
                return Some(decode_html_entities(value));
            }
            continue;
        }
        let value_start = quote.len_utf8();
        let end = rest[value_start..].find(quote)? + value_start;
        let value = &rest[value_start..end];
        rest = &rest[end + quote.len_utf8()..];
        if key.eq_ignore_ascii_case(attribute) {
            return Some(decode_html_entities(value));
        }
    }

    None
}

fn extract_snippet(html: &str, from: usize) -> Option<String> {
    let remaining = &html[from..];
    let class_offset = remaining.find("result__snippet")?;
    let class_start = from + class_offset;
    let tag_start = html[..class_start].rfind('<')?;
    let tag_end = class_start + html[class_start..].find('>')?;
    let content_start = tag_end + 1;
    let content_end = html[content_start..].find("</")? + content_start;
    if tag_start >= tag_end || content_start > content_end {
        return None;
    }
    Some(clean_html_text(&html[content_start..content_end]))
}

fn normalize_result_url(url: &str) -> String {
    let url = decode_html_entities(url.trim());
    let parsed = if let Ok(parsed) = Url::parse(&url) {
        parsed
    } else if let Ok(parsed) = Url::parse(&format!("https:{url}")) {
        parsed
    } else {
        return url;
    };

    if parsed
        .domain()
        .is_some_and(|domain| domain.ends_with("duckduckgo.com"))
    {
        if let Some((_, target)) = parsed.query_pairs().find(|(key, _)| key == "uddg") {
            return target.into_owned();
        }
    }

    parsed.to_string()
}

fn is_http_url(url: &str) -> bool {
    Url::parse(url)
        .map(|parsed| matches!(parsed.scheme(), "http" | "https") && parsed.host_str().is_some())
        .unwrap_or(false)
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
    decode_html_entities(&collapse_whitespace(text.trim()))
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_html_entities(input: &str) -> String {
    let mut output = input
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");

    while let Some(start) = output.find("&#x") {
        let Some(end_rel) = output[start..].find(';') else {
            break;
        };
        let end = start + end_rel;
        let hex = &output[start + 3..end];
        let Some(code) = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) else {
            break;
        };
        output.replace_range(start..=end, &code.to_string());
    }

    output
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut output = value.chars().take(max_chars).collect::<String>();
    if value.chars().count() > max_chars {
        output.push('…');
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
        if output.len().saturating_add(block.len()) > MAX_OUTPUT_CHARS {
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
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn definition_exposes_bounded_search_contract() {
        let tool = WebSearchTool;
        let value = (tool.definition().parameters_fn)();
        assert_eq!(tool.definition().name, "web_search");
        assert_eq!(value["required"][0], "query");
        assert_eq!(value["properties"]["query"]["maxLength"], MAX_QUERY_CHARS);
        assert_eq!(value["properties"]["max_results"]["maximum"], MAX_RESULTS);
    }

    #[test]
    fn parser_accepts_reordered_and_single_quoted_attributes() {
        let html = r#"
            <div class='result'>
              <a href='//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.com%2Fdocs&amp;rut=1' data-x='1' class='result__a'>Example &amp; Docs</a>
              <a data-x='2' class='result__snippet'>A <b>useful</b> snippet.</a>
            </div>
        "#;
        let results = parse_results(html, 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Example & Docs");
        assert_eq!(results[0].url, "https://example.com/docs");
        assert_eq!(results[0].snippet, "A useful snippet.");
    }

    #[test]
    fn parser_rejects_non_http_urls() {
        let html = r#"<a class="result__a" href="javascript:alert(1)">Bad</a>"#;
        assert!(parse_results(html, 1).is_empty());
    }

    #[test]
    fn parser_respects_result_limit() {
        let html = (0..4)
            .map(|i| {
                format!(r#"<a class="result__a" href="https://example.com/{i}">Result {i}</a><a class="result__snippet">Snippet {i}</a>"#)
            })
            .collect::<String>();
        let results = parse_results(&html, 2);
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn url_normalization_preserves_direct_urls() {
        assert_eq!(
            normalize_result_url("https://example.com/a?q=1"),
            "https://example.com/a?q=1"
        );
    }

    #[tokio::test]
    async fn empty_query_is_rejected_without_network() {
        let result = WebSearchTool
            .execute(&json!({"query":"   "}), Path::new("."), &[])
            .await;
        assert!(matches!(result, ToolResult::Err(error) if error.contains("query")));
    }

    #[tokio::test]
    async fn overlong_query_is_rejected_without_network() {
        let result = WebSearchTool
            .execute(
                &json!({"query":"x".repeat(MAX_QUERY_CHARS + 1)}),
                Path::new("."),
                &[],
            )
            .await;
        assert!(matches!(result, ToolResult::Err(error) if error.contains("trop long")));
    }

    #[tokio::test]
    async fn local_http_round_trip_exercises_transport_and_parser() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let html = r#"<a class="result__a" href="https://example.com">Example</a><a class="result__snippet">Local test</a>"#;
        let payload = html.as_bytes().to_vec();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 4096];
            let _ = socket.read(&mut request).await.unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                payload.len()
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
            socket.write_all(&payload).await.unwrap();
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let results = search_endpoint(
            &client,
            &format!("http://{address}/html/"),
            "rust",
            "wt-wt",
            1,
        )
        .await
        .unwrap();

        server.await.unwrap();
        assert_eq!(results[0].title, "Example");
        assert_eq!(results[0].url, "https://example.com/");
        assert_eq!(results[0].snippet, "Local test");
    }

    #[tokio::test]
    async fn local_http_rejects_oversized_content_length() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            let headers = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                MAX_RESPONSE_BYTES + 1
            );
            socket.write_all(headers.as_bytes()).await.unwrap();
        });

        let client = Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let error = search_endpoint(
            &client,
            &format!("http://{address}/html/"),
            "rust",
            "wt-wt",
            1,
        )
        .await
        .unwrap_err();

        server.await.unwrap();
        assert!(error.contains("trop volumineuse"));
    }
}
