//! Handler notification `session/cancel`.

use agent_client_protocol::schema::v1::CancelNotification;
use agent_client_protocol::Error as AcpError;
use agent_runtime::AppState;

/// D-08 : `session/cancel` est une **notification** — aucune réponse ne peut
/// être renvoyée au client, une `Err` serait donc jetée par le transport sans
/// jamais être visible. On journalise les problèmes (id invalide, échec du
/// cancel) et on retourne toujours `Ok(())`.
pub async fn handle(notif: CancelNotification, state: &AppState) -> Result<(), AcpError> {
    tracing::info!(session = %notif.session_id, "session/cancel");

    if !crate::handlers::session::is_valid_session_id(&notif.session_id.0) {
        tracing::warn!(
            session = %notif.session_id,
            "session/cancel rejected: invalid session id (notification, no response)"
        );
        return Ok(());
    }

    if let Err(error) = state.turns.cancel(&notif.session_id.0).await {
        tracing::warn!(
            session = %notif.session_id,
            error = %error,
            "session/cancel failed (notification, no response)"
        );
    }
    Ok(())
}
