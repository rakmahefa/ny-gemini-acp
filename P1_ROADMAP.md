# P1 ROADMAP — Runtime Integrity & Robustness

## Objectif

Après validation complète des P0, traiter les défauts importants qui n'impliquent pas une rupture immédiate de la frontière de sécurité mais qui peuvent encore produire des états incohérents, des pertes de cancellation, des résultats d'outils trompeurs ou des divergences entre runtime et transport.

## Préconditions

- P0-1 à P0-5 validés par `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings` vert.
- `cargo fmt --check` et `cargo check --workspace` verts.

## P1-1 — Tool result semantics

**Cible :** un exit code processus non nul doit être représenté comme `ToolCallResult::error` / `is_ok = false`, sans perdre stdout/stderr ni le code retour.

**Tests :** succès, exit non nul, signal, timeout.

## P1-2 — SemanticEvent rejection propagation

**Cible :** aucun appel à `tool_call_requested`, `permission_requested`, `tool_execution_started` ou `tool_result_received` ne doit ignorer silencieusement un refus sémantique.

**Tests :** chaque rejet doit produire une erreur structurée et un terminal event cohérent.

## P1-3 — LLM cancellation boundary

**Cible :** le contrat `LlmProvider` doit rendre annulable l'établissement du stream, pas seulement sa consommation.

**Tests :** provider bloqué avant création du stream, cancellation concurrente, cancellation après émission partielle.

## P1-4 — Process tree cancellation / timeout

**Cible :** un timeout ou une cancellation shell doit terminer le groupe de processus et éviter les descendants orphelins.

**Tests :** arbre `sh -> child -> grandchild`, timeout, cancellation, cleanup.

## P1-5 — Tool identity collision / MCP precedence

**Cible :** aucun outil MCP ne doit masquer silencieusement un builtin ; les identités d'outils doivent être uniques et déterministes.

**Tests :** collision de noms, lookup, execution, UI model, replay.

## P1-6 — Persistence transaction consistency

**Cible :** aligner snapshot, session principale et état retourné au caller ; définir explicitement la stratégie de récupération après crash entre deux écritures.

**Tests :** panne simulée entre snapshot et session, reprise, génération concurrente.

## P1-7 — Busy ownership robustness

**Cible :** éviter les faux positifs liés au PID reuse et renforcer l'ownership du turn lock.

**Tests :** PID stale, PID réutilisé simulé, crash owner.

## P1-8 — Error-path panic elimination

**Cible :** éliminer les `expect()`/`unwrap()` sur les frontières runtime où une violation d'invariant doit produire une erreur structurée.

**Tests :** chaque invariant cassé doit retourner une erreur et ne jamais tuer le runtime.

## P1-9 — Turn result equals committed state

**Cible :** le `TurnExecutionResult.session` doit refléter l'état final effectivement committé, y compris les métadonnées de finalisation.

## P1-10 — Lock scope reduction

**Cible :** ne pas conserver les write locks globaux pendant des I/O évitables.

**Validation :** tests de concurrence et absence de régression fonctionnelle.

## Sortie P1

```text
P1-1 Tool result semantics       ⏳
P1-2 Event rejection propagation ⏳
P1-3 LLM cancellation            ⏳
P1-4 Process tree cleanup        ⏳
P1-5 MCP identity                ⏳
P1-6 Persistence consistency     ⏳
P1-7 Busy ownership              ⏳
P1-8 Panic elimination           ⏳
P1-9 Committed result            ⏳
P1-10 Lock scope                 ⏳
```
