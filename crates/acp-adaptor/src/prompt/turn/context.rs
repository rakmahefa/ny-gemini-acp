use agent_client_protocol::schema::v1::{
    ContentBlock, SessionId, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use agent_client_protocol::{Client, ConnectionTo};
use agent_runtime::LlmProvider;
use tools_provider::tools::executor::safe_session_update;

pub(crate) async fn upload_images(
    llm: &dyn LlmProvider,
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    images: &[(String, String)],
) -> Result<Vec<String>, ()> {
    let mut refs = Vec::new();
    if images.is_empty() {
        return Ok(refs);
    }
    let total = images.len();
    let upload_call_id = ToolCallId::from(format!("call_{}", uuid::Uuid::new_v4().simple()));
    safe_session_update(
        cx,
        session_id,
        SessionUpdate::ToolCall(
            ToolCall::new(upload_call_id.clone(), format!("Upload {total} image(s)"))
                .kind(ToolKind::Fetch)
                .status(ToolCallStatus::InProgress),
        ),
    );
    for (index, (base64, mime)) in images.iter().enumerate() {
        match llm.upload_image(base64, mime).await {
            Ok(reference) => refs.push(reference),
            Err(error) => {
                let content = vec![ToolCallContent::Content(
                    agent_client_protocol::schema::v1::Content::new(ContentBlock::Text(
                        TextContent::new(format!(
                            "Upload image {}/{} échoué: {error}",
                            index + 1,
                            total
                        )),
                    )),
                ];
                safe_session_update(
                    cx,
                    session_id,
                    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                        upload_call_id.clone(),
                        ToolCallUpdateFields::new()
                            .status(ToolCallStatus::Failed)
                            .content(content),
                    )),
                );
                return Err(());
            }
        }
    }
    let content = vec![ToolCallContent::Content(
        agent_client_protocol::schema::v1::Content::new(ContentBlock::Text(TextContent::new(
            format!("{total} image(s) uploadée(s) avec succès"),
        ))),
    )];
    safe_session_update(
        cx,
        session_id,
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            upload_call_id,
            ToolCallUpdateFields::new()
                .status(ToolCallStatus::Completed)
                .content(content),
        )),
    );
    Ok(refs)
}
