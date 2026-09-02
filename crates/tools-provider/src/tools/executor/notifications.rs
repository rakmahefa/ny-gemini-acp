use agent_client_protocol::schema::v1::{SessionId, SessionNotification, SessionUpdate};
use agent_client_protocol::{Client, ConnectionTo};

pub fn safe_session_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    update: SessionUpdate,
) {
    let _ = cx.send_notification(SessionNotification::new(session_id.clone(), update));
}
