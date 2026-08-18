use super::SemanticEvent;

#[derive(Debug, Default)]
pub struct EventStream {
    events: Vec<SemanticEvent>,
}

impl EventStream {
    pub fn push(&mut self, event: SemanticEvent) {
        self.events.push(event);
    }

    pub fn iter(&self) -> impl Iterator<Item = &SemanticEvent> {
        self.events.iter()
    }
}
