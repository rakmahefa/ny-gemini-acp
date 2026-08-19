# Tool semantic contract

The agent runtime owns the canonical model-facing tool protocol.

## Tool calls

A model emits one fenced JSON object per invocation:

```tool_call
{"name":"<tool_name>","id":"<call_id>","arguments":{}}
```

`name` and `id` must be non-empty. `id` is unique within a turn. `arguments` is a JSON object.

## Tool results

A tool result is serialized as one JSON line:

```text
[Tool result]: {"tool":"...","id":"...","status":"ok|error","content":"..."}
```

`content` is data and is JSON-escaped. It must never be interpreted as protocol syntax.

## Compatibility

The semantic parser may continue accepting legacy inline markers and function-call fences for backward compatibility. Writers advertise and emit only the canonical fenced `tool_call` protocol.

## Layering

```text
Gemini raw wire
  -> GeminiFrameDecoder
  -> GeminiSemanticStream
  -> ModelEvent
  -> Agent Runtime
```

The ACP adapter is responsible for protocol transport, not tool-call detection or interpretation.
