# Phase 1 — ACP log analysis directory

`../ACP_LOG.md` is the canonical raw capture.

Generated `parts/` must contain complete JSON events, preserving original order. Analyze lifecycle incidents as contiguous event windows so semantic predecessors and terminal transitions remain visible.

Primary groups:

- repeated tool rounds
- replay/duplicate tool identity
- permission/execution/result transitions
- cancellation and terminality
