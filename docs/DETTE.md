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
| P2 | Dette des erreurs structurées | 🚧 Prochaine priorité | À traiter progressivement après orchestration |
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

- écritures session atomiques ;
- écritures snapshot atomiques ;
- `sync_all()` avant remplacement du fichier final ;
- nettoyage des fichiers temporaires orphelins au démarrage ;
- cohérence du cache live après une persistance réussie ;
- récupération des sentinelles `.busy` orphelines lors du redémarrage ;
- conservation du contrôle de génération pour éviter les tours obsolètes ;
- tests couvrant les écritures atomiques et les scénarios de récupération concernés.

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

- création de `agent_runtime::TurnService` ;
- `TurnService` intégré à la composition root `AppState` ;
- exécution provider-neutral déplacée de l'adaptateur vers `agent-runtime` ;
- création et configuration de `AgentLoop` centralisées dans le service ;
- terminalisation `SemanticEvent` regroupée dans le service ;
- finalisation/persistance `Store::end_turn` regroupée dans le service ;
- conservation des `AgentActionHandler` et `ToolPermissionHandler` comme dépendances injectées ;
- suppression de l'ancien `TurnGuard` devenu redondant ;
- extraction de l'orchestration ACP restante vers `prompt::handle_prompt` ;
- `acp-adaptor/src/agent.rs` réduit au routage des requêtes/notifications et au délégateur de prompt ;
- création du `turn_id` et du transport sémantique différée jusqu'à l'acceptation effective du turn ;
- introduction de `TurnExecutionRequest` et d'un contrat explicite pour le prompt builder, sans contourner Clippy ;
- ajout d'un test runtime couvrant `provider → AgentLoop → SemanticEvent lifecycle → Store persistence`.

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

La branche peut maintenant être revue puis mergée dans `main`. La prochaine dette prioritaire est celle des erreurs structurées.

---

# 5. Dette de gestion des erreurs — PRIORITÉ 2 🚧 PROCHAINE PRIORITÉ

## Constat

Plusieurs modèles d'erreur coexistent :

- `anyhow` pour certaines couches d'application ;
- `thiserror` pour certains contrats ;
- `Result<T, String>` pour certaines interfaces ;
- erreurs ACP aux frontières protocolaire ;
- erreurs provider spécifiques.

## Risque

Les `String` utilisés comme erreurs perdent de l'information structurée et rendent plus difficiles le traitement programmatique, les tests précis et le mapping vers les couches supérieures.

## Direction retenue

Introduire progressivement des erreurs métier typées, notamment autour de :

```text
Session
Persistence
Tool configuration
Tool execution
Provider
Turn lifecycle
```

Puis convertir explicitement ces erreurs aux frontières applicatives et ACP.

## Priorité

**P2 — prochaine dette à rembourser.**

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

La validation doit continuer à couvrir au minimum les vérifications pertinentes du workspace :

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

- réduire une classe d'erreurs impossible à détecter autrement ;
- clarifier un contrat architectural ;
- améliorer la robustesse des états persistés ;
- réduire le couplage entre couches ;
- faciliter les tests et la maintenance future.

Les changements qui n'apportent aucun de ces bénéfices doivent être considérés comme optionnels et ne doivent pas être prioritaires face aux dettes listées ci-dessus.

---

# 11. Point de reprise

La prochaine étape est la dette des erreurs structurées. La branche `debt/turn-orchestration` peut être revue puis mergée dans `main` avant d'ouvrir le prochain chantier.
