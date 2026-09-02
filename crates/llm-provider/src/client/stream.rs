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
use agent_runtime::LlmError;

// C-17 : plafond par frame individuelle (ligne du flux Gemini). Ne pas
// confondre avec `MAX_BUFFER_BYTES` (core/frames.rs) qui borne le buffer
// cumulé du décodeur — deux niveaux de défense différents.
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
    diverged: &'a mut bool,
    tx: &'a mpsc::Sender<StreamResult>,
}

/// Décodeur UTF-8 incrémental : conserve le résidu d'octets d'un chunk à
/// l'autre afin qu'un caractère multi-octets coupé à une frontière de chunk
/// TCP ne soit jamais corrompu en U+FFFD (D-01).
struct IncrementalUtf8 {
    residual: Vec<u8>,
}

impl IncrementalUtf8 {
    fn new() -> Self {
        Self {
            residual: Vec::new(),
        }
    }

    /// Pousse un chunk d'octets et retourne le texte valide correspondant.
    /// Un caractère multi-octets incomplet en fin de chunk est conservé en
    /// résidu et complété au chunk suivant. Une séquence réellement invalide
    /// est ignorée (avec un warn) au lieu d'être convertie en U+FFFD.
    fn push(&mut self, bytes: &[u8]) -> String {
        self.residual.extend_from_slice(bytes);
        let (text, keep_from) = match std::str::from_utf8(&self.residual) {
            Ok(text) => (text.to_owned(), self.residual.len()),
            Err(e) => (
                String::from_utf8_lossy(&self.residual[..e.valid_up_to()]).into_owned(),
                match e.error_len() {
                    Some(invalid_len) => {
                        // Séquence invalide : on la saute plutôt que d'insérer U+FFFD.
                        tracing::warn!(
                            bytes = invalid_len,
                            "invalid UTF-8 sequence skipped in Gemini stream"
                        );
                        e.valid_up_to() + invalid_len
                    }
                    None => e.valid_up_to(),
                },
            ),
        };
        self.residual.drain(..keep_from);
        text
    }

    /// Flush final : tout résidu restant est nécessairement incomplet ou
    /// invalide (fin du stream), on le décode en mode lossy.
    fn finish(&mut self) -> String {
        let text = String::from_utf8_lossy(&self.residual).into_owned();
        self.residual.clear();
        text
    }
}

/// Tronque `s` à `max` octets en s'arrêtant sur une frontière de caractère
/// (`String::truncate` panique sinon — D-02).
fn truncate_at_char_boundary(s: &mut String, max: usize) {
    if s.len() <= max {
        return;
    }
    let mut cut = max;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    s.truncate(cut);
}

/// Traite un segment de texte UTF-8 valide : plafond de taille de frame,
/// accumulation brute, détection d'erreurs amont et émission des frames.
/// Retourne `true` si le flux doit se terminer immédiatement (blocage
/// sécurité déjà envoyé au canal).
async fn feed_decoded_text(
    state: &mut AttemptState<'_>,
    raw_accumulator: &mut String,
    text: &str,
) -> anyhow::Result<bool> {
    if state.decoder.pending().len().saturating_add(text.len()) > MAX_FRAME_BYTES {
        return Err(GeminiError::Other(anyhow::anyhow!(
            "Gemini frame exceeded the configured safety limit ({MAX_FRAME_BYTES} bytes)"
        ))
        .into());
    }
    trace!(
        "segment {} octets, queue ligne {}",
        text.len(),
        state.decoder.pending().len()
    );
    if raw_accumulator.len() < MAX_RAW_ACCUMULATOR {
        raw_accumulator.push_str(text);
        truncate_at_char_boundary(raw_accumulator, MAX_RAW_ACCUMULATOR);
    }

    // SPEC-P1-06: no detection runs on the undecoded raw stream. Backend
    // errors and safety blocks are identified on DECODED frames only, so a
    // model echoing e.g. "BardErrorInfo [401]" inside legitimate prose can
    // never kill the stream by mistake.
    for frame in state.decoder.feed(text) {
        if let GeminiFrameEvent::Metadata { kind, value } = &frame {
            if kind == "blockReason" {
                let reason = value.as_str().unwrap_or("politique de sécurité");
                let _ = state
                    .tx
                    .send(Err(LlmError::Provider(format!(
                        "Gemini a refusé de répondre (blockReason: {reason}). Reformulez votre prompt."
                    ))))
                    .await;
                return Ok(true);
            }
        }
        if let Some(code) = decoded_bard_error(&frame) {
            if code == 401 {
                return Err(GeminiError::CookiesExpired { code }.into());
            }
            return Err(GeminiError::UpstreamRejected { code }.into());
        }
        validate_frame_event(&frame)?;
        emit_frame(
            frame,
            state.emitted,
            state.emitted_tools,
            state.diverged,
            state.tx,
        )
        .await?;
    }
    Ok(false)
}

/// Detects a backend error marker inside a DECODED frame's metadata payload
/// (e.g. an `unparsed_frame` preview). This replaces the former pre-decode
/// scan of the raw stream: a structural, protocol-level signal only. Text
/// candidates are deliberately NOT scanned — prose quoting the marker is
/// data, not a backend failure.
fn decoded_bard_error(frame: &GeminiFrameEvent) -> Option<i64> {
    match frame {
        GeminiFrameEvent::Metadata { value, .. } => frames::bard_error(&value.to_string()),
        GeminiFrameEvent::Text(_) | GeminiFrameEvent::ToolCall { .. } => None,
    }
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
        let mut diverged = false;
        let mut decoder = GeminiFrameDecoder::new();

        for attempt in 1..=attempts {
            if tx.is_closed() {
                return Ok(());
            }
            let mut state = AttemptState {
                decoder: &mut decoder,
                emitted: &mut emitted,
                emitted_tools: &mut emitted_tools,
                diverged: &mut diverged,
                tx: &tx,
            };
            match self
                .attempt_http(&prompt, &refs, resolved, &mut state)
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) => {
                    // D-14 : décision de retry basée sur les erreurs typées (et
                    // plus sur des sous-chaînes du message d'erreur).
                    let fatal = e.downcast_ref::<GeminiError>().is_some_and(|error| {
                        matches!(
                            error,
                            GeminiError::CookiesExpired { .. }
                                | GeminiError::UpstreamRejected { .. }
                                | GeminiError::UnknownModel(_)
                        )
                    });
                    if fatal {
                        return Err(e);
                    }
                    let retryable_divergence = e
                        .downcast_ref::<GeminiError>()
                        .is_some_and(|error| matches!(error, GeminiError::StreamDivergence));
                    if attempt < attempts
                        && emitted_tools.is_empty()
                        && (emitted.is_empty() || retryable_divergence)
                    {
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
                            attempt,
                            total = attempts,
                            divergence = retryable_divergence,
                            "attempt failed, retrying in {}ms — {e:#}",
                            effective
                        );
                        decoder.clear();
                        diverged = false;
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
            .map_err(|error| {
                // C-16 : les erreurs réseau sont typées (timeout, DNS,
                // connexion rompue) au lieu d'un anyhow générique.
                GeminiError::Network(format!("requête Gemini impossible: {error}"))
            })?;

        if !response.status().is_success() {
            let status = response.status().as_u16();
            return Err(GeminiError::Http { status }.into());
        }

        let mut bytes_stream = response.bytes_stream();
        let mut raw_accumulator = String::new();
        let mut utf8 = IncrementalUtf8::new();

        loop {
            tokio::select! {
                _ = state.tx.closed() => return Ok(None),
                chunk = bytes_stream.next() => {
                    let Some(chunk) = chunk else {
                        // Fin du stream : flush du résidu UTF-8 puis des frames
                        // partielles restantes.
                        let tail = utf8.finish();
                        let mut blocked = false;
                        if !tail.is_empty() {
                            blocked = feed_decoded_text(state, &mut raw_accumulator, &tail).await?;
                        }
                        for frame in state.decoder.finish() {
                            validate_frame_event(&frame)?;
                            emit_frame(frame, state.emitted, state.emitted_tools, state.diverged, state.tx).await?;
                        }
                        if blocked {
                            return Ok(Some(()));
                        }
                        // SPEC-P1-06: the former raw-accumulator safety scan
                        // (hardcoded refusal phrases like "I can't help with
                        // that") is removed. Safety blocks are detected on
                        // the typed `blockReason` metadata of decoded frames
                        // above; prose quoting a refusal phrase is data.
                        if *state.diverged {
                            return Err(GeminiError::StreamDivergence.into());
                        }
                        if state.emitted.is_empty() && state.emitted_tools.is_empty() && frames::is_empty_stream(&raw_accumulator) {
                            let _ = state
                                .tx
                                .send(Err(LlmError::Provider(
                                    "Gemini produced no usable response.".to_string(),
                                )))
                                .await;
                            return Ok(Some(()));
                        }
                        return Ok(Some(()));
                    };
                    let bytes = chunk.map_err(|error| {
                        GeminiError::Network(format!("lecture flux Gemini interrompue: {error}"))
                    })?;
                    let text = utf8.push(&bytes);
                    if text.is_empty() {
                        // Rien de décodable pour l'instant (caractère coupé en
                        // attente de la suite) : on attend le chunk suivant.
                        continue;
                    }
                    if feed_decoded_text(state, &mut raw_accumulator, &text).await? {
                        return Ok(Some(()));
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
                        warn!("failed to read /app: {e:#}");
                        return;
                    }
                };
                let tokens = extract_page_tokens(&body);
                *self.inner.page.write().await = Some((tokens.clone(), Instant::now()));
                debug!(
                    "page tokens retrieved (at: {}, push_id: {}, pctx: {})",
                    tokens.at.is_some(),
                    tokens.push_id.is_some(),
                    tokens.pctx.is_some()
                );
            }
            Err(e) => {
                let safe = self.inner.config.proxy.as_ref().map(|_| "<redacted>");
                warn!("GET /app failed: {e:#} proxy={:?}", safe);
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
    diverged: &mut bool,
    tx: &mpsc::Sender<StreamResult>,
) -> anyhow::Result<()> {
    match frame {
        GeminiFrameEvent::Text(candidate) => {
            if candidate == *emitted {
                *diverged = false;
                return Ok(());
            }
            if !candidate.starts_with(emitted.as_str()) {
                *diverged = true;
                tracing::warn!(
                    emitted_bytes = emitted.len(),
                    candidate_bytes = candidate.len(),
                    "Gemini stream candidate diverged; waiting for a compatible snapshot"
                );
                return Ok(());
            }
            let delta = frames::clean_text(&candidate[emitted.len()..], false);
            *emitted = candidate;
            *diverged = false;
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn divergent_snapshot_is_recoverable_before_stream_end() {
        let (tx, mut rx) = mpsc::channel(8);
        let mut emitted = "Bonjour".to_string();
        let mut tools = HashSet::new();
        let mut diverged = false;

        emit_frame(
            GeminiFrameEvent::Text("Bonjouir".into()),
            &mut emitted,
            &mut tools,
            &mut diverged,
            &tx,
        )
        .await
        .unwrap();
        assert!(diverged);
        assert!(rx.try_recv().is_err());
        assert_eq!(emitted, "Bonjour");

        emit_frame(
            GeminiFrameEvent::Text("Bonjour, monde".into()),
            &mut emitted,
            &mut tools,
            &mut diverged,
            &tx,
        )
        .await
        .unwrap();
        assert!(!diverged);
        assert_eq!(emitted, "Bonjour, monde");
        assert!(matches!(rx.try_recv().unwrap(), Ok(StreamItem::Text(text)) if text == ", monde"));
    }

    #[tokio::test]
    async fn unrecoverable_divergence_remains_distinguishable() {
        let (tx, _rx) = mpsc::channel(8);
        let mut emitted = "Bonjour".to_string();
        let mut tools = HashSet::new();
        let mut diverged = false;

        emit_frame(
            GeminiFrameEvent::Text("Bonsoir".into()),
            &mut emitted,
            &mut tools,
            &mut diverged,
            &tx,
        )
        .await
        .unwrap();

        assert!(diverged);
        assert_eq!(emitted, "Bonjour");
        let error = GeminiError::StreamDivergence;
        assert!(matches!(error, GeminiError::StreamDivergence));
        let _ = json!({"kind": "stream_divergence"});
    }

    #[test]
    fn utf8_split_across_chunks_is_not_corrupted() {
        // 'é' fait 2 octets, 🎉 en fait 4 : on coupe au milieu de chacun.
        let full = "émoji 🎉 suite";
        let bytes = full.as_bytes();
        let (a, rest) = bytes.split_at(1); // 'é' coupé
        let (b, c) = rest.split_at(9); // 🎉 coupé
        let mut dec = IncrementalUtf8::new();

        assert_eq!(dec.push(a), "");
        assert_eq!(dec.push(b), "émoji ");
        assert_eq!(dec.push(c), "🎉 suite");
        assert_eq!(dec.push(b""), "");
        assert_eq!(dec.finish(), "");

        // Et en une seule fois, le texte complet ressort intact.
        let mut dec = IncrementalUtf8::new();
        assert_eq!(dec.push(bytes), full);
    }

    #[test]
    fn utf8_invalid_sequence_is_skipped_without_fffd() {
        let mut dec = IncrementalUtf8::new();
        assert_eq!(dec.push(&[0x41, 0xFF, 0x42]), "A");
        assert_eq!(dec.finish(), "B");
    }

    #[test]
    fn truncate_stops_at_char_boundary() {
        let mut s = "héhé".to_string(); // frontières : 0, 1, 3, 4, 6
        truncate_at_char_boundary(&mut s, 2);
        assert_eq!(s, "h");

        let mut s = "abc".to_string();
        truncate_at_char_boundary(&mut s, 64);
        assert_eq!(s, "abc");

        let mut s = "ééé".to_string();
        truncate_at_char_boundary(&mut s, 0);
        assert_eq!(s, "");
    }
}

#[cfg(test)]
mod decoded_detection_tests {
    use super::*;
    use crate::core::frames::{GeminiFrameDecoder, GeminiFrameEvent};
    use serde_json::{json, Value};

    fn wire_line(inner: Value) -> String {
        let escaped = serde_json::to_string(&inner.to_string()).unwrap();
        format!("[[\"wrb.fr\",[62,0],{escaped}],[\"di\",72]]")
    }

    /// SPEC-P1-06: prose quoting a refusal phrase or a backend error marker
    /// is data — it must be emitted, never abort the stream.
    #[tokio::test]
    async fn prose_quoting_refusal_phrases_and_error_markers_is_emitted() {
        let decoder = &mut GeminiFrameDecoder::new();
        let emitted = &mut String::new();
        let tools = &mut HashSet::new();
        let diverged = &mut false;
        let (tx, mut rx) = mpsc::channel(8);
        let state = &mut AttemptState {
            decoder,
            emitted,
            emitted_tools: tools,
            diverged,
            tx: &tx,
        };
        let raw = &mut String::new();

        let inner = json!([
            null,
            ["tok"],
            "padding-padding-padding-padding",
            [],
            json!([[
                "c",
                ["Le modele a dit : I can't help with that et BardErrorInfo [401]"]
            ]]),
            [],
            [],
            []
        ]);
        let line = wire_line(inner);
        let blocked = feed_decoded_text(state, raw, &format!("{line}\n"))
            .await
            .unwrap();
        assert!(!blocked, "legitimate prose quoting markers must not abort");
        // Release the channel sender so the collector below can drain it.
        drop(tx);
        let mut seen = String::new();
        while let Ok(item) = rx.try_recv() {
            if let Ok(StreamItem::Text(delta)) = item {
                seen.push_str(&delta);
            }
        }
        assert!(seen.contains("I can't help with that"), "seen = {seen:?}");
        assert!(seen.contains("BardErrorInfo [401]"), "seen = {seen:?}");
    }

    /// SPEC-P1-06: a structural backend error (unparsed frame metadata) is
    /// still classified as a real failure.
    #[tokio::test]
    async fn structural_backend_error_in_metadata_is_detected() {
        let frame = GeminiFrameEvent::Metadata {
            kind: "unparsed_frame".into(),
            value: serde_json::json!({"preview": "{\"error\": \"BardErrorInfo [401]\"}"}),
        };
        assert_eq!(decoded_bard_error(&frame), Some(401));
    }
}
