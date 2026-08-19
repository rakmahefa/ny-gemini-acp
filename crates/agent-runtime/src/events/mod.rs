mod bus;
mod context;
mod emitter;
mod event;
mod integrity;
mod model_projection;
mod stream;
mod tool_sink;

pub use bus::EventBus;
pub use context::{EventContext, ToolEventContext};
pub use emitter::TurnEventEmitter;
pub use event::SemanticEvent;
pub use stream::EventStream;

pub(crate) use model_projection::{
    consume_model_stream, ModelProjectionError, ModelRound, PendingToolCall,
};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod bus_tests;
