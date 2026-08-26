# DETTE TECHNIQUE — ny-gemini-acp

## Statut

Ce document recense la dette technique identifiée sur la branche `chore/deep-audit-cleanup` et définit l'ordre de remboursement retenu pour la suite du projet.

L'objectif n'est pas de supprimer toute dette immédiatement, mais de traiter en priorité les dettes qui risquent de rendre les contrats internes plus difficiles à stabiliser ou qui augmentent fortement le coût des évolutions futures.

---

## 1. Priorités actuelles

| Priorité | Dette | Statut | Décision |
|---|---|---|---|
| P1 | Dette de typage sémantique | À traiter | Priorité actuelle |
| P1 | Dette de persistance | À traiter ensuite | Deuxième chantier |
| P2 | Dette d'orchestration des turns | À surveiller | Après typage/persistance |
| P2 | Dette des erreurs structurées | À traiter progressivement | Après stabilisation des contrats |
| P2 | Dette de tests d'intégration/système | À renforcer | Validation manuelle conservée |
| P3 | Sandbox shell | Différé volontairement | À traiter plus tard |
| P3 | CI automatisée | Hors priorité | Tests CI manuels par le mainteneur |

---

# 2. Dette de typage sémantique — PRIORITÉ 1

## Constat

Les contrats centraux du runtime utilisent encore des primitives générales, notamment `String`, `Vec<String>`, `serde_json::Value` et parfois `Result<_, String>`.

Cela concerne particulièrement :

- identifiants de session ;
- identifiants de turn ;
- identifiants d'appel d'outil ;
- noms d'outils ;
- identifiants de modèle ;
- noms de serveurs ;
- certaines erreurs de contrat.

Le problème n'est pas l'utilisation de `String` pour du texte libre. Le problème est l'utilisation d'un même type pour plusieurs concepts métier différents.

## Risque

Cette situation permet au compilateur d'accepter des mélanges sémantiques qui devraient être impossibles :

```text
SessionId
TurnId
ToolCallId
ToolName
ModelId
ServerName
```

sont actuellement trop souvent représentés par le même type primitif.

Le risque augmente maintenant que les Semantic Events et la machine d'état d'intégrité sont devenus plus stricts.

## Direction retenue

Introduire progressivement des types forts pour les identités réellement sémantiques :

```rust
SessionId
TurnId
ToolCallId
ToolName
ModelId
ServerName
```

Ne pas convertir artificiellement tous les `String` en newtypes. Les prompts, contenus, descriptions et messages libres doivent rester des chaînes ordinaires.

## Objectif

Obtenir des contrats internes dans lesquels le compilateur aide à empêcher les erreurs d'identité et de routage.

## Priorité

**P1 — chantier immédiat.**

---

# 3. Dette de persistance — PRIORITÉ 1 (après typage)

## Constat

Le `Store` combine actuellement :

- état live en mémoire ;
- verrous de concurrence ;
- persistance sur disque ;
- snapshots ;
- génération de turn ;
- état busy ;
- fermeture/libération des sessions.

Le fonctionnement actuel est cohérent pour l'usage présent, mais plusieurs opérations sont composées de plusieurs écritures et mises à jour successives.

## Risque

En cas d'arrêt brutal, erreur disque ou interruption pendant une opération, plusieurs représentations d'un même état peuvent théoriquement diverger :

```text
état mémoire
snapshot
session persistée
busy state
[génération]
```

La génération des turns protège déjà contre certains tours obsolètes, mais elle ne constitue pas à elle seule une transaction globale.

## Direction retenue

Après le chantier de typage, définir explicitement un modèle de persistance robuste, incluant notamment :

- atomicité des mises à jour importantes ;
- stratégie de récupération après crash ;
- cohérence entre état live et disque ;
- stratégie claire pour les snapshots ;
- comportement lors d'une interruption au milieu d'un turn ;
- tests de reprise et de corruption partielle.

Une solution de type transaction logique, écriture atomique ou journalisation pourra être retenue après analyse du modèle réel d'utilisation.

## Priorité

**P1 — deuxième chantier, après la dette de typage.**

---

# 4. Dette d'orchestration des turns — PRIORITÉ 2

## Constat

Le `acp-adaptor` possède déjà des handlers séparés, mais le câblage du traitement d'un prompt reste concentré dans la composition ACP : création du turn, abonnement aux Semantic Events, projection ACP, contexte interactif, exécution, terminaison et conversion d'erreurs.

## Risque

À mesure que les fonctionnalités augmentent, la composition ACP peut devenir trop fortement couplée au workflow interne d'un turn.

## Direction retenue

Faire évoluer progressivement cette orchestration vers un service de turn clairement identifiable, capable de centraliser :

```text
création du turn
→ lifecycle sémantique
→ exécution provider
→ projection
→ terminalisation
```

## Priorité

**P2 — après typage et persistance.**

---

# 5. Dette de gestion des erreurs — PRIORITÉ 2

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

**P2 — après stabilisation des principaux contrats.**

---

# 6. Dette de tests d'intégration — PRIORITÉ 2

## Constat

Le projet dispose désormais d'une base importante de tests unitaires, notamment pour la machine d'état Semantic Events et le cycle de vie des outils.

Cependant, les tests de bout en bout de la chaîne complète restent plus importants à mesure que le système devient distribué entre plusieurs crates et couches :

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

---

# 7. CI — NON PRIORITAIRE / VALIDATION MANUELLE

## Décision du projet

La CI automatisée n'est **pas considérée comme une dette prioritaire à rembourser actuellement**.

Les tests et vérifications du projet sont volontairement exécutés manuellement par le mainteneur.

Cela signifie que l'absence d'une CI active ne doit pas être interprétée comme un chantier bloquant pour les prochaines étapes d'architecture.

## Validation actuelle

La validation doit continuer à couvrir au minimum les vérifications pertinentes du workspace :

```text
cargo fmt --check
cargo check --workspace
cargo test --workspace
```

ainsi que les audits spécifiques du dépôt lorsque nécessaire.

## Futur

Une CI automatisée pourra être réintroduite ultérieurement lorsque les contraintes de ressources et le rythme du projet le justifieront.

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

L'ordre de travail retenu est :

```text
1. Dette de typage sémantique
        ↓
2. Stabilisation des contrats runtime
        ↓
3. Dette de persistance
        ↓
4. Orchestration des turns
        ↓
5. Erreurs structurées
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
