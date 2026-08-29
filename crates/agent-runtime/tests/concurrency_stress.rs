use std::sync::Arc;
use std::thread;

use agent_runtime::{EventBus, EventContext, SemanticEvent};

fn event(turn_id: &str, sequence: u64) -> SemanticEvent {
    SemanticEvent::TurnStarted {
        context: EventContext::new("session", turn_id, sequence),
    }
}

#[test]
fn concurrent_turn_publication_preserves_delivery_without_loss() {
    let bus = Arc::new(EventBus::new());
    let mut receiver = bus.subscribe_turn("concurrent");

    const THREADS: usize = 8;
    const EVENTS_PER_THREAD: usize = 1_000;

    let handles = (0..THREADS)
        .map(|thread_id| {
            let bus = Arc::clone(&bus);
            thread::spawn(move || {
                let base = (thread_id * EVENTS_PER_THREAD) as u64;
                for offset in 0..EVENTS_PER_THREAD {
                    bus.publish_turn(event("concurrent", base + offset as u64))
                        .expect("turn transport must remain available");
                }
            })
        })
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().expect("publisher thread must not panic");
    }

    let mut received = 0usize;
    let mut seen = std::collections::HashSet::new();
    while let Ok(event) = receiver.try_recv() {
        let sequence = match event {
            SemanticEvent::TurnStarted { context } => context.sequence,
            _ => unreachable!("stress test publishes only TurnStarted events"),
        };
        assert!(seen.insert(sequence), "duplicate event sequence {sequence}");
        received += 1;
    }

    assert_eq!(received, THREADS * EVENTS_PER_THREAD);
}

#[test]
fn closing_one_turn_does_not_break_another_concurrent_transport() {
    let bus = Arc::new(EventBus::new());
    let mut receiver = bus.subscribe_turn("stable");
    let _transient = bus.subscribe_turn("transient");

    let publisher = {
        let bus = Arc::clone(&bus);
        thread::spawn(move || {
            for sequence in 0..500u64 {
                bus.publish_turn(event("stable", sequence)).unwrap();
                if sequence == 100 {
                    bus.close_turn("transient");
                }
            }
        })
    };

    publisher.join().unwrap();

    let mut count = 0;
    while receiver.try_recv().is_ok() {
        count += 1;
    }
    assert_eq!(count, 500);
}
