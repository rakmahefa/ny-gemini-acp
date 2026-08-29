# FIX ROADMAP — P0 Integrity

## Objectif

Corriger les vulnérabilités et incohérences P0 identifiées lors de l'audit statique de `main`, sans élargir le périmètre aux dettes P1/P2 tant que les invariants P0 ne sont pas validés.

## Branche

`fix/p0-integrity`

## Règles de validation

Chaque correctif doit :

1. ajouter ou renforcer les tests couvrant le défaut réel ;
2. préserver l'architecture provider-neutral ;
3. passer au minimum `cargo fmt --check`, `cargo check --workspace`, `cargo test --workspace` et `cargo clippy --workspace --all-targets -- -D warnings` lorsque l'environnement d'exécution local est disponible ;
4. mettre à jour ce document immédiatement après validation ;
5. ne pas masquer une erreur d'intégrité par un fallback silencieux.

## P0-1 — Identité de session / chemins de persistance

**Problème :** certaines entrées ACP peuvent atteindre `Store` sans validation canonique de `SessionId`, alors que les chemins de persistance sont dérivés directement de l'identifiant.

**État :** ⚠️ À revalider après le reset de la branche ; la protection précédemment implémentée n'est pas considérée comme présente tant que le code et les tests n'ont pas été revérifiés sur cette base.

## Correctif de compilation Clippy — `llm-provider/web2api`

**Problème :** `json_body()` retournait `Result<Value, Response>`, déclenchant `clippy::result-large-err` avec `-D warnings`.

**Correctif appliqué :** l'erreur est maintenant `Box<Response>`, avec adaptation explicite des handlers Google et Responses via `return *e`.

**Commits :** `e458f13`, `9b4a569`, `cd248d4`.

**État :** ✅ Corrigé selon la validation locale fournie (`test + clippy` verts).

## P0-2 — Transaction d'intégrité `SemanticEvent` → transport

**Problème :** l'état d'intégrité était commité avant la publication obligatoire ; une panne de transport pouvait laisser un état terminal/avancé sans événement correspondant.

**Correctif appliqué :** le transport obligatoire est tenté avant le fan-out global et l'état modifié par les transitions est préparé avant commit dans `TurnEventEmitter`.

**État :** ⚠️ Implémenté — validation locale globale en attente.

**Note :** `emitter.rs` provient d'une première réécriture fonctionnelle (`5cf1819`) dont le diff est trop large ; cette forme doit être reformatée et resserrée avant merge même si les tests passent.

## P0-3 — Cancellation avant `TurnStarted`

**Problème :** une cancellation déjà active pouvait provoquer un `TurnCancelled` rejeté parce que l'intégrité attend d'abord un turn actif.

**Correctif appliqué :** `TurnService` traite explicitement le chemin pré-cancel : `TurnStarted → TurnCancelled`, finalisation de la session, puis retour `Cancelled`, sans entrer dans la boucle modèle/outils.

**Test ajouté :** `pre_cancelled_turn_has_started_and_cancelled_semantics`.

**État :** ⚠️ Implémenté — validation locale globale en attente.

## P0-4 — Sandbox shell / exécution réelle

**Problème :** la politique autorisait des programmes capables d'exécuter du code arbitraire alors que le shell utilisait `sh -c` sans confinement OS.

**Correctif appliqué :** fail-closed pour les capacités dynamiques/build/runtime (`python`, `node`, `awk`, `cargo`, compilateurs, package managers, containers, etc.), interdiction des commandes mutantes sans confinement OS, interdiction des chemins absolus/traversal dans les arguments shell et extension du blocage `xargs`/méta-commandes.

**Tests ajoutés :** rejet des programmes dynamiques et des chemins directs hors périmètre.

**État :** ⚠️ Implémenté — validation locale globale en attente.

**Limite explicitement conservée :** le confinement OS complet reste nécessaire pour offrir une vraie sandbox de processus/filesystem.

## P0-5 — Scope filesystem / symlink / TOCTOU

**Problème :** la validation de chemin pouvait suivre des liens symboliques et déclarer sûr un chemin dont la résolution pouvait sortir du périmètre.

**Correctif appliqué :** inspection `symlink_metadata()` de chaque composant existant, rejet explicite de tout lien symbolique dans le chemin, et refus si un composant existant ne peut pas être inspecté. Les chemins non existants restent supportés uniquement après validation de leurs ancêtres existants.

**Test ajouté :** `existing_symlink_component_is_rejected`.

**État :** ⚠️ Implémenté — validation locale globale en attente.

**Limite explicitement conservée :** ceci ferme le bypass symlink immédiat, mais ne constitue pas à lui seul une garantie TOCTOU atomique ; un mécanisme OS de type `openat`/`O_NOFOLLOW`/équivalent reste la cible définitive.

## Statut global P0

```text
P0-1 Session identity       ⚠️ à revalider
P0-2 SemanticEvent commit   ⚠️ implémenté / validation en attente
P0-3 Pre-cancel turn        ⚠️ implémenté / validation en attente
P0-4 Shell boundary         ⚠️ implémenté / validation en attente
P0-5 Filesystem scope       ⚠️ implémenté / validation en attente
```

Le lot P0-2 → P0-5 est préparé sur `fix/p0-integrity`. Aucun P1/P2 ne doit être traité avant validation locale complète de ce lot.
