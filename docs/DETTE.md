# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document recense la dette technique identifiée sur la branche `chore/deep-audit-cleanup` et l'ordre de remboursement retenu.

Les chantiers de typage sémantique, persistance et orchestration des turns sont stabilisés et présents dans `main`. La branche `debt/structured-errors` a terminé le remboursement de la dette de gestion des erreurs structurées, sous réserve de la validation globale finale `fmt/check`.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | `TurnService` extrait, runtime découplé d'ACP |
| P2 | Dette des erreurs structurées | ✅ Terminée | Contrats structurés + mappings ACP + audit des conversions terminés |
| P2 | Dette de tests d'intégration/système | À renforcer | Prochaine dette prioritaire |
| P3 | Sandbox shell | Différé volontairement | À traiter plus tard |
| P3 | CI automatisée | Hors priorité | Validation manuelle par le mainteneur |

---

# 2. Dette de typage sémantique — PRIORITÉ 1 ✅ TERMINÉE

Les identités sémantiques centrales disposent de types forts :

```rust
SessionId
TurnId
ToolCallId
```

Elles sont intégrées aux contrats runtime et aux `SemanticEvent`, avec conversions protocolaires explicites aux frontières ACP.

Validation historique :

```text
cargo fmt --check                                      ✅
cargo check --workspace                                ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

---

# 3. Dette de persistance — PRIORITÉ 1 ✅ TERMINÉE

Le modèle de persistance a été renforcé autour des écritures atomiques session/snapshot, du `sync_all()`, du nettoyage des temporaires orphelins, de la cohérence du cache live et du contrôle de génération contre les tours obsolètes.

Validation historique :

```text
cargo fmt --check   ✅
cargo check --workspace   ✅
cargo test --workspace    ✅
```

---

# 4. Dette d'orchestration des turns — PRIORITÉ 2 ✅ TERMINÉE

Le traitement des turns a été déplacé vers `agent-runtime::TurnService` en conservant une frontière nette entre runtime et protocole ACP.

Le découpage final est :

```text
ACP request
    ↓
acp-adaptor
    ├── routage ACP
    └── orchestration de présentation
            ↓
     agent-runtime::TurnService
            ├── AgentLoop
            ├── SemanticEvent lifecycle
            ├── provider/tool execution
            └── Store::end_turn
```

Le runtime ne connaît ni `PromptRequest`, ni `ConnectionTo<Client>`, ni les types de présentation ACP.

---

# 5. Dette de gestion des erreurs — PRIORITÉ 2 ✅ TERMINÉE

## Objectif

Les erreurs métier/runtime restent structurées jusqu'à la frontière applicative. Les conversions en `String` ou `AcpError` sont réservées aux façades protocolaires qui les nécessitent réellement.

## 5.1 Session / configuration ✅

Le runtime utilise désormais `ToolConfigurationError` et `SessionToolConfigurationError`.

Le contrat canonique est :

```text
Tool configuration
    ↓
ToolConfigurationError
    ↓
SessionToolConfigurationError
    ↓
frontière applicative / ACP
```

`configure_mcp_typed()` conserve l'erreur structurée. `configure_mcp()` reste une façade de compatibilité ACP qui réalise volontairement la conversion textuelle au bord du système.

## 5.2 Persistance / finalisation ✅

La finalisation d'un tour distingue explicitement :

```text
AgentLoopError
StoreError
AgentAndPersistence { agent, persistence }
```

`TurnServiceError` conserve l'erreur primaire et signale séparément une défaillance de persistance lorsque les deux surviennent.

## 5.3 Provider LLM ✅

Le runtime expose `LlmProviderErrorKind` :

```text
Authentication
InvalidRequest
ModelUnavailable
Network
Upstream
StreamDivergence
Upload
```

Le mapping Gemini préserve ces catégories au lieu de réduire systématiquement les erreurs à `Provider(String)`.

## 5.4 Outillage / MCP ✅

`ToolConfigurationError` distingue :

```text
InvalidConfiguration
Transport
Protocol
Remote { code, message }
MessageTooLarge
PaginationLimit
Provider
```

Le mapping `McpError → ToolConfigurationError` conserve le code JSON-RPC, la nature du transport et les erreurs de protocole au niveau runtime.

## 5.5 Actions interactives ACP ✅

Les actions interactives sont structurées avec `AgentActionError` :

```text
InvalidInput
Cancelled
Rejected
Failed
```

L'adaptateur ACP traduit explicitement les `FollowUpError` vers ce contrat runtime. Les erreurs d'action ne sont donc plus injectées comme une chaîne nue dans `AgentLoopError::Action`.

## 5.6 Mapping ACP final ✅

Les mappings ACP finaux sont réalisés.

`acp-adaptor/src/prompt/turn.rs` conserve notamment :

```text
AgentLoopError
    ↓
agent_error_kind()
    ↓
AcpError + données machine-readable
```

Les erreurs LLM exposent également `llm_kind`, et les combinaisons agent/persistance conservent les deux diagnostics.

Les terminaisons protocolaires (`Cancelled`, `MaxRounds`) sont séparées des erreurs internes qui doivent rester des erreurs ACP structurées plutôt que devenir silencieusement un `StopReason`.

## 5.7 Audit systématique des `Result<T, String>` ✅ TERMINÉ

L'audit a été réalisé avec la règle suivante :

```text
runtime / métier
    → erreur structurée

provider adapter
    → erreur structurée provider-neutral

ACP / présentation
    → AcpError ou String uniquement lorsque le contrat l'impose
```

Une occurrence textuelle n'a pas été supprimée mécaniquement : elle a été classifiée selon son rôle.

### Résultats

1. Le chemin actif `acp-adaptor/src/prompt/action_typed.rs` utilise désormais :

```rust
Result<Option<String>, AgentActionError>
```

2. L'ancien `acp-adaptor/src/prompt/action.rs`, qui conservait `Result<Option<String>, String>`, était devenu un fichier de code mort : `prompt/mod.rs` redirige explicitement le module `action` vers `action_typed.rs`. Le fichier obsolète a donc été supprimé au lieu de maintenir deux contrats concurrents.

3. `SessionManager::configure_mcp()` reste volontairement en :

```rust
Result<(), String>
```

Cette occurrence est une façade de compatibilité ACP documentée. Le contrat runtime canonique est `configure_mcp_typed()`, qui expose `SessionToolConfigurationError`. La conversion vers `String` est effectuée uniquement au bord du système.

Aucune conversion `Result<T, String>` supplémentaire n'est conservée dans les chemins runtime/provider concernés par ce chantier.

---

## Validation de la branche

Validations confirmées après les corrections :

```text
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

La validation globale `fmt/check` reste à confirmer avant le merge final :

```text
cargo fmt --check                                      ⏳
cargo check --workspace                                ⏳
```

La CI GitHub n'est pas actuellement active sur le dépôt ; la validation complète reste donc manuelle.

---

## Statut final de la dette d'erreurs

```text
✅ session / configuration
✅ persistance / finalisation
✅ provider LLM
✅ outillage / MCP
✅ actions interactives ACP
✅ mappings ACP finaux
✅ audit Result<T, String>
✅ cargo test --workspace
✅ cargo clippy --workspace --all-targets -- -D warnings
⏳ cargo fmt --check
⏳ cargo check --workspace
```

La dette des erreurs structurées est donc **fonctionnellement remboursée**. Le chantier est considéré terminé après confirmation de `fmt` et `check`.

---

# 6. Dette de tests d'intégration — PRIORITÉ 2

Le projet possède une base importante de tests unitaires et de tests de cycle de vie Semantic Events.

Il reste utile de renforcer progressivement les tests de chaîne complète :

```text
provider
→ runtime
→ semantic events
→ projection
→ ACP
```

Ordre cible :

```text
unit tests
integration tests
protocol tests
end-to-end tests
```

**Priorité : P2, prochaine dette après stabilisation finale des erreurs structurées.**

---

# 7. CI — NON PRIORITAIRE / VALIDATION MANUELLE

La CI automatisée est volontairement différée.

Validation minimale actuelle :

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

**Priorité : P3.**

---

# 8. Sandbox shell — DIFFÉRÉ VOLONTAIREMENT

Le sandbox shell utilise encore des heuristiques pour classifier certaines commandes dangereuses.

La direction future privilégiée reste :

```text
commande brute
→ tokenisation / parsing
→ représentation normalisée
→ politique de risque
→ décision d'exécution
```

**Priorité : P3.**

---

# 9. Stratégie de remboursement

```text
✅ 1. Dette de typage sémantique
        ↓
✅ 2. Stabilisation des contrats runtime
        ↓
✅ 3. Dette de persistance
        ↓
✅ 4. Orchestration des turns
        ↓
✅ 5. Erreurs structurées
        ├── ✅ session / configuration
        ├── ✅ persistance / finalisation
        ├── ✅ provider LLM
        ├── ✅ outillage / MCP
        ├── ✅ actions interactives ACP
        ├── ✅ mappings ACP finaux
        └── ✅ audit Result<T, String>
        ↓
6. Tests d'intégration / système renforcés
        ↓
7. Sandbox shell
        ↓
8. CI automatisée
```

Cet ordre peut être révisé lorsqu'un bug concret ou un changement architectural révèle une priorité supérieure.

---

# 10. Règle générale

La dette technique ne doit pas être remboursée uniquement par refactorisation esthétique.

Chaque chantier doit viser au moins l'un des bénéfices suivants :

* réduire une classe d'erreurs impossible à détecter autrement ;
* clarifier un contrat architectural ;
* améliorer la robustesse des états persistés ;
* réduire le couplage entre couches ;
* faciliter les tests et la maintenance future.

Les changements qui n'apportent aucun de ces bénéfices restent optionnels et ne doivent pas passer avant les dettes prioritaires.
