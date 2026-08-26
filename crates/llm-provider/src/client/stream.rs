//! Streaming HTTP : retry avec backoff exponentiel, lecture du flux Gemini,
//! construction de la requête, gestion des cookies et jetons de page.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::core::auth::sapisid_hash;
use crate::core::cookies::CookieJar;
use crate::core::frames::{self, GeminiFrameDecoder, GeminiFrameEvent};
use crate::core::models;
use crate::core::GeminiError;
use anyhow::{bail, Context};
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, ORIGIN, REFERER};
use tokio::sync::mpsc;
use tracing::{debug, trace, warn};

use super::config::{StreamItem, StreamResult, ENDPOINT, TOKEN_TTL};
use super::payload::{extract_page_tokens, load_jar, next_reqid, payload};
use super::Client;

const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_EVENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_ID_BYTES: usize = 1024;
const MAX_TOOL_NAME_BYTES: usize = 1024;
const MAX_TOOL_ARGUMENTS_BYTES: usize = 1024 * 1024;
const MAX_RAW_ACCUMULATOR: usize = 64 * 1024;

struct AttemptState<'a> {
    decoder: &'a mut GeminiFrameDecoder,
    emitted: &'a mut String,
    emitted_tools: &'a mut HashSet<String>,
    tx: &'a mpsc::Sender<StreamResult>,
}

impl Client {
    pub(crate) async fn run_turn(
        &self,
        tx: mpsc::Sender<StreamResult>,
        prompt: String,
        refs: Vec<String>,
        resolved: &models::Resolved,
    ) -> anyhow::Result<()> {
        let attempts = self.inner.config.retry_attempts.max(1);
        let mut emitted = String::new();
        let mut emitted_tools = HashSet::new();
        let mut decoder = GeminiFrameDecoder::new();

        for attempt in 1..=attempts {
            if tx.is_closed() {
                return Ok(());
            }
            let mut state = AttemptState {
                decoder: &mut decoder,
                emitted: &mut emitted,
                emitted_tools: &mut emitted_tools,
                tx: &tx,
            };
            match self
                .attempt_http(&prompt, &refs, resolved, &mut state)
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) => {
                    let es = e.to_string();
                    if es.contains("cookie")
                        || es.contains("Cookie")
                        || es.contains("BardErrorInfo")
                    {
                        return Err(e);
                    }
                    if emitted.is_empty() && emitted_tools.is_empty() && attempt < attempts {
                        let base_ms = self.inner.config.retry_delay.as_millis() as u64;
                        let delay_ms =
                            std::cmp::min(base_ms.saturating_mul(1u64 << (attempt - 1)), 30_000);
                        let jitter = delay_ms / 4;
                        let ts_nanos = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_nanos() as u64;
                        let jitter_ms = (ts_nanos % (2 * jitter + 1)).saturating_sub(jitter);
                        let effective = delay_ms.saturating_add(jitter_ms);
                        debug!(
                            tentative = attempt,
                            total = attempts,
                            "tentative échouée, retry dans {}ms — {e:#}",
                            effective
                        );
                        decoder.clear();
                        tokio::select! {
                            _ = tx.closed() => return Ok(()),
                            _ = tokio::time::sleep(Duration::from_millis(effective)) => {}
                        }
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        unreachable!("run_turn: la boucle de tentatives doit toujours retourner")
    }

    async fn attempt_http(
        &self,
        prompt: &str,
        refs: &[String],
        resolved: &models::Resolved,
        state: &mut AttemptState<'_>,
    ) -> anyhow::Result<Option<()>> {
        let (url, headers, body) = self.build_request(prompt, refs, resolved).await?;
        let response = self
            .inner
            .http
            .post(&url)
            .headers(headers)
            .body(body)
            .send()
            .await
            .context("envoi requête Gemini")?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(GeminiError::Http { status }.into());
        }

        let mut bytes_stream = response.bytes_stream();
        let mut raw_accumulator = String::new();

        loop {
            tokio::select! {
                _ = state.tx.closed() => return Ok(None),
                chunk = bytes_stream.next() => {
                    let Some(chunk) = chunk else {
                        for frame in state.decoder.finish() {
                            validate_frame_event(&frame)?;
                            emit_frame(frame, state.emitted, state.emitted_tools, state.tx).await?;
                        }
                        if let Some(reason) = frames::detect_safety_block(&raw_accumulator) {
                            let _ = state.tx.send(Err(reason)).await;
                            return Ok(Some(()));
                        }
                        if state.emitted.is_empty() && state.emitted_tools.is_empty() && frames::is_empty_stream(&raw_accumulator) {
                            let _ = state.tx.send(Err("Gemini n'a produit aucune réponse exploitable.".to_string())).await;
                            return Ok(Some(()));
                        }
                        return Ok(Some(()));
                    };
                    let bytes = chunk.context("lecture flux Gemini")?;
                    let text = String::from_utf8_lossy(&bytes);
                    if state.decoder.pending().len().saturating_add(text.len()) > MAX_FRAME_BYTES {
                        return Err(GeminiError::Other(anyhow::anyhow!(
                            "Gemini frame exceeded the configured safety limit ({MAX_FRAME_BYTES} bytes)"
                        )).into());
                    }
                    trace!("chunk {} octets, queue ligne {}", text.len(), state.decoder.pending().len());
                    if raw_accumulator.len() < MAX_RAW_ACCUMULATOR {
                        raw_accumulator.push_str(&text);
                        if raw_accumulator.len() > MAX_RAW_ACCUMULATOR {
                            raw_accumulator.truncate(MAX_RAW_ACCUMULATOR);
                        }
                    }

                    let combined = format!("{}{}", state.decoder.pending(), text);
                    if combined.contains("BardErrorInfo") {
                        let code = frames::bard_error(&combined).unwrap_or(0);
                        return Err(GeminiError::CookiesExpired { code }.into());
                    }
                    if let Some(reason) = frames::detect_safety_block(&combined) {
                        let _ = state.tx.send(Err(reason)).await;
                        return Ok(Some(()));
                    }
                    for frame in state.decoder.feed(&text) {
                        validate_frame_event(&frame)?;
                        emit_frame(frame, state.emitted, state.emitted_tools, state.tx).await?;
                    }
                }
            }
        }
    }

    async fn build_request(
        &self,
        prompt: &str,
        refs: &[String],
        resolved: &models::Resolved,
    ) -> anyhow::Result<(String, HeaderMap, String)> {
        let inner = &self.inner;
        let prefix = inner
            .config
            .auth_user
            .map(|n| format!("/u/{n}"))
            .unwrap_or_default();
        let reqid = next_reqid();
        let url = format!(
            "https://gemini.google.com{prefix}/{ENDPOINT}?bl={}&hl=en&_reqid={reqid}&rt=c",
            inner.config.bl
        );
        let jar = self.jar().await;
        let mut headers = HeaderMap::new();
        if let Some(cookie) = jar.as_ref().and_then(CookieJar::header) {
            let mut v = HeaderValue::from_str(&cookie).context("header Cookie invalide")?;
            v.set_sensitive(true);
            headers.insert(reqwest::header::COOKIE, v);
        }
        if let Some(sapisid) = jar.as_ref().and_then(CookieJar::sapisid) {
            let auth = sapisid_hash(sapisid, "https://gemini.google.com");
            let mut v = HeaderValue::from_str(&auth).context("header Authorization invalide")?;
            v.set_sensitive(true);
            headers.insert(AUTHORIZATION, v);
        }
        headers.insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/x-www-form-urlencoded;charset=utf-8"),
        );
        headers.insert(
            ORIGIN,
            HeaderValue::from_static("https://gemini.google.com"),
        );
        headers.insert(
            REFERER,
            HeaderValue::from_str(&format!("https://gemini.google.com{prefix}/app"))?,
        );
        headers.insert("X-Same-Domain", HeaderValue::from_static("1"));
        if let Some(user) = inner.config.auth_user {
            headers.insert("X-Goog-AuthUser", HeaderValue::from_str(&user.to_string())?);
        }
        let body = payload(
            prompt,
            resolved,
            refs,
            self.page_tokens().await.at.as_deref(),
        );
        Ok((url, headers, body))
    }

    pub(crate) async fn jar(&self) -> Option<CookieJar> {
        let mut guard = self.inner.jar.write().await;
        let mtime = tokio::fs::metadata(&self.inner.config.cookie_file)
            .await
            .and_then(|m| m.modified())
            .ok();
        if guard.1 != mtime {
            *guard = load_jar(&self.inner.config.cookie_file).await;
        }
        guard.0.clone()
    }

    pub(crate) async fn page_tokens(&self) -> super::config::PageTokens {
        {
            let guard = self.inner.page.read().await;
            if let Some((tokens, at)) = guard.as_ref() {
                if at.elapsed() < TOKEN_TTL {
                    return tokens.clone();
                }
            }
        }
        self.refresh_page().await;
        self.inner
            .page
            .read()
            .await
            .as_ref()
            .map(|(t, _)| t.clone())
            .unwrap_or_default()
    }

    pub(crate) async fn refresh_page(&self) {
        let prefix = self
            .inner
            .config
            .auth_user
            .map(|n| format!("/u/{n}"))
            .unwrap_or_default();
        let url = format!("https://gemini.google.com{prefix}/app");
        match self.inner.http.get(&url).send().await {
            Ok(resp) => {
                let body = match resp.text().await {
                    Ok(b) => b,
                    Err(e) => {
                        warn!("lecture /app impossible: {e:#}");
                        return;
                    }
                };
                let tokens = extract_page_tokens(&body);
                *self.inner.page.write().await = Some((tokens.clone(), Instant::now()));
                debug!(
                    "jetons de page récupérés (at: {}, push_id: {}, pctx: {})",
                    tokens.at.is_some(),
                    tokens.push_id.is_some(),
                    tokens.pctx.is_some()
                );
            }
            Err(e) => {
                let safe = self.inner.config.proxy.as_ref().map(|_| "<redacted>");
                warn!("GET /app impossible: {e:#} proxy={:?}", safe);
            }
        }
    }
}

fn validate_frame_event(frame: &GeminiFrameEvent) -> anyhow::Result<()> {
    match frame {
        GeminiFrameEvent::Text(text) if text.len() > MAX_TEXT_EVENT_BYTES => {
            bail!("Gemini text frame exceeded {} bytes", MAX_TEXT_EVENT_BYTES);
        }
        GeminiFrameEvent::ToolCall {
            id,
            name,
            arguments,
        } => {
            if id.len() > MAX_TOOL_ID_BYTES {
                bail!("Gemini tool call id exceeded {} bytes", MAX_TOOL_ID_BYTES);
            }
            if name.len() > MAX_TOOL_NAME_BYTES {
                bail!("Gemini tool name exceeded {} bytes", MAX_TOOL_NAME_BYTES);
            }
            let argument_bytes = serde_json::to_vec(arguments)?.len();
            if argument_bytes > MAX_TOOL_ARGUMENTS_BYTES {
                bail!(
                    "Gemini tool arguments exceeded {} bytes",
                    MAX_TOOL_ARGUMENTS_BYTES
                );
            }
        }
        GeminiFrameEvent::Metadata { value, .. } => {
            let metadata_bytes = serde_json::to_vec(value)?.len();
            if metadata_bytes > 64 * 1024 {
                bail!("Gemini metadata exceeded 65536 bytes");
            }
        }
        _ => {}
    }
    Ok(())
}

async fn emit_frame(
    frame: GeminiFrameEvent,
    emitted: &mut String,
    emitted_tools: &mut HashSet<String>,
    tx: &mpsc::Sender<StreamResult>,
) -> anyhow::Result<()> {
    match frame {
        GeminiFrameEvent::Text(candidate) => {
            if candidate == *emitted || emitted.starts_with(&candidate) {
                return Ok(());
            }
            if !candidate.starts_with(emitted.as_str()) {
                if emitted.is_empty() {
                    *emitted = candidate;
                    return Ok(());
                }
                bail!("Gemini stream content changed during retry");
            }
            let delta = frames::clean_text(&candidate[emitted.len()..], false);
            *emitted = candidate;
            if !delta.is_empty() {
                tx.send(Ok(StreamItem::Text(delta)))
                    .await
                    .map_err(|_| anyhow::anyhow!("stream receiver closed"))?;
            }
        }
        GeminiFrameEvent::ToolCall {
            id,
            name,
            arguments,
        } => {
            if emitted_tools.insert(id.clone()) {
                tx.send(Ok(StreamItem::ToolCall {
                    id,
                    name,
                    arguments,
                }))
                .await
                .map_err(|_| anyhow::anyhow!("stream receiver closed"))?;
            }
        }
        GeminiFrameEvent::Metadata { kind, value } => {
            tx.send(Ok(StreamItem::Metadata { kind, value }))
                .await
                .map_err(|_| anyhow::anyhow!("stream receiver closed"))?;
        }
    }
    Ok(())
}
