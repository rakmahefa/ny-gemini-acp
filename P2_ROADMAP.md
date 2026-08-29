# P2 ROADMAP — Consolidation, Quality & Operability

## Objectif

Après P0/P1, consolider le projet pour améliorer la maintenabilité, l'observabilité, la reproductibilité et la qualité des contrats sans modifier inutilement l'architecture provider-neutral.

## P2-1 — CI complète et reproductible

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace`
- `cargo clippy --workspace --all-targets -- -D warnings`
- tests ciblés sécurité / intégrité
- matrix minimale Linux stable

## P2-2 — Test matrix des invariants sémantiques

Construire une matrice explicite de séquences valides/invalides :

```text
TurnStarted
AssistantStarted / Delta / Completed
ThinkingStarted / Delta / Completed
ToolRequested → Permission → Execution → Result
TurnCompleted / TurnCancelled / TurnFailed
```

Inclure transport absent, transport déconnecté et ordre invalide.

## P2-3 — Replay / audit deterministe

Définir un format de journal sémantique permettant de vérifier :

- monotonie des séquences ;
- identité session/turn/tool ;
- terminalité unique ;
- compatibilité projection ACP ;
- reconstruction d'un diagnostic après incident.

## P2-4 — Observabilité structurée

Ajouter des événements/logs structurés pour les transitions refusées, les erreurs de transport, les timeouts outils, les cancellations et les échecs de persistance.

## P2-5 — Documentation des contrats

Documenter explicitement :

- frontières de sécurité applicative ;
- limites du confinement sans primitives OS ;
- garanties de persistance ;
- sémantique des tool results ;
- cancellation semantics ;
- ownership des identifiants.

## P2-6 — Fuzzing / property testing

Cibles prioritaires :

- parser shell ;
- normalisation de commandes ;
- transitions `TurnIntegrity` ;
- corrélation des tool IDs ;
- désérialisation/persistance.

## P2-7 — Concurrency stress tests

Ajouter des tests de charge légère pour :

- turns concurrents ;
- abonnements/déconnexions ACP ;
- persistance ;
- cancellation pendant tool execution ;
- MCP lookup/call concurrence.

## P2-8 — Dependency hygiene

- audit des dépendances ;
- versions minimales compatibles ;
- suppression des dépendances mortes ;
- vérification des features optionnelles.

## P2-9 — API ergonomics

Réduire les APIs qui permettent des états impossibles, privilégier des types forts (`SessionId`, `TurnId`, `ToolCallId`) et rendre les contrats internes impossibles à contourner par construction lorsque cela est raisonnable.

## P2-10 — Release readiness

Préparer :

- checklist release ;
- changelog ;
- compatibilité ACP ;
- migration/persistence policy ;
- diagnostics utilisateur ;
- rollback procedure.

## Sortie P2

```text
P2-1 CI reproducible       ⏳
P2-2 Semantic test matrix  ⏳
P2-3 Replay/audit           ⏳
P2-4 Observability          ⏳
P2-5 Contract docs          ⏳
P2-6 Fuzzing                ⏳
P2-7 Concurrency tests      ⏳
P2-8 Dependency hygiene    ⏳
P2-9 API ergonomics        ⏳
P2-10 Release readiness    ⏳
```
