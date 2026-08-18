/// Commands understood by the encapsulated ACP worker.
///
/// The foundation intentionally keeps this protocol small. ACP-specific
/// request payloads will be introduced by the agent layer when it migrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCommand {
    Stop,
}
