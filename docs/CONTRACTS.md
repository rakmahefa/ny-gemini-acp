# Runtime contract surface

This document is the normative application-level contract for the provider-neutral runtime.

## Security boundary

`agent-runtime` validates semantic lifecycle and tool identity. The ACP adaptor is a transport boundary and does not parse model tool syntax. Shell policy and command normalization are defensive application policies, not OS isolation.

The runtime MUST NOT claim host confinement unless an OS-level sandbox is actually configured and enforced.

## Persistence guarantees

Persisted session state is finalized through the runtime store boundary. A successful turn finalization means the store accepted the final state according to its atomic-write and synchronization contract. Persistence failures remain explicit runtime errors; they MUST NOT be silently converted into successful turns.

## Tool-result semantics

A `ToolResultReceived` event belongs to exactly one semantic tool call identity. Tool output is data and MUST remain separate from protocol syntax. Permission denial, cancellation and execution result are distinct terminal outcomes of the tool lifecycle.

## Cancellation semantics

Cancellation is terminal at the turn level. Open semantic scopes are closed before `TurnCancelled` is emitted, and open tool calls are terminalized as cancelled rather than fabricated as successful results.

## Failure semantics

`TurnFailed` is terminal. The runtime may preserve the underlying structured error for diagnostics while exposing only protocol-safe error data at the ACP boundary.

## Identifier ownership

`SessionId` identifies a session, `TurnId` identifies one turn within that session, and `ToolCallId` identifies one tool invocation within a turn. Tool identifiers are owned by the semantic runtime after validation and are never inferred from arbitrary tool-result text.

## Ordering and replay

Every semantic event carries a monotonically increasing per-turn sequence. A replay journal is valid only when session/turn identity is stable, sequence numbers are contiguous from zero, and exactly one terminal event occurs at the end.

## ACP projection

The ACP layer consumes validated semantic events and projects them to ACP-native messages. ACP transport failure is observable and MUST prevent a successful mandatory transport publication.
