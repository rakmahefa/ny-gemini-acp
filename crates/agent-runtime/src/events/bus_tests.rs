use super::{EventBus, EventContext};
use crate::events::AcpSemanticEvent;

#[tokio::test]
async fn publishes_events_to_subscribers() {
    let bus = EventBus::new();
    let mut receiver = bus.subscribe();

    bus.publish(AcpSemanticEvent::TurnStarted {
        context: EventContext::new("session", "turn", 1),
    })
    .expect("event should have a receiver");

    let received = receiver.recv().await.expect("event received");
    assert!(matches!(received, AcpSemanticEvent::TurnStarted { .. }));
}
