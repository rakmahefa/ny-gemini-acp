# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document suit le remboursement des dettes techniques qui menacent les contrats d'architecture, la robustesse des turns, la persistance ou la testabilité du runtime.

Les chantiers suivants sont désormais stabilisés dans `main` :

- typage sémantique (`SessionId`, `TurnId`, `ToolCallId`) ;
- persistance et finalisation des turns ;
- orchestration des turns via `agent-runtime::TurnService` ;
- erreurs structurées et mappings ACP ;
- intégrité du cycle `SemanticEvent` et du transport par tour ;
- tests d'intégration/système du runtime et de la projection ACP.

La branche courante `debt/sandbox-shell` rembourse la tranche de dette P3 consacrée au parsing, à la normalisation et à la politique d'exécution du shell.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | ✅ Terminée | Stabilisée et mergée dans `main` |
| P1 | Dette de persistance | ✅ Terminée | Stabilisée et mergée dans `main` |
| P2 | Dette d'orchestration des turns | ✅ Terminée | `TurnService` extrait, runtime découplé d'ACP |
| P2 | Dette des erreurs structurées | ✅ Terminée | Contrats structurés + mappings ACP terminés |
| P2 | Tests d'intégration/système | ✅ Terminée | Runtime, SemanticEvent, projection ACP et chemins adversariaux validés |
| P3 | Sandbox shell — parsing/politique | ✅ Terminée | Parser lexical, normalisation déterministe, allowlist/blocklist et refus des constructions dynamiques intégrés |
| P3 | Sandbox shell — confinement OS | ⏳ À traiter | Le périmètre applicatif reste devant les mécanismes OS de confinement ; aucune exécution n'est considérée isolée du host par cette couche seule |
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

## 6.6 Validation de la branche de tests

Validation locale historiquement confirmée :

```text
cargo fmt --check                                      ✅
cargo test --workspace                                 ✅
cargo clippy --workspace --all-targets -- -D warnings  ✅
```

La CI GitHub n'est pas active sur le dépôt ; la validation de référence reste donc locale et reproductible par ces trois commandes.

---

# 7. Sandbox shell — PRIORITÉ 3 / TRANCHE ACTUELLE ✅

La tranche de dette traitée sur `debt/sandbox-shell` remplace les décisions principalement fondées sur regex par un pipeline explicite :

```text
commande brute
    ↓
lexer shell limité
    ↓
segments + opérateurs
    ↓
normalisation déterministe
    ↓
politique sandbox
    ↓
analyse de risque
    ↓
décision d'exécution
```

## 7.1 Parsing lexical ✅

`crates/tools-provider/src/tools/sandbox/parser.rs` reconnaît explicitement :

- quotes simples et doubles ;
- échappements ;
- arguments séparés ;
- pipelines `|` ;
- opérateurs `;`, `&&`, `||` et `&` ;
- commentaires en début de token ;
- substitutions de commande `$(...)` et backticks ;
- redirections et here-documents.

Le parser ne cherche pas à devenir un interpréteur shell complet. Il doit seulement représenter les constructions nécessaires à la frontière de sécurité.

Les erreurs lexicales deviennent des refus plutôt que des commandes partiellement comprises.

## 7.2 Normalisation ✅

Chaque commande est convertie vers :

```text
ParsedShellCommand
├── segments
│   ├── program
│   └── args
├── operators
└── has_environment_expansion
```

La normalisation est déterministe et retire les différences de quoting qui ne changent pas les arguments sémantiques.

Exemple :

```text
cat 'file name.txt' | grep "foo bar"
```

devient :

```text
cat file name.txt | grep foo bar
```

La représentation normalisée sert ensuite à l'analyse et non l'inverse : les règles de sécurité ne sont plus basées sur un simple `starts_with()` du texte brut.

## 7.3 Politique d'exécution ✅

La sandbox impose maintenant :

- allowlist explicite des programmes connus ;
- blocklist explicite des programmes d'escalade, arrêt système et réseau sortant ;
- interdiction d'exécuter un programme via un chemin explicite ;
- interdiction des affectations d'environnement en tête de commande ;
- interdiction des interpréteurs shell (`sh`, `bash`, `zsh`, etc.) ;
- interdiction du code inline (`python -c`, `node -e`, etc.) ;
- interdiction de `find -exec` / `-execdir` ;
- interdiction des chaînes `xargs` vers un interpréteur ;
- interdiction des cibles absolues ou traversées `../` pour les opérations destructrices (`rm`, `rmdir`, `chmod`, `chown`) ;
- interdiction de la substitution de commande et des expansions d'environnement ;
- interdiction des redirections, here-documents et opérateurs shell non-pipe.

Le seul opérateur composé conservé comme construction de premier niveau est `|`, chaque segment étant ensuite validé indépendamment.

## 7.4 Analyse de risque ✅

`ShellAnalysis` est maintenant construit depuis la représentation parsée :

```text
Low
Medium
High
Critical
```

Le risque distingue notamment les pipelines, commandes à effets de bord, expansions dynamiques et opérations destructrices.

Une commande non analysable devient `Critical` dans l'analyse descriptive et est refusée par la politique d'exécution.

## 7.5 Tests adversariaux ✅

La suite couvre maintenant :

```text
✅ bypass de préfixe (`gitfoo`, `catabc`)
✅ shell interpreters
✅ code inline
✅ curl/wget/nc/socat
✅ pipes vers shell/interpréteur
✅ xargs vers shell
✅ eval / exec
✅ ; / && / || / &
✅ redirections / here-documents
✅ command substitution
✅ environment expansion
✅ chemins absolus
✅ traversal ../
✅ quoting + normalisation
✅ lignes/commentaires
```

Le comportement permissif de test reste disponible, mais il est isolé explicitement et ne correspond pas à la politique par défaut.

## 7.6 Limite restante : confinement OS ⏳

Cette tranche ne prétend pas fournir un sandbox kernel/container.

Le chemin d'exécution ACP reste conceptuellement :

```text
policy decision
    ↓
ACP terminal
    ↓
sh -c <commande validée>
```

La validation actuelle réduit les classes de commandes dangereuses mais ne constitue pas une frontière de confidentialité, de privilège ou d'isolation du host.

La prochaine tranche de cette dette devra donc décider explicitement si l'exécution doit être confinée par mécanisme OS/container, avec un contrat de fallback sécurisé lorsque le confinement n'est pas disponible.

---

# 8. CI — PRIORITÉ 3 / DIFFÉRÉE

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
✅ 6. Sandbox shell — parsing / normalisation / politique
        ↓
⏳ 7. Sandbox shell — confinement OS
        ↓
8. CI automatisée
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
