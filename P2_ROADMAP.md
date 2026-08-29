# P2 ROADMAP — Consolidation, Quality & Operability

## Objectif

Après P0/P1, consolider le projet pour améliorer la maintenabilité, l'observabilité, la reproductibilité et la qualité des contrats sans modifier inutilement l'architecture provider-neutral.

## Avancement P2-1 à P2-10

### P2-1 — CI complète et reproductible

**Implémentation : ✅**

La validation repository est centralisée dans `scripts/validate.sh` et exécute :

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

La CI GitHub utilise un runner `ubuntu-latest`, Rust stable, `rustfmt` + `clippy`, cache Cargo et se déclenche sur `push`, `pull_request` et manuellement.

**Validation : ✅ local / Codespace**

Les validations `test` et `clippy` ont été confirmées correctes sur la branche. La disponibilité du runner GitHub reste indépendante du code et peut varier selon l'environnement Actions.

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

Cas supplémentaires : transport absent, ordre invalide, double terminalité, conservation de la séquence et projection canonique.

### P2-3 — Replay / audit déterministe

**Base implémentée : ✅**

`SemanticJournal` et `ReplayDiagnostic` fournissent :

- séquences strictement monotones à partir de `0` ;
- identité `session_id` / `turn_id` cohérente ;
- terminalité unique ;
- rejet des événements après terminalité ;
- sérialisation JSONL déterministe ;
- relecture JSONL avec validation ;
- diagnostic exploitable après incident.

L'intégration automatique dans le flux runtime et la vérification ACP exhaustive restent un incrément ultérieur.

### P2-4 — Observabilité structurée

**Implémentation initiale : ✅**

`EventBus` émet des logs structurés pour publication, livraison transport, transport absent/déconnecté, lifecycle des subscribers et identité session/turn/tool/sequence.

Les rejets de transitions critiques sont également journalisés.

Les hooks dédiés aux timeouts d'outils et aux erreurs de persistance restent un incrément ultérieur.

### P2-5 — Documentation des contrats

**Implémentation : ✅**

Ajout de `docs/CONTRACTS.md`, couvrant :

- frontières de sécurité applicative ;
- absence de revendication de confinement OS sans primitives dédiées ;
- garanties et limites de persistance ;
- sémantique des tool results ;
- cancellation/failure semantics ;
- ownership des identifiants ;
- ordering et replay ;
- frontière de projection ACP.

### P2-6 — Fuzzing / property testing

**Implémentation partielle : ✅**

Ajout de `crates/agent-runtime/tests/semantic_event_properties.rs` avec des tests property-like bornés portant sur :

- séquences contiguës ;
- détection systématique des gaps ;
- stabilité JSONL aller-retour ;
- cohérence des identités session/turn ;
- distinction transport/journal.

Le fuzzing dédié du parser shell, des tool IDs et de la persistance reste à ajouter ultérieurement.

### P2-7 — Concurrency stress tests

**Implémentation initiale : ✅**

Ajout de `crates/agent-runtime/tests/concurrency_stress.rs` pour :

- publications concurrentes vers un même turn ;
- détection de pertes/doublons ;
- fermeture d'un transport secondaire sans affecter un autre turn.

Les scénarios persistence/tool execution/MCP restent à compléter.

### P2-8 — Dependency hygiene

**Implémentation : ✅**

Ajout de :

- `docs/DEPENDENCY_POLICY.md` ;
- `scripts/dependency-audit.sh`.

Le contrôle couvre doublons de versions et graphe de features. L'audit reste volontairement informatif afin de ne pas transformer toute duplication transitive en échec automatique.

### P2-9 — API ergonomics

**Implémentation : ✅ initiale**

Les frontières publiques privilégient les types forts (`SessionId`, `TurnId`, `ToolCallId`) et `TurnEventEmitter` conserve l'état d'intégrité en interne. `TurnPhase` est exposé explicitement et la documentation `docs/API_ERGONOMICS.md` formalise les invariants, terminalité et transport obligatoire.

### P2-10 — Release readiness

**Implémentation : ✅ documentation initiale**

Ajout de :

- `CHANGELOG.md` ;
- `RELEASE_CHECKLIST.md`.

Le checklist couvre validation, contrats, compatibilité ACP, persistence/migration, diagnostics et rollback. La préparation d'une release réelle dépendra du passage final de la CI et de la version cible.

## Sortie P2

```text
P2-1 CI reproducible       ✅ test/clippy validés localement
P2-2 Semantic test matrix  ✅
P2-3 Replay/audit          ✅ base
P2-4 Observability         ✅ base
P2-5 Contract docs         ✅
P2-6 Property testing      ✅ partiel — fuzzing dédié restant
P2-7 Concurrency tests     ✅ partiel — scénarios runtime restant
P2-8 Dependency hygiene    ✅ politique + audit
P2-9 API ergonomics        ✅ initial
P2-10 Release readiness    ✅ documentation initiale
```

## Prochain incrément P2

Priorités restantes avant clôture complète de P2 :

1. intégrer `SemanticJournal` directement au runtime d'exécution ;
2. ajouter les hooks observabilité timeout/cancellation/persistance ;
3. compléter fuzzing/property testing sur shell, tool IDs et persistance ;
4. étendre les stress tests aux tool execution, persistence et MCP ;
5. effectuer l'audit réel des dépendances et préparer la compatibilité/version de release.

**État : P2-5 à P2-10 entamées avec artefacts versionnés. Aucun merge dans `main`.**
