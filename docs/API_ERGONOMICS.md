# API ergonomics and invariant boundaries

The public runtime API is intentionally narrow around semantic identity and turn lifecycle.

## Strong identifiers

Use `SessionId`, `TurnId` and `ToolCallId` rather than raw strings at API boundaries. Conversion from external protocol data happens once, at the boundary that owns validation.

## Turn lifecycle

`TurnEventEmitter` owns the mutable semantic state machine. Callers do not receive mutable access to its internal `TurnIntegrity`, tool phases, or identity bindings.

A transition that violates the lifecycle returns `false` and leaves the committed state and sequence unchanged. This gives callers a simple non-panicking contract while preventing partial state mutation.

## Terminality

Once a turn reaches `TurnPhase::Terminal`, subsequent lifecycle transitions are rejected. Cancellation and failure close open semantic scopes before terminal emission.

## Transport boundary

Mandatory transport mode is explicit through `TurnEventEmitter::new_with_required_transport`. This prevents callers from accidentally treating a missing ACP turn subscriber as a successful protocol publication.

## Replay boundary

`SemanticJournal` accepts only events with contiguous per-turn sequences. The journal API exposes immutable event views and deterministic JSONL serialization; callers cannot mutate the internal vector in place.

These constraints intentionally trade a small amount of API ceremony for stronger invariants and easier reasoning about impossible states.
