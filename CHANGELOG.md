# Changelog

All notable changes to this project are documented here.

## [Unreleased]

### Runtime

- strengthened semantic event lifecycle validation;
- added deterministic semantic journal JSONL replay/audit;
- added structured observability for semantic transport and transition failures;
- added bounded property-like tests for event sequences and journal round-trips;
- added concurrent event transport stress coverage;
- exposed `TurnPhase` as part of the runtime event API.

### Quality

- added reproducible Linux stable validation;
- documented runtime contracts, security boundaries, cancellation and persistence semantics;
- added dependency hygiene policy and audit script;
- added release-readiness checklist.
