# Phase 1 — Prompt Test Matrix

## Purpose

Phase 1 validates semantic lifecycle integrity, especially the real failure observed during multi-round tool execution: a tool can reach an invalid `Terminal` state before permission, execution, or result events arrive.

Use these prompts in real Zed after each lifecycle-related implementation change. Record ACP evidence in `ACP_LOG.md`.

## P1-001 — One tool, one round

```text
Liste les fichiers du workspace, puis résume ce que tu as trouvé.
```

Expected lifecycle:

```text
TurnStarted
→ AssistantStarted/Delta*
→ ToolCall
→ ToolExecution/Result
→ Assistant continuation
→ TurnCompleted
```

No lifecycle violation should be logged.

## P1-002 — Two sequential tools

```text
Liste les fichiers du workspace, puis lis `README.md` si le fichier existe.
```

Expected: two distinct tool calls with distinct IDs and valid transitions for each.

## P1-003 — Multi-round tool loop

```text
Analyse le workspace. Utilise plusieurs outils si nécessaire, puis donne un résumé final.
```

Expected: round 0 and later rounds remain part of one coherent turn; no tool is considered terminal before its result.

## P1-004 — Permission then execution

```text
Exécute `ls -la` dans le workspace puis explique le résultat.
```

Expected:

```text
ToolCall(pending)
→ permission_requested
→ permission decision
→ tool_execution_started
→ tool_result_received
→ tool_completed
```

No `state Terminal` rejection before execution/result.

## P1-005 — Permission rejection

```text
Exécute `ls -la` dans le workspace.
```

Reject the permission request.

Expected: terminal rejection/failure for that tool, no later execution or successful result event.

## P1-006 — Tool failure

```text
Essaie de lire `fichier-qui-nexiste-pas.txt`, puis explique l'erreur.
```

Expected: tool failure is represented semantically and the turn can continue or terminate coherently without a false success.

## P1-007 — Write then read

```text
Crée `phase1.txt` contenant exactement `phase 1`, puis relis ce fichier et confirme son contenu.
```

Expected: write tool and read tool have independent IDs and valid lifecycle ordering.

## P1-008 — Multiple writes

```text
Crée `a.txt` avec `A` et `b.txt` avec `B`, puis vérifie que les deux existent.
```

Expected: no duplicate-ID or cross-tool state corruption.

## P1-009 — Follow-up after tool

```text
Liste les fichiers, puis après le résultat explique précisément ce que tu ferais ensuite sans effectuer d'autre outil.
```

Expected: after the tool result, assistant continuation is emitted without a phantom second execution.

## P1-010 — Cancellation during tool activity

```text
Analyse le workspace et utilise les outils nécessaires pour produire un résumé détaillé.
```

Cancel the active turn while a tool or permission request is visible.

Expected: one terminal cancellation path; no successful completion afterwards; no orphan active tool.

## P1-011 — Cancellation with pending permission

```text
Exécute `ls -la` puis attends ma décision de permission.
```

Cancel while permission is pending.

Expected: cancellation invalidates the pending tool/turn consistently.

## P1-012 — Session continuation after tool turn

```text
Liste les fichiers du workspace.
```

After completion:

```text
Parmi les fichiers que tu viens de trouver, donne-moi uniquement le nom du premier fichier.
```

Expected: session context continues without replaying old tool execution.

## Phase 1 acceptance

A Phase 1 run should have zero unexpected semantic lifecycle transition errors in ACP stderr. Any failure must include the exact prompt and corresponding raw ACP evidence in `ACP_LOG.md`.
