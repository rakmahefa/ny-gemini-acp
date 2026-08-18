//! Client web API Gemini (cf. spec §4.2/§4.3 — vérité =
//! `vendor/gemini-web2api/gemini.py`).

mod config;
pub(crate) mod payload;
mod stream;
mod upload;

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use crate::llm::{LlmError, LlmProvider, LlmRequest, LlmStream};
use tokio::sync::mpsc;
use tracing::{debug, warn};

pub use config::{ClientInner, Config, StreamItem, DEFAULT_BL};

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
                    "cookies chargés: {} paires, SAPISID {}",
                    n,
                    if cookies.sapisid().is_some() { "présent" } else { "absent" }
                );
            }
            None => warn!(
                "aucun cookie chargé depuis {:?} — les requêtes échoueront",
                config.cookie_file
            ),
        }
        let inner = Arc::new(ClientInner {
            http: builder.build().context("construction client HTTP")?,
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
    ) -> Result<mpsc::Receiver<StreamItem>> {
        let model_arg = match think {
            Some(t) => format!("{model}@think={t}"),
            None => model.to_string(),
        };
        let resolved = crate::core::models::resolve(&model_arg, &self.inner.config.default_model)
            .map_err(|e| anyhow::anyhow!(e))?;
        debug!(
            "stream: {} -> mode {} think {} extra {:?}",
            resolved.name, resolved.mode, resolved.think, resolved.extra
        );
        let (tx, rx) = mpsc::channel(16);
        let client = self.clone();
        let prompt = prompt.to_string();
        let refs = refs.to_vec();
        tokio::spawn(async move {
            if let Err(e) = client.run_turn(tx.clone(), prompt, refs, &resolved).await {
                let _ = tx.send(Err(format!("{e:#}"))).await;
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
                Ok(delta) => out.push_str(&delta),
                Err(e) => anyhow::bail!("{e}"),
            }
        }
        Ok(out)
    }
}

#[async_trait::async_trait]
impl LlmProvider for Client {
    fn name(&self) -> &'static str { "gemini" }

    fn is_thinking_model(&self, model: &str) -> bool {
        crate::core::models::resolve(model, &self.inner.config.default_model)
            .map(|resolved| crate::core::models::is_thinking_mode(resolved.mode))
            .unwrap_or(false)
    }

    async fn upload_images(&self, images: &[(String, String)]) -> Result<Vec<String>, LlmError> {
        let mut refs = Vec::with_capacity(images.len());
        for (base64, mime) in images {
            refs.push(
                self.upload_image(base64, mime)
                    .await
                    .map_err(|error| LlmError::Provider(error.to_string()))?,
            );
        }
        Ok(refs)
    }

    async fn stream(&self, request: LlmRequest) -> Result<LlmStream, LlmError> {
        let receiver = self
            .stream(&request.prompt, &request.model, request.thinking, &request.refs)
            .await
            .map_err(|error| LlmError::Provider(error.to_string()))?;
        let (tx, mapped) = mpsc::channel(16);
        tokio::spawn(async move {
            let mut receiver = receiver;
            while let Some(item) = receiver.recv().await {
                let mapped_item = item.map_err(LlmError::Provider);
                if tx.send(mapped_item).await.is_err() { break; }
            }
        });
        Ok(LlmStream::new(mapped))
    }
}

#[cfg(test)]
#[path = "../test/client.rs"]
mod tests;
