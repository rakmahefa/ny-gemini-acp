# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document recense la dette technique identifiée sur la branche `chore/deep-audit-cleanup` et l'ordre de remboursement retenu.

Les chantiers de typage sémantique, persistance et orchestration des turns sont stabilisés et présents dans `main`. La branche `debt/structured-errors` poursuit le remboursement de la dette de gestion des erreurs en conservant des contrats structurés dans le runtime et les conversions textuelles uniquement aux frontières qui l'exigent.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | `TurnService` extrait, runtime découplé d'ACP |
| P2 | Dette des erreurs structurées | 🚧 En cours | Tranches runtime traitées ; audit final des conversions en cours |
| P2 | Dette de tests d'intégration/système | À renforcer | Après stabilisation des erreurs structurées |
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

# 5. Dette de gestion des erreurs — PRIORITÉ 2 🚧 EN COURS

## Objectif

Les erreurs métier/runtime doivent rester structurées jusqu'à la frontière applicative. Les conversions en `String` ou `AcpError` sont réservées aux façades protocolaires qui les nécessitent réellement.

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

`configure_mcp_typed()` conserve l'erreur structurée. `configure_mcp()` reste une façade de compatibilité qui réalise volontairement la conversion textuelle au bord du système.

## 5.2 Persistance / finalisation ✅

La finalisation d'un tour distingue désormais explicitement :

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

Les actions interactives sont maintenant structurées avec `AgentActionError` :

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

## 5.7 Audit systématique des `Result<T, String>` 🚧 EN COURS

L'audit final est en cours pour vérifier que chaque conversion textuelle est légitime et située à une frontière explicite.

Règles retenues :

```text
runtime / métier
    → erreur structurée

provider adapter
    → erreur structurée provider-neutral

ACP / présentation
    → AcpError ou String uniquement lorsque le contrat l'impose
```

Une occurrence textuelle n'est donc pas supprimée mécaniquement : elle doit être classifiée comme contrat runtime, erreur provider, compatibilité ou présentation.

### Progression actuelle

Le premier échec découvert pendant `cargo test --workspace` provenait d'un ancien test qui construisait :

```rust
AgentLoopError::Action("boom".into())
```

Le contrat est maintenant correctement consommé avec :

```rust
AgentLoopError::Action(AgentActionError::Failed("boom".into()))
```

Le correctif est poussé sur `debt/structured-errors`.

La recherche reste à poursuivre sur les autres chemins d'erreur avant de déclarer l'audit terminé.

---

## Validation de la branche

La dernière validation locale fournie avant le correctif a échoué pendant la compilation des tests de `acp-adaptor` sur ce cas typé.

Après le correctif :

```text
cargo fmt --check                                      ⏳
cargo check --workspace                                ⏳
cargo test --workspace                                 ⏳
cargo clippy --workspace --all-targets -- -D warnings  ⏳
```

Ces quatre validations doivent être rejouées avant de considérer la branche verte.

La CI GitHub n'est pas actuellement active sur le dépôt ; la validation complète reste donc manuelle.

---

## Limites restantes de la dette d'erreurs

```text
✅ session / configuration
✅ persistance / finalisation
✅ provider LLM
✅ outillage / MCP
✅ actions interactives ACP
✅ mappings ACP finaux
🚧 audit systématique des Result<T, String>
🚧 validation globale fmt / check / test / clippy
```

Une différenciation encore plus fine du contenu des erreurs LLM/MCP ne sera introduite que si elle apporte une valeur réelle pour le protocole, les diagnostics ou l'observabilité.

Le cœur runtime doit rester indépendant d'ACP.

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

**Priorité : P2, après stabilisation complète des erreurs structurées.**

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
🚧 5. Erreurs structurées
        ├── ✅ session / configuration
        ├── ✅ persistance / finalisation
        ├── ✅ provider LLM
        ├── ✅ outillage / MCP
        ├── ✅ actions interactives ACP
        ├── ✅ mappings ACP finaux
        └── 🚧 audit Result<T, String> + validation globale
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
