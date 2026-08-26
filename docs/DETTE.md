# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document suit le remboursement des dettes techniques qui menacent les contrats d'architecture, la robustesse des turns, la persistance ou la testabilité du runtime.

Les chantiers suivants sont désormais stabilisés dans `main` :

- typage sémantique (`SessionId`, `TurnId`, `ToolCallId`) ;
- persistance et finalisation des turns ;
- orchestration des turns via `agent-runtime::TurnService` ;
- erreurs structurées et mappings ACP ;
- intégrité du cycle `SemanticEvent` et du transport par tour.

La branche courante `debt/integration-system-tests` finalise le remboursement de la dette P2 de tests d'intégration/système.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | `TurnService` extrait, runtime découplé d'ACP |
| P2 | Dette des erreurs structurées | ✅ Terminée | Contrats structurés + mappings ACP terminés |
| P2 | Tests d'intégration/système | ✅ Terminée | Runtime, SemanticEvent, projection ACP et chemins adversariaux validés |
| P3 | Sandbox shell | Différé volontairement | Parsing/normalisation/politique de risque à traiter plus tard |
| P3 | CI automatisée | Différée | Validation manuelle maintenue pour l'instant |

---

# 2. Dette de typage sémantique — PRIORITÉ 1 ✅ TERMINÉE

Les identités sémantiques centrales disposent de types forts :

```rust
SessionId
TurnId
ToolCallId
```

Elles traversent les contrats runtime et les événements sémantiques avec conversions explicites aux frontières protocolaires.

Validation historique :

```text
cargo fmt --check                                      ✅
cargo check --workspace                                ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

---

# 3. Dette de persistance — PRIORITÉ 1 ✅ TERMINÉE

Le modèle de persistance couvre désormais :

- écritures atomiques session/snapshot ;
- `sync_all()` ;
- nettoyage des fichiers temporaires et sentinelles orphelines ;
- cohérence du cache live ;
- contrôle de génération contre les turns obsolètes ;
- finalisation sûre du turn via `Store::end_turn`.

Validation historique :

```text
cargo fmt --check        ✅
cargo check --workspace  ✅
cargo test --workspace   ✅
```

---

# 4. Dette d'orchestration des turns — PRIORITÉ 2 ✅ TERMINÉE

Le traitement d'un turn est désormais porté par `agent-runtime::TurnService`.

```text
ACP request
    ↓
acp-adaptor
    ↓
TurnService
    ├── AgentLoop
    ├── SemanticEvent lifecycle
    ├── provider/tool execution
    └── Store::end_turn
```

Le runtime ne dépend pas des types de présentation ACP.

---

# 5. Dette de gestion des erreurs — PRIORITÉ 2 ✅ TERMINÉE

Les erreurs métier/runtime restent structurées jusqu'à la frontière applicative.

Les principales familles sont maintenant typées :

```text
LlmProviderErrorKind
ToolConfigurationError
AgentActionError
TurnServiceError
AgentLoopError
StoreError
```

Les erreurs ACP utilisent des données machine-readable et distinguent notamment :

```text
agent_loop_failed
turn_finalization_failed
turn_failed_and_finalization_failed
```

Les terminaisons protocolaires (`Cancelled`, `MaxRounds`) restent séparées des erreurs internes.

L'audit des `Result<T, String>` a été mené jusqu'à la frontière ACP ; les conversions textuelles restantes sont volontairement localisées aux façades qui les imposent.

---

# 6. Dette de tests d'intégration — PRIORITÉ 2 ✅ TERMINÉE

## Objectif atteint

Les contrats critiques sont maintenant démontrés par une progression complète de tests :

```text
unit
→ integration
→ protocol
→ end-to-end contract
```

La preuve ne dépend pas d'un fournisseur Gemini réel : les tests utilisent des providers scriptés et exercent les mêmes contrats runtime et ACP que la production.

## 6.1 Pipeline runtime ✅

```text
provider
→ AgentLoop
→ SemanticEvent lifecycle
→ tool execution
→ Store persistence
```

`crates/agent-runtime/tests/turn_pipeline.rs` couvre :

- turn nominal ;
- turn multi-round avec outil ;
- conservation du `ToolCallId` ;
- persistance du `ToolCall` et `ToolResult` ;
- échec provider structuré ;
- terminalisation sémantique ;
- libération du turn et nouvelle génération.

## 6.2 Projection ACP ✅

```text
SemanticEvent
    ↓
turn identity validation
    ↓
sequence validation
    ↓
ProjectionAction
    ↓
ACP notification builder
```

La projection valide maintenant explicitement :

- tour attendu ;
- séquence contiguë ;
- absence de perte avant notification terminale ;
- conservation de l'identité outil ;
- propagation de la cancellation en cas d'intégrité rompue.

## 6.3 Notifications ACP ✅

Les notifications de production sont construites par des fonctions déterministes testables :

```text
AgentMessageChunk
AgentThoughtChunk
ToolCall
ToolCallUpdate
UsageUpdate
```

Les tests de payload vérifient :

- `session_id` / `message_id` ;
- texte assistant et reasoning ;
- `ToolCallId` ;
- input/output structurés ;
- statut ACP ;
- `ToolKind` ;
- métadonnées UI.

## 6.4 Chemins adversariaux ✅

Le moteur de projection couvre désormais explicitement :

```text
transport queue closed
→ ProjectionError::Closed
→ cancellation
```

```text
unexpected turn
→ ProjectionError::UnexpectedTurn
→ cancellation
```

```text
sequence gap
→ ProjectionError::SequenceGap
→ cancellation
→ aucun terminal ACP accepté
```

```text
ACP notification failure
→ ProjectionError::Acp
→ cancellation
```

Ces tests établissent qu'une violation de transport ou d'intégrité ne se transforme jamais silencieusement en une notification ACP apparemment valide.

## 6.5 Transport ACP réel : frontière couverte

Le chemin de production reste :

```text
TurnEventEmitter
→ EventBus per-turn transport
→ prompt::tool_stream::project
→ notify_*()
→ ConnectionTo<Client>::send_notification()
```

Les fonctions `notify_*` utilisées par ce chemin sont les mêmes que celles vérifiées par les tests de payload. Les erreurs de `send_notification()` sont propagées par la projection et déclenchent l'annulation du turn.

Le transport OS/stdio complet de `Agent::builder(...).connect_to(Stdio::new())` relève désormais de la validation de protocole/exécution du binaire, et non plus d'une dette de contrat runtime. Aucun scénario critique du pipeline interne ne dépend d'un mock permissif pour être considéré valide.

## 6.6 Validation de la branche

Validation locale confirmée :

```text
cargo fmt --check                                      ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

La CI GitHub n'est pas active sur le dépôt ; la validation de référence reste donc locale et reproductible par ces trois commandes.

---

# 7. CI — PRIORITÉ 3 / DIFFÉRÉE

La CI GitHub n'est actuellement pas active sur le dépôt.

Validation de référence :

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

La CI pourra être réintroduite ensuite pour protéger automatiquement les invariants désormais couverts par la suite de tests.

---

# 8. Sandbox shell — PRIORITÉ 3 / DIFFÉRÉE

Le sandbox shell conserve encore des heuristiques pour certaines commandes dangereuses.

Direction cible :

```text
commande brute
→ tokenisation / parsing
→ représentation normalisée
→ politique de risque
→ décision d'exécution
```

Ce chantier reste volontairement derrière les contrats d'intégration déjà stabilisés.

---

# 9. Stratégie de remboursement

```text
✅ 1. Dette de typage sémantique
        ↓
✅ 2. Dette de persistance
        ↓
✅ 3. Orchestration des turns
        ↓
✅ 4. Erreurs structurées
        ↓
✅ 5. Tests d'intégration / système
        ├── ✅ provider → runtime
        ├── ✅ runtime → SemanticEvent lifecycle
        ├── ✅ tool execution
        ├── ✅ runtime → persistence
        ├── ✅ SemanticEvent → ACP projection
        ├── ✅ ACP notification payloads
        └── ✅ adversarial transport/projection paths
        ↓
6. Sandbox shell
        ↓
7. CI automatisée
```

---

# 10. Règle générale

La dette technique ne doit pas être remboursée par refactorisation esthétique seule.

Chaque chantier doit apporter au moins un bénéfice concret :

- éliminer une classe d'erreurs ;
- clarifier un contrat architectural ;
- renforcer la robustesse d'un état persistant ;
- réduire le couplage entre couches ;
- améliorer la capacité de test et de maintenance.

Une fonctionnalité n'est considérée comme stabilisée que lorsque son comportement critique est démontré par la bonne couche de test.
