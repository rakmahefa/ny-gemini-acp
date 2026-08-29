use agent_client_protocol::schema::v1::StopReason;

#[allow(dead_code)]
pub fn map_stop_reason(gemini_finish: Option<&str>) -> StopReason {
    match gemini_finish {
        Some("length") | Some("max_tokens") => StopReason::MaxTokens,
        Some("content_filter") | Some("safety") | Some("block_reason") => StopReason::Refusal,
        _ => StopReason::EndTurn,
    }
}
