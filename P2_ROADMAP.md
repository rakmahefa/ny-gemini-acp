# P2 ROADMAP — Consolidation, Quality & Operability

## Objectif

Après P0/P1, consolider le projet pour améliorer la maintenabilité, l'observabilité, la reproductibilité et la qualité des contrats sans modifier inutilement l'architecture provider-neutral.

## Avancement P2-1 à P2-4

### P2-1 — CI complète et reproductible

**Implémentation : ✅**

La validation repository est désormais centralisée dans `scripts/validate.sh` et exécute systématiquement :

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

La CI GitHub utilise un runner `ubuntu-latest`, Rust stable, `rustfmt` + `clippy`, cache Cargo, et se déclenche sur `push`, `pull_request` et manuellement.

**Validation CI : ⚠️ environnement**

Une exécution GitHub Actions a échoué avant l'exécution des étapes du job (`steps: []`, aucun runner attribué). Cela indique un problème d'environnement/runner GitHub et non un échec de compilation constaté par les étapes du workflow. La validation locale complète n'a pas pu être exécutée depuis cet environnement.

### P2-2 — Test matrix des invariants sémantiques

**Implémentation : ✅**

Ajout de `crates/agent-runtime/tests/semantic_event_matrix.rs` couvrant les séquences valides et invalides :

```text
TurnStarted
AssistantStarted → Delta → Completed
ThinkingStarted → Delta → Completed
ToolRequested → Permission → Execution → Result
TurnCompleted / TurnCancelled / TurnFailed
```

Cas d'intégrité supplémentaires : transport absent, ordre invalide, double terminalité, conservation de la séquence et projection globale des événements canoniques.

### P2-3 — Replay / audit déterministe

**Base implémentée : ✅**

Ajout de `SemanticJournal` et `ReplayDiagnostic` dans `agent-runtime` :

- séquences strictement monotones à partir de `0` ;
- identité `session_id` / `turn_id` cohérente ;
- terminalité unique ;
- rejet des événements après terminalité ;
- sérialisation JSONL déterministe ;
- relecture JSONL avec validation ;
- diagnostic exploitable après incident.

**Reste à compléter : ⏳**

L'intégration automatique du journal dans le flux runtime et la vérification explicite de compatibilité avec la projection ACP seront traitées dans l'incrément suivant de P2-3.

### P2-4 — Observabilité structurée

**Implémentation initiale : ✅**

`EventBus` émet maintenant des logs structurés pour :

- publication globale d'un `SemanticEvent` ;
- livraison vers le transport d'un turn ;
- transport absent ;
- transport déconnecté ;
- enregistrement et fermeture d'un subscriber ;
- identité session/turn/tool et numéro de séquence.

Les rejets de transitions sémantiques critiques étaient déjà journalisés par `TurnEventEmitter`.

**Reste à compléter : ⏳**

Ajouter les points d'observabilité structurée dédiés aux timeouts d'outils, cancellations et échecs de persistance dans leurs frontières d'exécution respectives.

## P2-5 — Documentation des contrats

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
P2-1 CI reproducible       ⚠️ implémenté / validation runner à corriger
P2-2 Semantic test matrix  ✅ implémenté
P2-3 Replay/audit           ✅ base implémentée / intégration à poursuivre
P2-4 Observability          ✅ base implémentée / hooks runtime à poursuivre
P2-5 Contract docs          ⏳
P2-6 Fuzzing                ⏳
P2-7 Concurrency tests      ⏳
P2-8 Dependency hygiene     ⏳
P2-9 API ergonomics         ⏳
P2-10 Release readiness     ⏳
```

## Validation de sortie P2-1 à P2-4

**À confirmer sur un runner GitHub disponible :**

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

**État de cette branche : P2-1 à P2-4 entamées et documentées. Aucun merge dans `main` n'est effectué à cette étape.**
