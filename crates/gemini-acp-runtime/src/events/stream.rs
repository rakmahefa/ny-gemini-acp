use super::AcpSemanticEvent;

#[derive(Debug, Default)]
pub struct EventStream {
    events: Vec<AcpSemanticEvent>,
}

impl EventStream {
    pub fn push(&mut self, event: AcpSemanticEvent) {
        self.events.push(event);
    }

    pub fn iter(&self) -> impl Iterator<Item = &AcpSemanticEvent> {
        self.events.iter()
    }
}
