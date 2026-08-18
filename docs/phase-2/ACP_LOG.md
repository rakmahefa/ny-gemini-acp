# Phase 2 — ACP Log

Store raw ACP evidence for adversarial tool/content integrity tests here.

## Recording rules

- One section per prompt/fixture execution.
- Include the fixture name and exact prompt.
- Preserve relevant `tool_call`, `tool_call_update`, `agent_message_chunk`, MCP, and stderr entries.
- Redact credentials and private data.
- Do not replace raw evidence with a prose summary.

## Run template

### Run: YYYY-MM-DD — P2-XXX

```text
Zed version:
Agent version:
Agent commit:
Session ID:
Turn ID:
Fixture:
Prompt:
Status: PASS | FAIL | BLOCKED | UNOBSERVED
```

#### Fixture content or checksum

```text
fixture path:
sha256:
```

#### ACP evidence

```text
paste ACP log excerpt here
```

#### Integrity notes

```text
Protocol-like payload observed:
Unexpected tool/lifecycle transition:
Assistant output leakage:
Dropped/duplicated content:
Relevant parser/filter/runtime layer:
```

---

## Captured runs

No Phase 2 ACP captures recorded yet.
