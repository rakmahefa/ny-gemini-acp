//! Construction du payload `f.req`, encodage URL, extraction de jetons de page.

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use crate::core::cookies::CookieJar;

use super::config::PageTokens;

pub(crate) fn payload(
    prompt: &str,
    resolved: &crate::core::models::Resolved,
    refs: &[String],
    xsrf: Option<&str>,
) -> String {
    let mut inner = vec![serde_json::Value::Null; 102];
    let refs_json = if refs.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::Value::Array(refs.iter().map(|r| serde_json::json!([null, null, r])).collect())
    };
    inner[0] = serde_json::json!([prompt, 0, null, refs_json, null, null, 0]);
    inner[1] = serde_json::json!(["en"]);
    inner[2] = serde_json::json!(["", "", "", null, null, null, null, null, null, ""]);
    inner[6] = serde_json::json!([0]);
    inner[7] = serde_json::json!(1);
    inner[10] = serde_json::json!(1);
    inner[11] = serde_json::json!(0);
    inner[17] = serde_json::json!([[resolved.think]]);
    inner[18] = serde_json::json!(0);
    inner[27] = serde_json::json!(1);
    inner[30] = serde_json::json!([4]);
    inner[41] = serde_json::json!([2]);
    inner[53] = serde_json::json!(0);
    inner[59] = serde_json::json!(uuid::Uuid::new_v4().to_string());
    inner[61] = serde_json::json!([]);
    inner[68] = serde_json::json!(1);
    inner[79] = serde_json::json!(resolved.mode);
    if let Some(extra) = &resolved.extra {
        for (k, v) in extra {
            inner[*k as usize] = serde_json::json!(v);
        }
    }
    let outer = serde_json::json!([null, serde_json::Value::Array(inner).to_string()]);
    let mut params = vec![("f.req".to_string(), outer.to_string())];
    if let Some(at) = xsrf {
        params.push(("at".to_string(), at.to_string()));
    }
    form_urlencode(&params)
}

pub(crate) fn form_urlencode(params: &[(String, String)]) -> String {
    params.iter().map(|(k, v)| format!("{}={}", enc(k), enc(v))).collect::<Vec<_>>().join("&")
}

fn enc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

pub(crate) fn extract_field(body: &str, key: &str) -> Option<String> {
    let hay = format!("\"{key}\":\"");
    let start = body.find(&hay)? + hay.len();
    let end = body[start..].find('"')?;
    let value = &body[start..start + end];
    (!value.is_empty()).then(|| value.to_string())
}

pub(crate) fn extract_page_tokens(body: &str) -> PageTokens {
    PageTokens {
        at: extract_field(body, "SNlM0e"),
        push_id: extract_field(body, "qKIAYe"),
        pctx: extract_field(body, "Ylro7b"),
    }
}

pub(crate) async fn load_jar(path: &Path) -> (Option<CookieJar>, Option<SystemTime>) {
    let mtime = tokio::fs::metadata(path).await.and_then(|m| m.modified()).ok();
    let jar = tokio::fs::read_to_string(path).await.ok().and_then(|raw| CookieJar::parse(&raw).ok());
    (jar, mtime)
}

pub(crate) fn next_reqid() -> u64 {
    let counter = super::config::REQID_COUNTER.fetch_add(1, Ordering::Relaxed);
    (unix_now().wrapping_mul(100_000)).wrapping_add(counter % 100_000) % 1_000_000
}

fn unix_now() -> u64 {
    crate::core::time::now_unix_u64()
}

#[cfg(test)]
pub(crate) fn decode_freq(body: &str) -> String {
    let raw = body.split_once("f.req=").unwrap().1.split('&').next().unwrap();
    let mut out = Vec::new();
    let bytes = raw.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let h = u8::from_str_radix(&raw[i + 1..i + 3], 16).unwrap();
                out.push(h);
                i += 3;
                continue;
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}
