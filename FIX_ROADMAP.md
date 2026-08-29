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

**Correctif appliqué :** `prompt::run_turn` valide désormais `SessionManager::validate_id()` avant le premier accès à `Store::begin_turn()`. Les formes non conformes sont rejetées avec une erreur ACP `invalid_params`.

**État :** ✅ Corrigé — validation statique acquise

**Validation effectuée :**

- présence d'un test contre `../../escape` ;
- présence d'un test contre `sess_../escape` ;
- conservation d'un test d'ID canonique valide ;
- relecture du chemin `PromptRequest → validate_id → Store::begin_turn` confirmant qu'un ID ACP invalide n'atteint plus la persistance ;
- `cargo` ne peut pas être exécuté dans l'environnement courant faute d'accès réseau/runtime au dépôt, donc la validation complète `fmt/check/test/clippy` reste à faire localement.

## P0-2 — Transaction d'intégrité `SemanticEvent` → transport

**Problème :** l'état d'intégrité est parfois commité avant la publication de l'événement ; une panne de transport peut donc laisser l'émetteur terminal ou avancé alors que l'événement n'a pas été livré.

**Cible :** aucune transition observable ne doit être considérée comme committée lorsque sa publication obligatoire échoue.

**État :** ⏳ À faire

**Validation :** tests d'échec de publication pour chaque transition critique, absence de terminal fantôme, cohérence du séquencement.

## P0-3 — Cancellation avant `TurnStarted`

**Problème :** une cancellation déjà active peut provoquer un `TurnCancelled` rejeté parce que l'intégrité attend d'abord un turn actif.

**Cible :** toute terminaison d'un turn commencé par le service doit avoir une séquence sémantique cohérente ; un chemin pré-cancel doit être explicitement défini.

**État :** ⏳ À faire

**Validation :** tests pré-cancelés, terminalité exactement une fois, absence de turn fantôme.

## P0-4 — Sandbox shell / exécution réelle

**Problème :** la politique autorise des programmes capables d'exécuter du code arbitraire et le shell exécute ensuite `sh -c` ; la politique applicative ne constitue donc pas une frontière de confinement fiable.

**Cible :** réduire immédiatement les capacités incohérentes et établir un contrat explicite fail-closed pour les chemins où la politique ne peut pas garantir la propriété attendue. Le confinement OS complet reste un chantier séparé.

**État :** ⏳ À faire

**Validation :** corpus adversarial élargi et tests montrant qu'aucun programme interdit ne peut être atteint indirectement par la politique courante.

## P0-5 — Scope filesystem / symlink / TOCTOU

**Problème :** la validation de chemin peut être contournée par des liens symboliques et des changements entre validation et accès.

**Cible :** ne pas prétendre fournir une isolation filesystem sans mécanisme permettant une résolution sûre au moment de l'accès ; documenter et fermer les bypass applicatifs immédiats.

**État :** ⏳ À faire

**Validation :** tests symlink, chemins non existants, chemins autorisés additionnels et race-sensitive paths ; contrôle de non-régression.

## Statut global P0

```text
P0-1 Session identity       ✅ corrigé / validation statique
P0-2 SemanticEvent commit   ⏳
P0-3 Pre-cancel turn        ⏳
P0-4 Shell boundary         ⏳
P0-5 Filesystem scope       ⏳
```

Aucun P1/P2 ne doit être traité avant stabilisation de ces cinq invariants, sauf découverte d'une dépendance technique nécessaire à leur correction.
