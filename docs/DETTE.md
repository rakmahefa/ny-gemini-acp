
# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document recense la dette technique identifiée sur la branche `chore/deep-audit-cleanup` et définit l'ordre de remboursement retenu pour la suite du projet.

Les chantiers P1 de typage sémantique et de persistance ont été réalisés sur la branche `debt/semantic-typing`, validés localement avec `cargo fmt --check`, `cargo check --workspace` et `cargo test --workspace`, puis mergés dans `main`.

L'objectif reste de traiter en priorité les dettes qui risquent de rendre les contrats internes plus difficiles à stabiliser ou qui augmentent fortement le coût des évolutions futures.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | TurnService extrait, adaptateur découplé, tests et validations verts |
| P2 | Dette des erreurs structurées | 🚧 En cours | Tranches session/persistance/provider traitées sur `debt/structured-errors` |
| P2 | Dette de tests d'intégration/système | À renforcer | Validation manuelle conservée |
| P3 | Sandbox shell | Différé volontairement | À traiter plus tard |
| P3 | CI automatisée | Hors priorité | Tests CI manuels par le mainteneur |

---

# 2. Dette de typage sémantique — PRIORITÉ 1 ✅ TERMINÉE

## Réalisé

Les identités sémantiques centrales disposent maintenant de types forts :

```rust
SessionId
TurnId
ToolCallId
```
Ils sont intégrés aux contrats du runtime et aux `SemanticEvent`, notamment `EventContext` et `ToolEventContext`.

Les frontières ACP convertissent explicitement ces types vers leurs représentations protocolaires lorsque nécessaire, sans réintroduire les primitives dans le cœur du runtime.

La représentation sérialisée des identités reste compatible avec les valeurs historiques grâce à des wrappers transparents `serde`.

## Validation

```text
cargo fmt --check   ✅
cargo check --workspace   ✅
cargo test --workspace    ✅
cargo clippy --workspace --all-targets -- -D warnings   ✅
```

## Limite volontaire

Les types `ToolName`, `ModelId` et `ServerName` restent à envisager ultérieurement si leur introduction apporte une protection réelle sans sur-typer les contrats.

## Statut

**✅ Terminé et mergé dans `main`.**

---

# 3. Dette de persistance — PRIORITÉ 1 ✅ TERMINÉE

## Réalisé

Le modèle de persistance a été renforcé autour des points suivants :

* écritures session atomiques ;
* écritures snapshot atomiques ;
* `sync_all()` avant remplacement du fichier final ;
* nettoyage des fichiers temporaires orphelins au démarrage ;
* cohérence du cache live après une persistance réussie ;
* récupération des sentinelles `.busy` orphelines lors du redémarrage ;
* conservation du contrôle de génération pour éviter les tours obsolètes ;
* tests couvrant les écritures atomiques et les scénarios de récupération concernés.

Le cache live n'est plus considéré comme modifié avec succès tant que la persistance correspondante n'a pas elle-même réussi.

## Modèle retenu

La solution actuelle reste volontairement simple :

```text
état runtime
   ↓
persistance atomique
   ↓
fichier session / snapshot
```

Une journalisation complète ou une transaction multi-fichiers pourra être étudiée ultérieurement seulement si le modèle d'utilisation réel la justifie.

## Validation

```text
cargo fmt --check   ✅
cargo check --workspace   ✅
cargo test --workspace    ✅
```

## Statut

**✅ Terminé et mergé dans `main`.**

---

# 4. Dette d'orchestration des turns — PRIORITÉ 2 ✅ TERMINÉE

## Constat initial

Le `acp-adaptor` possédait un câblage important autour du traitement d'un prompt : prise de possession du turn, création du transport sémantique, projection ACP, contexte interactif et remise de la réponse.

## Remboursement réalisé sur `debt/turn-orchestration`

Le chantier a été traité en conservant une frontière nette entre runtime et protocole :

* création de `agent_runtime::TurnService` ;
* `TurnService` intégré à la composition root `AppState` ;
* exécution provider-neutral déplacée de l'adaptateur vers `agent-runtime` ;
* création et configuration de `AgentLoop` centralisées dans le service ;
* terminalisation `SemanticEvent` regroupée dans le service ;
* finalisation/persistance `Store::end_turn` regroupée dans le service ;
* conservation des `AgentActionHandler` et `ToolPermissionHandler` comme dépendances injectées ;
* suppression de l'ancien `TurnGuard` devenu redondant ;
* extraction de l'orchestration ACP restante vers `prompt::handle_prompt` ;
* `acp-adaptor/src/agent.rs` réduit au routage des requêtes/notifications et au délégateur de prompt ;
* création du `turn_id` et du transport sémantique différée jusqu'à l'acceptation effective du turn ;
* introduction de `TurnExecutionRequest` et d'un contrat explicite pour le prompt builder, sans contourner Clippy ;
* ajout d'un test runtime couvrant `provider → AgentLoop → SemanticEvent lifecycle → Store persistence`.

## Découpage final

```text
ACP request
    ↓
acp-adaptor
    ├── routage ACP
    └── prompt::handle_prompt
            ├── turn ownership
            ├── projection ACP
            ├── interaction handlers
            └── ACP response
                    ↓
             agent-runtime::TurnService
                    ├── AgentLoop
                    ├── SemanticEvent lifecycle
                    ├── provider/tool execution
                    └── Store::end_turn
```

Le runtime reste indépendant d'ACP. `TurnService` ne connaît ni `PromptRequest`, ni `ConnectionTo<Client>`, ni les types de présentation ACP.

## Garanties de test

La couverture est organisée autour de deux barrières complémentaires :

```text
provider
   ↓
TurnService
   ↓
SemanticEvent lifecycle
   ↓
Store persistence
```

et :

```text
SemanticEvent
   ↓
sequence / turn integrity
   ↓
ACP projection
```

Les tests de projection vérifient notamment les ruptures de séquence, les changements de turn inattendus et la conservation de l'identité des tool calls dans le modèle de présentation.

## Validation finale

Validation locale confirmée :

```text
cargo fmt --check   ✅
cargo check --workspace   ✅
cargo test --workspace    ✅
cargo clippy --workspace --all-targets -- -D warnings   ✅
```

## Statut

**✅ Terminé sur `debt/turn-orchestration`.**

La branche peut maintenant être revue puis mergée dans `main`. La dette prioritaire suivante est celle des erreurs structurées.

---

# 5. Dette de gestion des erreurs — PRIORITÉ 2 🚧 EN COURS

## Constat

Plusieurs modèles d'erreur coexistent encore :

* `anyhow` pour certaines couches d'application ;
* `thiserror` pour les contrats métier ;
* quelques erreurs historiques sous forme de `String` ;
* erreurs ACP aux frontières protocolaires ;
* erreurs provider spécifiques.

L'objectif est de préserver des erreurs structurées dans le cœur du runtime et de ne transformer ces erreurs en représentations textuelles qu'aux frontières qui l'exigent réellement.

---

## 5.1 Première tranche — session / configuration ✅

Le contrat runtime des outils n'utilise plus `Result<(), String>` pour la configuration de session.

Réalisé :

* introduction de `ToolConfigurationError` dans `agent-runtime` ;
* propagation de cette erreur structurée vers `DefaultToolProvider` ;
* introduction de `SessionToolConfigurationError` pour distinguer l'absence de session d'un échec provider ;
* export explicite des nouveaux contrats d'erreur par `agent-runtime` ;
* séparation entre `configure_mcp_typed()` et la compatibilité `configure_mcp()` de frontière ACP ;
* test de régression couvrant une configuration MCP sur une session inexistante.

Contrat :

```text
Tool configuration
    ↓
ToolConfigurationError
    ↓
SessionToolConfigurationError
    ↓
frontière applicative / ACP
```

---

## 5.2 Deuxième tranche — persistance / finalisation ✅

La finalisation d'un tour ne masque plus certaines erreurs de persistance.

Réalisé :

* introduction de `StoreError` pour la finalisation de tour ;
* erreur explicite de génération obsolète ;
* erreur explicite de persistance ;
* propagation de l'échec de finalisation par `TurnService` ;
* conservation de l'erreur d'exécution lorsqu'une erreur de persistance survient simultanément grâce à `AgentAndPersistence` ;
* adaptation des mappings ACP aux nouveaux cas de `TurnServiceError`.

Contrat :

```text
AgentLoop
    ↓
TurnService
    ↓
Store::end_turn
    ↓
StoreError
```

---

## 5.3 Troisième tranche — erreurs provider LLM ✅

Le contrat provider-neutre du runtime distingue maintenant plusieurs familles d'erreurs.

Réalisé :

```rust
LlmProviderErrorKind
```

avec notamment :

```text
Authentication
InvalidRequest
ModelUnavailable
Network
Upstream
StreamDivergence
Upload
```

Le `LlmError` runtime porte désormais ces catégories explicitement.

Le provider Gemini mappe ses `GeminiError` vers le contrat runtime :

```text
GeminiError
    ↓
LlmError
    ↓
LlmProviderErrorKind
```

Mappings actuellement préservés :

```text
CookiesExpired   → Authentication
UnknownModel     → ModelUnavailable
Network          → Network
Http             → Upstream
StreamDivergence → StreamDivergence
UploadFailed     → Upload
SafetyBlocked    → Upstream
Other            → Upstream
```

Le provider ne transforme donc plus systématiquement les erreurs Gemini en un simple `Provider(String)`.

Les erreurs `anyhow::Error` provenant encore des frontières historiques du client sont converties explicitement, avec préservation de `GeminiError` lorsqu'il est disponible et fallback `Upstream` sinon.

## Validation

Validation locale confirmée pour cette tranche :

```text
cargo fmt --check                                      ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

---

## Limites restantes

La dette des erreurs structurées n'est pas encore totalement soldée.

Les prochaines zones à traiter sont notamment :

* erreurs d'outillage/MCP encore aplaties en chaînes ;
* erreurs de session et de persistance hors du chemin de finalisation d'un turn ;
* mappings ACP finaux et tests associés ;
* vérification systématique des `Result<T, String>` restants ;
* contrats d'actions interactives ACP encore exprimés en `String` ;
* différenciation plus fine des erreurs MCP `Config`, `Transport`, `Protocol` et `Remote`.

Le cœur runtime doit rester indépendant d'ACP.

Les conversions vers les erreurs protocolaires restent du ressort des adaptateurs.

---

## Validation globale de la branche

Les validations locales confirmées jusqu'à présent :

```text
cargo fmt --check                                      ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

La CI GitHub n'est pas actuellement active sur le dépôt ; la validation complète reste donc exécutée manuellement par le mainteneur.

---

## Statut

**🚧 En cours — trois tranches structurées réalisées :**

```text
✅ session / configuration
✅ persistance / finalisation
✅ provider LLM
🚧 outillage / MCP
🚧 mappings ACP finaux
🚧 audit systématique des Result<T, String>
```

---

# 6. Dette de tests d'intégration — PRIORITÉ 2

## Constat

Le projet dispose désormais d'une base importante de tests unitaires, notamment pour la machine d'état Semantic Events et le cycle de vie des outils.

Cependant, des tests de bout en bout de la chaîne complète restent utiles à mesure que le système devient distribué entre plusieurs crates et couches :

```text
provider
→ runtime
→ semantic events
→ projection
→ ACP
```

## Direction retenue

Renforcer progressivement la pyramide de validation :

```text
unit tests
integration tests
protocol tests
end-to-end tests
```

La validation locale et manuelle reste la méthode de validation principale du projet pour le moment.

## Priorité

**P2 — à renforcer après la dette des erreurs structurées.**

---

# 7. CI — NON PRIORITAIRE / VALIDATION MANUELLE

## Décision du projet

La CI automatisée n'est **pas considérée comme une dette prioritaire à rembourser actuellement**.

Les tests et vérifications du projet sont volontairement exécutés manuellement par le mainteneur.

## Validation actuelle

La validation doit continuer à couvrir au minimum :

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

ainsi que les audits spécifiques du dépôt lorsque nécessaire.

## Priorité

**P3 — volontairement différée.**

---

# 8. Sandbox shell — DIFFÉRÉ VOLONTAIREMENT

## Constat

Le sandbox shell actuel utilise encore des heuristiques pour classifier certaines commandes et chaînes dangereuses.

Cette implémentation devra être renforcée à terme, notamment pour une analyse plus correcte de la syntaxe shell.

## Décision du projet

Le sandbox shell **n'est pas un chantier prioritaire dans la phase actuelle**.

Il doit être conservé, testé et surveillé, mais son refactor de fond est explicitement reporté afin de ne pas disperser l'effort de stabilisation du runtime.

## Futur

Lorsque ce chantier sera ouvert, la direction privilégiée sera :

```text
commande brute
→ tokenisation / parsing
→ représentation normalisée
→ politique de risque
→ décision d'exécution
```

plutôt qu'une dépendance excessive à des recherches textuelles simples.

## Priorité

**P3 — à traiter plus tard.**

---

# 9. Stratégie de remboursement

L'ordre de travail retenu est désormais :

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
        ├── 🚧 outillage / MCP
        └── 🚧 mappings ACP finaux
        ↓
6. Tests d'intégration / système renforcés
        ↓
7. Sandbox shell
        ↓
8. CI automatisée
```

Cet ordre peut être révisé si un bug concret ou un changement architectural révèle une priorité supérieure.

---

# 10. Règle générale

La dette technique ne doit pas être remboursée uniquement par refactorisation esthétique.

Chaque chantier doit viser au moins l'un des bénéfices suivants :

* réduire une classe d'erreurs impossible à détecter autrement ;
* clarifier un contrat architectural ;
* améliorer la robustesse des états persistés ;
* réduire le couplage entre couches ;
* faciliter les tests et la maintenance future.

Les changements qui n'apportent aucun de ces bénéfices doivent être considérés comme optionnels et ne doivent pas être prioritaires face aux dettes listées ci-dessus.

---

# 11. Point de reprise

La branche `debt/structured-errors` constitue actuellement le point de reprise de la dette des erreurs structurées.

Les prochaines étapes sont :

```text
outillage / MCP
    ↓
erreurs structurées MCP
    ↓
mappings ACP
    ↓
audit Result<T, String>
    ↓
tests d'intégration
```

Principe directeur :

> Les erreurs doivent rester structurées aussi longtemps que possible dans le runtime.
> La conversion en texte ne doit intervenir qu'à une frontière de présentation ou de protocole qui l'exige explicitement.

```

Source actuelle : `docs/DETTE.md` sur `debt/structured-errors`. 
```
