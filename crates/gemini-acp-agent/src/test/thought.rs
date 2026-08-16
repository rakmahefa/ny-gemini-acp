use super::*;

#[test]
fn non_thinking_model_emits_response_events_directly() {
    let mut stream = ThoughtStream::new(false);
    assert_eq!(
        stream.feed("Bonjour"),
        vec![ThoughtEvent::ResponseChunk("Bonjour".into())]
    );
    assert_eq!(
        stream.feed(" le monde"),
        vec![ThoughtEvent::ResponseChunk(" le monde".into())]
    );
    assert_eq!(stream.phase(), ThoughtPhase::Response);
}

#[test]
fn thinking_model_without_explicit_thought_envelope_preserves_response() {
    let mut stream = ThoughtStream::new(true);
    assert!(stream.feed("Voici la réponse finale").is_empty());
    let tail = stream.feed(" avec plus de détails");
    assert_eq!(
        tail,
        vec![ThoughtEvent::ResponseChunk("Voici la réponse finale avec plus de détails".into())]
    );
    assert_eq!(stream.phase(), ThoughtPhase::Response);
    assert!(!stream.has_emitted_thought());
}

#[test]
fn thinking_model_emits_explicit_thought_then_response() {
    let mut stream = ThoughtStream::new(true);
    assert_eq!(stream.feed("<thinking>"), vec![ThoughtEvent::ThoughtStart]);
    let first = stream.feed("Voici une pensée suffisamment longue pour dépasser la ");
    assert_eq!(first.len(), 1);
    assert!(matches!(first[0], ThoughtEvent::ThoughtChunk(_)));

    let second = stream.feed("fenêtre de garde</thinking>Voici");
    assert_eq!(
        second,
        vec![
            ThoughtEvent::ThoughtChunk("fenêtre de garde".into()),
            ThoughtEvent::ThoughtEnd,
            ThoughtEvent::ResponseChunk("Voici".into()),
        ]
    );
    assert_eq!(stream.phase(), ThoughtPhase::Response);
    assert!(stream.has_emitted_thought());

    assert_eq!(
        stream.feed(" le résultat"),
        vec![ThoughtEvent::ResponseChunk(" le résultat".into())]
    );
}

#[test]
fn xml_open_marker_split_across_deltas_is_atomic() {
    let mut stream = ThoughtStream::new(true);
    assert!(stream.feed("<thi").is_empty());
    assert_eq!(
        stream.feed("nking>Réflexion").first(),
        Some(&ThoughtEvent::ThoughtStart)
    );
    let tail = stream.feed(" puis </thi");
    assert!(tail.iter().any(|event| match event {
        ThoughtEvent::ThoughtChunk(text) => text.contains("Réflexion"),
        _ => false,
    }));
    assert_eq!(
        stream.feed("nking>Réponse"),
        vec![
            ThoughtEvent::ThoughtEnd,
            ThoughtEvent::ResponseChunk("Réponse".into()),
        ]
    );
}

#[test]
fn opening_marker_is_never_exposed_to_consumers() {
    let mut stream = ThoughtStream::new(true);
    assert_eq!(stream.feed("<thinking>raisonnement"), vec![ThoughtEvent::ThoughtStart]);
    assert_eq!(
        stream.finish(),
        vec![
            ThoughtEvent::ThoughtChunk("raisonnement".into()),
            ThoughtEvent::ThoughtEnd,
        ]
    );
}

#[test]
fn thought_start_marker_variants_are_supported() {
    for marker in ["<think>", "[Thinking]:", "[thinking]:"] {
        let mut stream = ThoughtStream::new(true);
        assert_eq!(stream.feed(marker), vec![ThoughtEvent::ThoughtStart]);
        assert_eq!(
            stream.feed("raisonnement</think>Réponse"),
            vec![
                ThoughtEvent::ThoughtChunk("raisonnement".into()),
                ThoughtEvent::ThoughtEnd,
                ThoughtEvent::ResponseChunk("Réponse".into()),
            ]
        );
    }
}

#[test]
fn closing_marker_split_across_deltas_is_atomic() {
    let mut stream = ThoughtStream::new(true);
    assert_eq!(stream.feed("<thinking>pensée"), vec![ThoughtEvent::ThoughtStart]);
    assert!(stream.feed(" utile </thi").is_empty());
    assert_eq!(
        stream.feed("nking>Réponse"),
        vec![
            ThoughtEvent::ThoughtChunk("pensée utile ".into()),
            ThoughtEvent::ThoughtEnd,
            ThoughtEvent::ResponseChunk("Réponse".into()),
        ]
    );
}

#[test]
fn finish_without_explicit_thought_boundary_emits_response() {
    let mut stream = ThoughtStream::new(true);
    stream.feed("Réponse générée par le modèle");
    assert_eq!(
        stream.finish(),
        vec![ThoughtEvent::ResponseChunk("Réponse générée par le modèle".into())]
    );
    assert!(!stream.has_emitted_thought());
    assert_eq!(stream.phase(), ThoughtPhase::Completed);
}

#[test]
fn finish_is_idempotent() {
    let mut stream = ThoughtStream::new(true);
    stream.feed("<thinking>pensée");
    assert_eq!(
        stream.finish(),
        vec![
            ThoughtEvent::ThoughtChunk("pensée".into()),
            ThoughtEvent::ThoughtEnd,
        ]
    );
    assert!(stream.finish().is_empty());
    assert_eq!(stream.phase(), ThoughtPhase::Completed);
}

#[test]
fn legacy_splitter_facade_preserves_existing_contract() {
    let mut splitter = ThoughtSplitter::new(true);
    let (thought, message) = splitter.feed("<thinking>Réflexion suffisamment longue pour être émise");
    assert!(!thought.is_empty());
    assert!(message.is_empty());

    let (thought, message) = splitter.feed("</thinking>Voici");
    assert_eq!(thought, "");
    assert_eq!(message, "Voici");
    assert!(splitter.has_emitted_thought());
}
