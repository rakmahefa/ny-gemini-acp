# gemini-acp-encaps

Foundation for encapsulating the concurrent execution lifecycle of ACP work.

## Responsibility

`gemini-acp-encaps` owns:

- worker lifecycle (`Created → Starting → Running → Stopping → Stopped/Failed`);
- a single Tokio task ownership boundary;
- cooperative cancellation shared by the thread and its children;
- an internal command channel;
- observable lifecycle state through `watch`.

It does **not** own ACP protocol handlers, Gemini clients, sessions, persistence,
or tools. Those remain in `gemini-acp-agent` and `gemini-acp-runtime`.

## Migration direction

The next integration step is to move the current `session/prompt` turn
orchestration behind `AcpThread` and introduce an `AcpTurn` abstraction. The
existing agent layer should then become an adapter from ACP requests to
encapsulated commands rather than owning task lifecycle directly.
