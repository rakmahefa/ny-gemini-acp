use agent_client_protocol::schema::v1::{
    ContentBlock, SessionId, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::LlmProvider;
use tools_provider::tools::executor::safe_session_update;

/// Typed image-upload failure (SPEC-P1-03): the turn boundary projects this
/// into an ACP `internal_error` with structured data, and pushes the user
/// message before finalizing so it survives the replay. The untyped
/// `Err(())` + `StopReason::Refusal` combination is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ImageUploadError {
    /// Zero-based index of the failing image.
    pub index: usize,
    /// Total number of images in the request.
    pub total: usize,
    /// Human-readable failure reason from the provider.
    pub message: String,
}

pub(crate) async fn upload_images(
    llm: &dyn LlmProvider,
    cx: Option<&ConnectionTo<Client>>,
    session_id: &SessionId,
    images: &[(String, String)],
) -> Result<Vec<String>, ImageUploadError> {
    let mut refs = Vec::new();
    if images.is_empty() {
        return Ok(refs);
    }
    let notify = |update: SessionUpdate| {
        if let Some(cx) = cx {
            safe_session_update(cx, session_id, update);
        }
    };
    let total = images.len();
    let upload_call_id = ToolCallId::from(format!("call_{}", uuid::Uuid::new_v4().simple()));
    notify(SessionUpdate::ToolCall(
        ToolCall::new(upload_call_id.clone(), format!("Upload {total} image(s)"))
            .kind(ToolKind::Fetch)
            .status(ToolCallStatus::InProgress),
    ));
    for (index, (base64, mime)) in images.iter().enumerate() {
        match llm.upload_image(base64, mime).await {
            Ok(reference) => refs.push(reference),
            Err(error) => {
                let message = error.to_string();
                let content = vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
                    format!("Upload image {}/{} failed: {message}", index + 1, total),
                )))];
                notify(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                    upload_call_id.clone(),
                    ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Failed)
                        .content(content),
                )));
                return Err(ImageUploadError {
                    index,
                    total,
                    message,
                });
            }
        }
    }
    let content = vec![ToolCallContent::from(ContentBlock::Text(TextContent::new(
        format!("{total} image(s) uploaded successfully"),
    )))];
    notify(SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
        upload_call_id,
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .content(content),
    )));
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_runtime::{LlmError, LlmModelInfo, ModelRequest};

    struct NeverUploadLlm;

    #[async_trait::async_trait]
    impl LlmProvider for NeverUploadLlm {
        async fn stream(
            &self,
            _request: ModelRequest,
        ) -> Result<agent_runtime::LlmStream, LlmError> {
            Err(LlmError::Provider("unused".into()))
        }

        async fn upload_image(&self, _base64: &str, _mime: &str) -> Result<String, LlmError> {
            Ok("ref-ok".to_string())
        }

        fn model_info(&self, _model: &str) -> LlmModelInfo {
            LlmModelInfo::default()
        }
    }

    struct AlwaysFailingLlm;

    #[async_trait::async_trait]
    impl LlmProvider for AlwaysFailingLlm {
        async fn stream(
            &self,
            _request: ModelRequest,
        ) -> Result<agent_runtime::LlmStream, LlmError> {
            Err(LlmError::Provider("unused".into()))
        }

        async fn upload_image(&self, _base64: &str, _mime: &str) -> Result<String, LlmError> {
            Err(LlmError::Upload("disk quota exceeded".into()))
        }

        fn model_info(&self, _model: &str) -> LlmModelInfo {
            LlmModelInfo::default()
        }
    }

    fn session_id() -> SessionId {
        SessionId::from("sess_x".to_string())
    }

    #[tokio::test]
    async fn failed_upload_returns_a_typed_error_with_index_and_total() {
        // No connection is passed: the notification path is inert and the
        // typed result is what the turn boundary consumes.
        let images = vec![
            ("aaaa".to_string(), "image/png".to_string()),
            ("bbbb".to_string(), "image/png".to_string()),
        ];
        let error = upload_images(&AlwaysFailingLlm, None, &session_id(), &images)
            .await
            .expect_err("failing upload must surface as a typed error");
        assert_eq!(error.index, 0);
        assert_eq!(error.total, 2);
        assert!(error.message.contains("disk quota exceeded"), "{error:?}");
    }

    #[tokio::test]
    async fn successful_uploads_return_references() {
        let images = vec![("aaaa".to_string(), "image/png".to_string())];
        let refs = upload_images(&NeverUploadLlm, None, &session_id(), &images)
            .await
            .expect("successful upload must return references");
        assert_eq!(refs, vec!["ref-ok".to_string()]);
    }
}
