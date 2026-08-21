use super::{EventBus, EventContext};
use crate::events::SemanticEvent;

#[tokio::test]
async fn publishes_events_to_global_subscribers() {
    let bus = EventBus::new();
    let mut receiver = bus.subscribe();

    assert_eq!(
        bus.publish_global(SemanticEvent::TurnStarted {
            context: EventContext::new("session", "turn", 1),
        }),
        1
    );

    let received = receiver.recv().await.expect("event received");
    assert!(matches!(received, SemanticEvent::TurnStarted { .. }));
}

#[tokio::test]
async fn publishes_events_to_required_turn_subscriber() {
    let bus = EventBus::new();
    let mut receiver = bus.subscribe_turn("turn");

    bus.publish_turn(SemanticEvent::TurnStarted {
        context: EventContext::new("session", "turn", 1),
    })
    .expect("event should have a turn receiver");

    let received = receiver.recv().await.expect("event received");
    assert!(matches!(received, SemanticEvent::TurnStarted { .. }));
}
