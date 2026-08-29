use serde::{Deserialize, Serialize};

use super::SemanticEvent;
use crate::{SessionId, TurnId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplayDiagnostic {
    pub session_id: SessionId,
    pub turn_id: TurnId,
    pub event_count: usize,
    pub last_sequence: Option<u64>,
    pub terminal_event: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SemanticJournal {
    events: Vec<SemanticEvent>,
}

impl SemanticJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: SemanticEvent) -> Result<(), String> {
        self.audit_candidate(&event)?;
        self.events.push(event);
        Ok(())
    }

    pub fn events(&self) -> &[SemanticEvent] {
        &self.events
    }

    pub fn into_events(self) -> Vec<SemanticEvent> {
        self.events
    }

    pub fn audit(&self) -> Result<ReplayDiagnostic, String> {
        let Some(first) = self.events.first() else {
            return Err("semantic journal is empty".to_owned());
        };
        let (session_id, turn_id) = event_identity(first);
        let mut expected_sequence = 0u64;
        let mut terminal_event = None;

        for event in &self.events {
            let (event_session, event_turn) = event_identity(event);
            if event_session != session_id || event_turn != turn_id {
                return Err("semantic journal mixes session/turn identities".to_owned());
            }

            let sequence = event_sequence(event);
            if sequence != expected_sequence {
                return Err(format!(
                    "semantic journal sequence gap: expected {expected_sequence}, got {sequence}"
                ));
            }
            expected_sequence = expected_sequence
                .checked_add(1)
                .ok_or_else(|| "semantic journal sequence overflow".to_owned())?;

            if let Some(kind) = terminal_kind(event) {
                if terminal_event.is_some() {
                    return Err("semantic journal contains multiple terminal events".to_owned());
                }
                terminal_event = Some(kind.to_owned());
            } else if terminal_event.is_some() {
                return Err("semantic journal contains events after terminality".to_owned());
            }
        }

        Ok(ReplayDiagnostic {
            session_id: session_id.clone(),
            turn_id: turn_id.clone(),
            event_count: self.events.len(),
            last_sequence: self.events.last().map(event_sequence),
            terminal_event,
        })
    }

    pub fn to_json_lines(&self) -> Result<String, serde_json::Error> {
        self.events
            .iter()
            .map(serde_json::to_string)
            .collect::<Result<Vec<_>, _>>()
            .map(|lines| lines.join("\n"))
    }

    pub fn from_json_lines(input: &str) -> Result<Self, String> {
        let mut journal = Self::new();
        for (line_number, line) in input.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let event: SemanticEvent = serde_json::from_str(line).map_err(|error| {
                format!("invalid semantic journal line {}: {error}", line_number + 1)
            })?;
            journal.push(event)?;
        }
        Ok(journal)
    }

    fn audit_candidate(&self, event: &SemanticEvent) -> Result<(), String> {
        let (session_id, turn_id) = event_identity(event);
        if let Some(last) = self.events.last() {
            let (last_session, last_turn) = event_identity(last);
            if last_session != session_id || last_turn != turn_id {
                return Err("semantic journal mixes session/turn identities".to_owned());
            }
            let expected = event_sequence(last)
                .checked_add(1)
                .ok_or_else(|| "semantic journal sequence overflow".to_owned())?;
            if event_sequence(event) != expected {
                return Err(format!(
                    "semantic journal sequence gap: expected {expected}, got {}",
                    event_sequence(event)
                ));
            }
            if terminal_kind(last).is_some() {
                return Err("semantic journal is already terminal".to_owned());
            }
        } else if event_sequence(event) != 0 {
            return Err(format!(
                "semantic journal must start at sequence 0, got {}",
                event_sequence(event)
            ));
        }
        Ok(())
    }
}

fn event_context(event: &SemanticEvent) -> &super::EventContext {
    match event {
        SemanticEvent::TurnStarted { context }
        | SemanticEvent::AssistantStarted { context }
        | SemanticEvent::AssistantDelta { context, .. }
        | SemanticEvent::AssistantCompleted { context }
        | SemanticEvent::ThinkingStarted { context }
        | SemanticEvent::ThinkingDelta { context, .. }
        | SemanticEvent::ThinkingCompleted { context }
        | SemanticEvent::TurnCancelled { context }
        | SemanticEvent::TurnFailed { context }
        | SemanticEvent::TurnCompleted { context } => context,
        SemanticEvent::ToolCallRequested { context, .. }
        | SemanticEvent::PermissionRequested { context }
        | SemanticEvent::ToolExecutionStarted { context, .. }
        | SemanticEvent::ToolResultReceived { context, .. } => &context.event,
    }
}

fn event_identity(event: &SemanticEvent) -> (&SessionId, &TurnId) {
    let context = event_context(event);
    (&context.session_id, &context.turn_id)
}

fn event_sequence(event: &SemanticEvent) -> u64 {
    event_context(event).sequence
}

fn terminal_kind(event: &SemanticEvent) -> Option<&'static str> {
    match event {
        SemanticEvent::TurnCancelled { .. } => Some("turn_cancelled"),
        SemanticEvent::TurnFailed { .. } => Some("turn_failed"),
        SemanticEvent::TurnCompleted { .. } => Some("turn_completed"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventContext;

    fn started(sequence: u64) -> SemanticEvent {
        SemanticEvent::TurnStarted {
            context: EventContext::new("session", "turn", sequence),
        }
    }

    #[test]
    fn journal_round_trips_and_audits_deterministically() {
        let mut journal = SemanticJournal::new();
        journal.push(started(0)).unwrap();
        journal
            .push(SemanticEvent::TurnCompleted {
                context: EventContext::new("session", "turn", 1),
            })
            .unwrap();
        let encoded = journal.to_json_lines().unwrap();
        assert_eq!(SemanticJournal::from_json_lines(&encoded).unwrap(), journal);
        assert_eq!(
            journal.audit().unwrap().terminal_event.as_deref(),
            Some("turn_completed")
        );
    }

    #[test]
    fn audit_rejects_sequence_gap() {
        let journal = SemanticJournal {
            events: vec![started(0), started(2)],
        };
        assert!(journal.audit().unwrap_err().contains("sequence gap"));
    }

    #[test]
    fn audit_rejects_events_after_terminality() {
        let journal = SemanticJournal {
            events: vec![
                started(0),
                SemanticEvent::TurnCompleted {
                    context: EventContext::new("session", "turn", 1),
                },
                started(2),
            ],
        };
        assert!(journal.audit().unwrap_err().contains("after terminality"));
    }
}
