# P2 ROADMAP — Consolidation, Quality & Operability

## Statut

**P2 — ✅ TERMINÉE** sur `feat/p2-consolidation-1-4`.

P2 a pour objectif de transformer les garanties introduites en P0/P1 en une base reproductible, observable, documentée et prête pour les prochaines phases. Les audits approfondis peuvent continuer indépendamment sans bloquer la clôture de cette phase.

## P2-1 — CI complète et reproductible ✅

La validation repository est centralisée dans `scripts/validate.sh` :

- `cargo fmt --check`
- `cargo check --workspace`
- `cargo test --workspace --all-targets`
- `cargo clippy --workspace --all-targets -- -D warnings`

La CI GitHub utilise Rust stable sur `ubuntu-latest` avec `rustfmt`, `clippy`, cache Cargo et déclenchement `push` / `pull_request` / manuel.

Validation locale/Codespace : **tests et clippy OK**.

## P2-2 — Test matrix des invariants sémantiques ✅

La matrice couvre :

```text
TurnStarted
AssistantStarted → Delta → Completed
ThinkingStarted → Delta → Completed
ToolRequested → Permission → Execution → Result
TurnCompleted / TurnCancelled / TurnFailed
```

Et couvre également transport absent, ordre invalide, double terminalité, séquence canonique et projection globale.

## P2-3 — Replay / audit déterministe ✅

`SemanticJournal` et `ReplayDiagnostic` fournissent :

- séquences monotones ;
- identité session/turn cohérente ;
- terminalité unique ;
- rejet d'événements post-terminaux ;
- sérialisation JSONL déterministe ;
- relecture validée ;
- diagnostic après incident.

Le journal est disponible comme primitive de replay/audit runtime. L'intégration applicative plus large peut être approfondie ultérieurement sans remettre en cause le contrat établi en P2.

## P2-4 — Observabilité structurée ✅

`EventBus` et les transitions du runtime exposent des logs structurés pour les publications/livraisons, transports absents ou déconnectés, rejets de transitions et identités sémantiques.

Les hooks spécifiques supplémentaires restent de l'amélioration opérationnelle continue.

## P2-5 — Documentation des contrats ✅

`docs/CONTRACTS.md` documente les frontières de sécurité, les limites du confinement OS, la persistance, les tool results, cancellation/failure, ownership des identifiants, ordering/replay et la frontière ACP.

## P2-6 — Property / fuzz testing ✅

Des tests property-like bornés couvrent les invariants des événements, gaps, identité et JSONL.

Le fuzzing spécialisé des parsers et de la persistance est conservé comme piste d'approfondissement qualité, sans bloquer la clôture P2.

## P2-7 — Concurrency stress tests ✅

Les tests couvrent les publications concurrentes, l'intégrité des séquences et l'isolement des transports par turn.

Les campagnes de stress étendues tool/persistence/MCP restent des tests de robustesse continus.

## P2-8 — Dependency hygiene ✅

`docs/DEPENDENCY_POLICY.md` et `scripts/dependency-audit.sh` définissent le contrôle des doublons, versions et features.

**Audit approfondi : à effectuer après clôture P2**, conformément à la décision de projet.

## P2-9 — API ergonomics ✅

Les frontières publiques privilégient `SessionId`, `TurnId` et `ToolCallId`, avec état d'intégrité conservé dans `TurnEventEmitter`. `TurnPhase` est explicitement exposé et les invariants sont documentés dans `docs/API_ERGONOMICS.md`.

## P2-10 — Release readiness ✅

`CHANGELOG.md` et `RELEASE_CHECKLIST.md` couvrent validation, contrats, compatibilité ACP, persistance/migration, diagnostics et rollback.

La release réelle sera traitée au moment où une version cible sera décidée.

## Sortie P2

```text
P2-1  CI reproductible        ✅
P2-2  Semantic test matrix    ✅
P2-3  Replay / audit          ✅
P2-4  Observability           ✅
P2-5  Contract documentation  ✅
P2-6  Property / fuzz base    ✅
P2-7  Concurrency stress base ✅
P2-8  Dependency hygiene     ✅
P2-9  API ergonomics          ✅
P2-10 Release readiness       ✅
```

## Post-P2 — audits et approfondissements

Ces travaux ne bloquent plus la clôture de P2 :

1. audit réel et détaillé des dépendances ;
2. fuzzing spécialisé parser shell / ToolCallId / persistance ;
3. stress tests étendus tool execution / persistence / MCP ;
4. campagnes CI supplémentaires selon les capacités de runner ;
5. définition et préparation d'une release concrète.

## Décision

**P2 est considérée comme terminée.** Les validations de code disponibles sont vertes (`cargo test`, `cargo clippy`). L'audit approfondi sera effectué ensuite sur une base P2 stabilisée.

**Aucun merge dans `main` n'est effectué automatiquement.**
