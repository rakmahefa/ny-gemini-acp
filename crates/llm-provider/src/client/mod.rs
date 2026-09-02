//! Client web API Gemini.

mod config;
pub(crate) mod payload;
mod stream;
mod upload;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::sync::mpsc;
use tracing::{debug, warn};

pub use config::{ClientInner, Config, StreamItem, StreamResult, DEFAULT_BL};

#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<ClientInner>,
}

impl Client {
    pub async fn new(config: Config) -> Result<Self> {
        use config::USER_AGENT;
        let mut builder = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(config.request_timeout);
        if let Some(proxy) = &config.proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy).context("proxy invalide")?);
        }
        let jar = payload::load_jar(&config.cookie_file).await;
        match &jar.0 {
            Some(cookies) => {
                let n = cookies.header().map_or(0, |h| h.split(';').count());
                debug!(
                    "cookies loaded: {} pairs, SAPISID {}",
                    n,
                    if cookies.sapisid().is_some() {
                        "present"
                    } else {
                        "absent"
                    }
                );
            }
            None => warn!(
                "no cookies loaded from {:?} — requests will fail",
                config.cookie_file
            ),
        }
        let inner = Arc::new(ClientInner {
            http: builder.build().context("failed to build HTTP client")?,
            config,
            jar: tokio::sync::RwLock::new(jar),
            page: tokio::sync::RwLock::new(None),
        });
        let client = Self { inner };
        client.refresh_page().await;
        Ok(client)
    }

    pub async fn stream(
        &self,
        prompt: &str,
        model: &str,
        think: Option<u32>,
        refs: &[String],
    ) -> Result<mpsc::Receiver<StreamResult>> {
        let model_arg = match think {
            Some(t) => format!("{model}@think={t}"),
            None => model.to_string(),
        };
        let resolved = crate::core::models::resolve(&model_arg, &self.inner.config.default_model)
            .map_err(anyhow::Error::new)?;
        debug!(
            "stream: {} -> mode {} think {} extra {:?}",
            resolved.name, resolved.mode, resolved.think, resolved.extra
        );
        let (tx, rx) = mpsc::channel(16);
        let client = self.clone();
        let prompt = prompt.to_string();
        let refs = refs.to_vec();
        tokio::spawn(async move {
            tokio::select! {
                result = client.run_turn(tx.clone(), prompt, refs, &resolved) => {
                    if let Err(e) = result {
                        // The typed error crosses the channel unchanged: the
                        // runtime taxonomy (authentication, upload, divergence,
                        // ...) stays intact for every downstream consumer.
                        let _ = tx.send(Err(crate::core::errors::map_gemini_error(&e))).await;
                    }
                }
                _ = tx.closed() => {}
            }
        });
        Ok(rx)
    }

    pub async fn complete(
        &self,
        prompt: &str,
        model: &str,
        think: Option<u32>,
        refs: &[String],
    ) -> Result<String> {
        let mut rx = self.stream(prompt, model, think, refs).await?;
        let mut out = String::new();
        while let Some(item) = rx.recv().await {
            match item {
                Ok(StreamItem::Text(delta)) => out.push_str(&delta),
                Ok(StreamItem::ToolCall { name, .. }) => {
                    anyhow::bail!("Gemini emitted tool call `{name}` during text completion")
                }
                Ok(StreamItem::Metadata { .. }) => {}
                Err(e) => anyhow::bail!("{e}"),
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod classification_tests {
    use crate::core::GeminiError;
    use agent_runtime::LlmError;

    fn exit_error(error: GeminiError) -> LlmError {
        // The exact projection applied by the spawned stream task when
        // `run_turn` fails.
        crate::core::errors::map_gemini_error(&anyhow::Error::new(error))
    }

    #[test]
    fn cookies_expired_arrives_as_authentication_not_provider_string() {
        let error = exit_error(GeminiError::CookiesExpired { code: 401 });
        assert!(
            matches!(error, LlmError::Authentication(_)),
            "cookies expiration must be classified as authentication, got {error:?}"
        );
    }

    #[test]
    fn every_stream_error_variant_keeps_its_taxonomy() {
        assert!(matches!(
            exit_error(GeminiError::UpstreamRejected { code: 403 }),
            LlmError::Provider(_) | LlmError::InvalidRequest(_)
        ));
        assert!(matches!(
            exit_error(GeminiError::Network("timeout".into())),
            LlmError::Network(_)
        ));
        assert!(matches!(
            exit_error(GeminiError::Http { status: 401 }),
            LlmError::Authentication(_)
        ));
        assert!(matches!(
            exit_error(GeminiError::StreamDivergence),
            LlmError::StreamDivergence
        ));
        assert!(matches!(
            exit_error(GeminiError::UploadFailed("scotty".into())),
            LlmError::Upload(_)
        ));
        assert!(matches!(
            exit_error(GeminiError::SafetyBlocked("refused".into())),
            LlmError::Provider(_)
        ));
        assert!(matches!(
            exit_error(GeminiError::UnknownModel("nope".into())),
            LlmError::Unavailable(_)
        ));
    }
}

#[cfg(test)]
#[path = "../test/client.rs"]
mod tests;
