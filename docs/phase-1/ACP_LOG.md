# Phase 1 — ACP Log

Store raw ACP evidence for semantic lifecycle and tool-state validation here.

## Recording rules

- One section per prompt execution.
- Preserve the order of ACP messages and relevant stderr lifecycle errors.
- Redact credentials and private payloads.
- Record the exact agent commit and Zed version.
- Keep interpretation and conclusions in the Phase 1 README or dedicated design notes; this file is evidence-first.

## Run template

### Run: YYYY-MM-DD — P1-XXX

```text
Zed version:
Agent version:
Agent commit:
Session ID:
Turn ID:
Prompt:
Status: PASS | FAIL | BLOCKED | UNOBSERVED
```

#### ACP evidence

```text
paste ACP log excerpt here
```

#### Lifecycle notes

```text
Observed tool IDs:
Observed state transitions:
Permission event:
Execution event:
Result event:
Terminal outcome:
Unexpected semantic-event errors:
```

---

## Captured runs

No Phase 1 ACP captures recorded yet.
