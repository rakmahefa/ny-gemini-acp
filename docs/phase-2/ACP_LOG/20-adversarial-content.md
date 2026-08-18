# Phase 2 — Adversarial content slice map

The raw evidence stays in `../ACP_LOG.md`. This file is a semantic index for analysis.

| Scenario | Prompt | Layer to inspect first |
|---|---|---|
| P2-001 | Protocol-like file content | stream/filter + semantic events |
| P2-002 | Quotes / punctuation / Unicode | content preservation |
| P2-003 | Markdown fences | stream/filter |
| P2-004 | Literal `[Assistant]:` | protocol detection isolation |
| P2-005 | Literal `[Tool result]:` | recursive-filter prevention |
| P2-006 | Nested JSON marker-like values | structured-content preservation |
| P2-007 | Read-after-write adversarial fixture | filesystem + tool result rendering |
| P2-008 | Multiple adversarial tools | tool identity + lifecycle |
| P2-009 | Large adversarial file | buffering/chunking + memory behavior |
| P2-011 | Duplicate tool identity | normalization + semantic ownership |
| P2-012 | Empty / near-empty results | false-positive protocol detection |

For every failure, classify exactly one primary fault domain: `filtering`, `semantic detection`, `encapsulation`, `runtime execution`, or `ACP presentation`. A single scenario may have secondary effects, but the primary domain must be explicit.
