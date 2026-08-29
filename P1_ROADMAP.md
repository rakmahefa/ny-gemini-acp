# P1 ROADMAP — Runtime Integrity & Robustness

## Objectif

Après validation complète des P0, traiter les défauts importants qui n'impliquent pas une rupture immédiate de la frontière de sécurité mais qui peuvent encore produire des états incohérents, des pertes de cancellation, des résultats d'outils trompeurs ou des divergences entre runtime et transport.

## Préconditions

- P0-1 à P0-5 validés par `cargo test --workspace`.
- `cargo clippy --workspace --all-targets -- -D warnings` vert.
- `cargo fmt --check` et `cargo check --workspace` verts.

## P1-1 — Tool result semantics

**Implémentation :** ✅

- exit code non nul et terminaison par signal sont des erreurs sémantiques ;
- stdout/stderr et le statut restent préservés ;
- timeout explicite comme résultat en échec.

**Validation locale :** ✅

## P1-2 — SemanticEvent rejection propagation

**Implémentation :** ✅

Les rejets sémantiques critiques sont propagés comme `AgentLoopError::SemanticEventRejected` et le turn est terminalisé en échec lorsque cela est encore possible.

**Validation locale :** ✅

## P1-3 — LLM cancellation boundary

**Implémentation :** ✅

`LlmProvider::stream_with_cancellation` rend annulable la phase d'établissement du stream et `AgentLoop` mappe cette cancellation vers `TurnCancelled`.

**Validation locale :** ✅

## P1-4 — Process tree cancellation / timeout

**Implémentation :** ✅ pour le backend shell Unix.

Le shell crée un process group dédié et le timeout tue le groupe complet. La sortie de la commande reste représentée comme résultat d'outil cohérent.

**Validation locale :** ✅

## P1-5 — Tool identity collision / MCP precedence

**Implémentation :** ✅

- doublons builtin refusés ;
- collision MCP/builtin transformée en erreur de configuration ;
- builtin prioritaire au dispatch ;
- définitions triées selon une identité déterministe.

**Validation locale :** ✅

## P1-6 — Persistence transaction consistency

**Implémentation :** ✅

La session canonique est désormais persistée avant la création/prune des snapshots. Un snapshot défaillant ne peut donc plus masquer ou bloquer le commit de l'état canonique ; le snapshot est explicitement traité comme artefact de récupération.

Les écritures de session restent atomiques via fichier temporaire synchronisé puis renommé.

**Validation locale :** ✅

## P1-7 — Busy ownership robustness

**Implémentation :** ✅

Le sentinel `.busy` enregistre désormais le PID ainsi que le temps de démarrage du processus lorsque disponible sous Linux. La récupération considère un PID réutilisé comme un owner différent si le start time ne correspond.

**Validation locale :** ✅

## P1-8 — Error-path panic elimination

**Implémentation :** ✅ sur la gestion d'état de lifecycle tool.

Les mutex globaux du lifecycle/cancellation/partial-output ne paniquent plus si un lock est empoisonné ; le guard empoisonné est récupéré et un avertissement est journalisé. La sérialisation de l'enveloppe de résultat possède également un fallback non-panique.

**Validation locale :** ✅

## P1-9 — Turn result equals committed state

**Implémentation :** ✅

Après finalisation, `TurnExecutionResult.session` est relu depuis le `Store`. Le caller reçoit donc l'état canonique effectivement committé, incluant les métadonnées de finalisation (`updated_at`, `turn_count`, normalisation de l'historique).

**Validation locale :** ✅

## P1-10 — Lock scope reduction

**Implémentation :** ✅ sur `begin_turn`.

Le verrou global mémoire n'est plus conservé pendant l'acquisition du sentinel filesystem. L'I/O d'acquisition est effectué avant le write lock global, réduisant la contention entre sessions/tours.

**Validation locale :** ✅

## Sortie P1

```text
P1-1 Tool result semantics       ✅
P1-2 Event rejection propagation ✅
P1-3 LLM cancellation            ✅
P1-4 Process tree cleanup        ✅
P1-5 MCP identity                ✅
P1-6 Persistence consistency     ✅
P1-7 Busy ownership              ✅
P1-8 Panic elimination           ✅
P1-9 Committed result            ✅
P1-10 Lock scope                 ✅
```

## Validation de sortie

```text
cargo fmt --check                ✅
cargo check --workspace          ✅
cargo test --workspace           ✅
cargo clippy --workspace --all-targets -- -D warnings ✅
```

**P1 validée localement. La branche est prête pour merge dans `main`.**
