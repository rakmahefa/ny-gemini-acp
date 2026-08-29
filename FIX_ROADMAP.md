# FIX ROADMAP — P0 Integrity

## Objectif

Corriger les vulnérabilités et incohérences P0 identifiées lors de l'audit statique de `main`, sans considérer les invariants comme validés tant que la validation locale complète n'est pas verte.

## Branche

`fix/p0-integrity`

## Règles de validation

Chaque correctif doit :

1. ajouter ou renforcer les tests couvrant le défaut réel ;
2. préserver l'architecture provider-neutral ;
3. passer `cargo fmt --check`, `cargo check --workspace`, `cargo test --workspace` et `cargo clippy --workspace --all-targets -- -D warnings` ;
4. mettre à jour ce document après validation ;
5. ne pas masquer une erreur d'intégrité par un fallback silencieux.

## P0-1 — Identité de session / chemins de persistance

**Problème :** certaines entrées ACP peuvent atteindre `Store` sans validation canonique de `SessionId`, alors que les chemins de persistance sont dérivés directement de l'identifiant.

**État :** ⚠️ À revalider sur la base actuelle de la branche.

## Correctif de compilation Clippy — `llm-provider/web2api`

**Problème :** `json_body()` retournait `Result<Value, Response>`, déclenchant `clippy::result-large-err`.

**Correctif :** erreur boxée via `Box<Response>` et adaptation explicite des handlers.

**État :** ✅ Validé selon la validation locale fournie (`test + clippy` verts).

## P0-2 — Transaction d'intégrité `SemanticEvent` → transport

**Problème :** l'état d'intégrité pouvait être commité avant la publication obligatoire.

**Correctif :** transport obligatoire avant fan-out global et commit de l'état uniquement après succès de publication. `emitter.rs` a été remis en forme lisible pour respecter les règles Clippy, sans changement de logique.

**État :** ⚠️ Implémenté — validation locale complète en attente.

## P0-3 — Cancellation avant `TurnStarted`

**Problème :** une cancellation déjà active pouvait provoquer un `TurnCancelled` rejeté.

**Correctif :** séquence explicite `TurnStarted → TurnCancelled`, sans exécution modèle/outils ; `TurnIntegrity` accepte désormais une terminaison précoce légitime après démarrage du turn.

**Test :** `pre_cancelled_turn_has_started_and_cancelled_semantics`.

**État :** ⚠️ Implémenté — validation locale complète en attente.

## P0-4 — Sandbox shell / exécution réelle

**Problème :** la politique autorisait des capacités dynamiques et l'exécution passait par `sh -c` sans confinement OS.

**Correctif :** fail-closed pour les programmes/capacités dynamiques, build/runtime, commandes mutantes, `xargs` et chemins absolus/traversal ; protection contre des options de type `--git-dir=/etc`.

**Tests :** rejet des programmes dynamiques, chemins directs et options portant des chemins hors périmètre. Les tests de risque distinguent désormais classification et autorisation d'exécution.

**État :** ⚠️ Implémenté — validation locale complète en attente.

**Limite :** le confinement OS complet reste nécessaire pour une vraie isolation de processus/filesystem.

## P0-5 — Scope filesystem / symlink / TOCTOU

**Problème :** la validation de chemin pouvait être contournée via des liens symboliques et la séparation validation/accès reste sensible au TOCTOU.

**Correctif :** inspection `symlink_metadata()` des composants existants, rejet des symlinks dans le chemin et validation renforcée des ancêtres.

**Test :** `existing_symlink_component_is_rejected`.

**État :** ⚠️ Implémenté — validation locale complète en attente.

**Limite :** la garantie TOCTOU atomique nécessite encore une primitive OS appropriée (`openat`/`O_NOFOLLOW` ou équivalent).

## Statut global P0

```text
P0-1 Session identity       ⚠️ à revalider
P0-2 SemanticEvent commit   ⚠️ implémenté / validation en attente
P0-3 Pre-cancel turn        ⚠️ implémenté / validation en attente
P0-4 Shell boundary         ⚠️ implémenté / validation en attente
P0-5 Filesystem scope       ⚠️ implémenté / validation en attente
```

## Roadmaps suivantes

```text
P1_ROADMAP.md  → robustesse runtime après validation P0
P2_ROADMAP.md  → consolidation, qualité et opérabilité après P1
```

Aucun P0 ne doit être déclaré validé avant une exécution locale verte de toute la suite demandée.