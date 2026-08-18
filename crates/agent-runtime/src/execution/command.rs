/// Commands understood by an agent execution thread.
///
/// The command layer is intentionally small and provider/protocol-neutral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCommand {
    Stop,
}
