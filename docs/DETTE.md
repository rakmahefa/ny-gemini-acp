# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document suit le remboursement des dettes techniques qui menacent les contrats d'architecture, la robustesse des turns, la persistance ou la testabilité du runtime.

Les chantiers suivants sont désormais stabilisés dans `main` :

- typage sémantique (`SessionId`, `TurnId`, `ToolCallId`) ;
- persistance et finalisation des turns ;
- orchestration des turns via `agent-runtime::TurnService` ;
- erreurs structurées et mappings ACP ;
- intégrité du cycle `SemanticEvent` et du transport par tour.

La branche courante `debt/integration-system-tests` poursuit maintenant le remboursement de la dette P2 de tests d'intégration/système.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | `TurnService` extrait, runtime découplé d'ACP |
| P2 | Dette des erreurs structurées | ✅ Terminée | Contrats structurés + mappings ACP terminés |
| P2 | Tests d'intégration/système | 🟡 En progression | Pipeline runtime + projection ACP renforcé ; E2E transport encore à couvrir |
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

# 6. Dette de tests d'intégration — PRIORITÉ 2 🟡 EN PROGRESSION

## Objectif

Démontrer par tests reproductibles que les contrats critiques tiennent sur une chaîne complète :

```text
provider
→ runtime / AgentLoop
→ SemanticEvent lifecycle
→ tool execution
→ Store persistence
→ ACP projection
→ ACP notification payload
```

Le transport réseau/stdio ACP complet reste une étape distincte à couvrir ensuite.

## 6.1 Couverture runtime ajoutée sur `debt/integration-system-tests`

Le test d'intégration :

```text
crates/agent-runtime/tests/turn_pipeline.rs
```

couvre maintenant trois scénarios sans fournisseur externe :

### A. Turn nominal ✅

```text
ScriptedLlm
    ↓
TextDelta
    ↓
AgentLoop
    ↓
SemanticEvent terminal
    ↓
Store::end_turn
    ↓
persisted assistant message
```

Le test vérifie notamment :

- sortie du modèle ;
- nombre de rounds ;
- terminalité du `SemanticEvent` ;
- message assistant persisté ;
- incrément du `turn_count`.

### B. Turn avec outil ✅

```text
ModelEvent::ToolCall
    ↓
canonical call id
    ↓
ToolProvider::call
    ↓
ToolResult
    ↓
second model round
    ↓
final text
    ↓
persistence
```

Le test vérifie :

- exécution réelle du `ToolProvider` de test ;
- conservation de `upstream-42` comme identifiant canonique ;
- deux rounds de modèle ;
- un tool call exécuté ;
- persistance de `ToolCall` et `ToolResult`.

### C. Échec provider ✅

```text
LlmError::Network
    ↓
AgentLoopError::Llm
    ↓
SemanticEvent terminal
    ↓
TurnServiceError::Agent
    ↓
busy state libéré
```

Le test vérifie qu'un échec du provider :

- remonte sous forme structurée ;
- termine le cycle sémantique ;
- finalise le turn ;
- permet immédiatement de commencer le turn suivant avec une génération nouvelle.

## 6.2 Projection ACP et notifications ✅ EN PROGRESSION

La projection dédiée :

```text
crates/acp-adaptor/src/prompt/tool_stream.rs
```

valide déjà les invariants de séquence et d'identité avant transformation ACP :

```text
SemanticEvent
    ↓
SequenceTracker
    ↓
ProjectionAction
    ↓
ACP notification builder
```

La construction des notifications ACP est maintenant factorisée dans `prompt/notify.rs` et réutilisée directement par les fonctions de transport. Les tests couvrent désormais les payloads sérialisés pour :

- `AgentMessageChunk` ;
- `AgentThoughtChunk` ;
- `ToolCall` ;
- `ToolCallUpdate` ;
- `UsageUpdate`.

La couverture vérifie notamment :

- `session_id` et `message_id` ;
- texte assistant/reasoning ;
- `ToolCallId` ;
- entrées/sorties structurées des outils ;
- statut ACP final ;
- présence des types de notifications attendus.

Les tests de projection existants couvrent aussi les pertes de séquence et la conservation des identités d'outil.

## 6.3 Ce qui reste à couvrir

La dette P2 n'est pas encore entièrement remboursée. Restent principalement :

```text
provider
→ runtime
→ SemanticEvent
→ ACP projection
→ ConnectionTo<Client>
→ transport ACP réel
```

ainsi que les scénarios adversariaux de chaîne complète :

```text
cancel
max rounds
empty stream
invalid model sequence
duplicate tool call
semantic event rejection
projection transport close
ACP notification failure
persistence failure
agent + persistence failure
```

Les tests unitaires et d'intégration couvrent déjà une partie de ces contrats ; l'objectif de la dette P2 est de les relier progressivement au chemin de transport réel.

## 6.4 Règle de progression

Une dette P2 est considérée comme remboursée uniquement lorsque la preuve couvre :

```text
unit
→ integration
→ protocol
→ end-to-end
```

Il ne suffit pas qu'un contrat soit testé isolément.

---

# 7. CI — PRIORITÉ 3 / DIFFÉRÉE

La CI GitHub n'est pas actuellement active sur le dépôt.

La validation cible reste :

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

La CI pourra être réintroduite après stabilisation de la matrice P2, afin qu'elle protège les mêmes invariants automatiquement.

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

Ce chantier reste volontairement derrière les tests système.

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
🟡 5. Tests d'intégration / système
        ├── ✅ provider → runtime
        ├── ✅ runtime → SemanticEvent lifecycle
        ├── ✅ tool execution
        ├── ✅ runtime → persistence
        ├── ✅ SemanticEvent → ACP projection
        ├── ✅ ACP notification payloads
        └── ⏳ ConnectionTo<Client> → transport ACP réel
        ↓
6. Sandbox shell
        ↓
7. CI automatisée
```

L'ordre peut être révisé lorsqu'un bug concret ou une contrainte protocolaire révèle une priorité supérieure.

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
