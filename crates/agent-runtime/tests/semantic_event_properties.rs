use agent_runtime::{EventBus, EventContext, SemanticEvent, SemanticJournal};

fn started(sequence: u64) -> SemanticEvent {
    SemanticEvent::TurnStarted {
        context: EventContext::new("session", "turn", sequence),
    }
}

#[test]
fn bounded_sequence_property_accepts_exactly_contiguous_journals() {
    for length in 1..=128usize {
        let mut journal = SemanticJournal::new();
        for sequence in 0..length as u64 {
            journal.push(started(sequence)).unwrap();
        }
        assert_eq!(journal.events().len(), length);
        assert!(journal.audit().is_ok());
    }
}

#[test]
fn every_single_sequence_gap_is_rejected() {
    for gap_at in 1..=64u64 {
        let mut journal = SemanticJournal::new();
        journal.push(started(0)).unwrap();
        let skipped = gap_at + 1;
        for sequence in 1..gap_at {
            journal.push(started(sequence)).unwrap();
        }
        let error = journal.push(started(skipped)).unwrap_err();
        assert!(error.contains("sequence gap"));
    }
}

#[test]
fn jsonl_round_trip_is_stable_for_bounded_event_streams() {
    for length in 1..=64usize {
        let mut journal = SemanticJournal::new();
        for sequence in 0..length as u64 {
            journal.push(started(sequence)).unwrap();
        }
        let encoded = journal.to_json_lines().unwrap();
        let decoded = SemanticJournal::from_json_lines(&encoded).unwrap();
        assert_eq!(decoded.events(), journal.events());
        assert_eq!(decoded.to_json_lines().unwrap(), encoded);
    }
}

#[test]
fn journal_rejects_mixed_turn_identity() {
    let mut journal = SemanticJournal::new();
    journal.push(started(0)).unwrap();
    let mixed = SemanticEvent::TurnStarted {
        context: EventContext::new("session", "other-turn", 1),
    };
    assert!(journal.push(mixed).unwrap_err().contains("session/turn"));
}

#[test]
fn empty_transport_is_explicitly_distinguished_from_journal_validity() {
    let bus = EventBus::new();
    assert!(!bus.has_turn_subscriber("turn"));
    let journal = SemanticJournal::new();
    assert!(journal.audit().is_err());
}
